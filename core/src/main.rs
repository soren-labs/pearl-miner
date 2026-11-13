#[cfg(unix)]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

use anyhow::{bail, Result};
use pearl_miner_core::config::{
    default_config_path, default_wallet_path, load_config, PoolKind,
};
use pearl_miner_core::engine::{
    detect_hardware, run_benchmark, run_daemon, run_mining_session,
};
use pearl_miner_core::pool_stats::MarketAndPoolSnapshot;
use pearl_miner_core::power::PowerTelemetry;
use pearl_miner_core::selftest::run_self_test;
use pearl_miner_core::wallet::{
    generate_wallet_data, load_and_verify_wallet, validate_payout_address,
    wallet_from_private_key_hex, wallet_from_watch_address, write_wallet_file,
};
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

fn print_help() {
    println!(
        "pearl-miner-core {} — Rust + Metal Pearl (PRL) miner for Apple Silicon\n\
         \n\
         USAGE:\n\
           pearl-miner-core daemon              Run JSON-RPC engine for PearlMiner macOS app\n\
           pearl-miner-core self-test           Verify Metal & V1/V2/V3 consensus proofs\n\
           pearl-miner-core benchmark [secs]    Offline GPU throughput & power benchmark\n\
           pearl-miner-core power               Read live SMC + IOReport power sensors\n\
           pearl-miner-core hardware            Print detected Apple Silicon Metal GPU\n\
           pearl-miner-core wallet new          Create a local prl1p... Taproot wallet\n\
           pearl-miner-core wallet show         Show local wallet address\n\
           pearl-miner-core wallet import-key <hex>\n\
           pearl-miner-core wallet connect <prl1p...>\n\
           pearl-miner-core mine [--pool luckypool|kryptex-eu|kryptex-us] [--address prl1p...] [--time-limit secs] [--max-accepted N]\n",
        env!("CARGO_PKG_VERSION")
    );
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        print_help();
        return Ok(());
    }

    match args[0].as_str() {
        "daemon" => run_daemon(),
        "self-test" | "--self-test" => {
            let report = run_self_test()?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        "benchmark" | "--benchmark" => {
            let secs = args
                .get(1)
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(15.0);
            let report = run_benchmark(secs)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        "hardware" => {
            let hw = detect_hardware()?;
            println!("{}", serde_json::to_string_pretty(&hw)?);
            Ok(())
        }
        "power" => {
            let mut t = PowerTelemetry::new();
            std::thread::sleep(std::time::Duration::from_millis(500));
            let hw = detect_hardware().ok();
            let dev = hw
                .as_ref()
                .map(|h| h.device_name.as_str())
                .unwrap_or("Apple Silicon");
            let r = t.sample(dev, 100);
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        "wallet" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("show");
            let path = default_wallet_path();
            match sub {
                "new" => {
                    if path.exists() {
                        bail!("wallet already exists at {}", path.display());
                    }
                    let w = generate_wallet_data()?;
                    write_wallet_file(&path, &w)?;
                    println!("Created wallet: {}", w.address);
                    println!("Saved to: {}", path.display());
                }
                "show" => {
                    let w = load_and_verify_wallet(&path)?;
                    println!("Address: {}", w.address);
                    println!("Mode: {:?}", w.mode);
                    println!("Path: {}", path.display());
                }
                "import-key" => {
                    let key = args.get(2).ok_or_else(|| anyhow::anyhow!("missing hex key"))?;
                    let w = wallet_from_private_key_hex(key)?;
                    write_wallet_file(&path, &w)?;
                    println!("Imported wallet: {}", w.address);
                }
                "connect" => {
                    let addr = args.get(2).ok_or_else(|| anyhow::anyhow!("missing address"))?;
                    let w = wallet_from_watch_address(addr)?;
                    write_wallet_file(&path, &w)?;
                    println!("Connected payout address: {}", w.address);
                }
                other => bail!("unknown wallet subcommand: {other}"),
            }
            Ok(())
        }
        "mine" => {
            let mut cfg = load_config(&default_config_path());
            let mut address: Option<String> = None;
            let mut time_limit: Option<f64> = None;
            let mut max_accepted: Option<u64> = None;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--pool" => {
                        i += 1;
                        let v = args.get(i).map(|s| s.as_str()).unwrap_or("");
                        cfg.pool = match v {
                            "luckypool" => PoolKind::Luckypool,
                            "kryptex" | "kryptex-eu" => PoolKind::KryptexEu,
                            "kryptex-us" => PoolKind::KryptexUs,
                            _ => bail!("unknown pool {v}"),
                        };
                    }
                    "--address" => {
                        i += 1;
                        address = args.get(i).cloned();
                    }
                    "--worker" => {
                        i += 1;
                        if let Some(w) = args.get(i) {
                            cfg.worker = w.clone();
                        }
                    }
                    "--time-limit" => {
                        i += 1;
                        time_limit = args.get(i).and_then(|s| s.parse().ok());
                    }
                    "--max-accepted" => {
                        i += 1;
                        max_accepted = args.get(i).and_then(|s| s.parse().ok());
                    }
                    other => bail!("unknown flag: {other}"),
                }
                i += 1;
            }
            let addr = match address {
                Some(a) => validate_payout_address(&a)?,
                None => load_and_verify_wallet(&default_wallet_path())?.address,
            };
            let stop = Arc::new(AtomicBool::new(false));
            let stop_sig = Arc::clone(&stop);
            unsafe {
                libc::signal(
                    libc::SIGINT,
                    sigint_handler as *const () as usize,
                );
            }
            std::thread::spawn(move || {
                while !SIGNALLED.load(Ordering::SeqCst) {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                stop_sig.store(true, Ordering::SeqCst);
            });
            let market = Arc::new(Mutex::new(MarketAndPoolSnapshot::default()));
            run_mining_session(cfg, addr, stop, market, None, max_accepted, time_limit)
        }
        other => {
            print_help();
            bail!("unknown command: {other}");
        }
    }
}

static SIGNALLED: AtomicBool = AtomicBool::new(false);
extern "C" fn sigint_handler(_: libc::c_int) {
    SIGNALLED.store(true, Ordering::SeqCst);
}
