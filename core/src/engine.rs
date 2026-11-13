use crate::config::{
    default_config_path, default_wallet_path, load_config, save_config, BatteryPolicy, MinerConfig,
};
use crate::economics::{compute_economics, EconomicsSnapshot};
use crate::grid::{
    plain_proof_to_base64, scaled_bound_le, verify_share_local, Grid, GridFactory, JobShape,
    SEED_LABEL_A, SEED_LABEL_B,
};
use crate::metal::{MetalBuffer, MetalContext, PmShape, HITS_BUF_BYTES, HITS_CAPACITY};
use crate::pool_stats::{fetch_snapshot, MarketAndPoolSnapshot};
use crate::power::{hid_idle_seconds, PowerReading, PowerTelemetry};
use crate::selftest::{run_self_test, SelfTestReport};
use crate::stratum::{backoff_delay, Job, PoolConnection, PoolEvent};
use crate::wallet::{
    generate_wallet_data, load_and_verify_wallet, validate_payout_address,
    wallet_from_private_key_hex, wallet_from_watch_address, write_wallet_file, WalletSummary,
};
use anyhow::Result;
use primitive_types::U256;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const BATTERY_LOW_INTENSITY: u8 = 25;
const BATTERY_POLL_SECS: u64 = 15;
const WATCHDOG_MAX_SILENCE_SECS: f64 = 300.0;

pub struct RateMeter {
    samples: VecDeque<(Instant, u64)>,
    total: u64,
    started: Instant,
}

impl RateMeter {
    pub fn new() -> Self {
        Self {
            samples: VecDeque::new(),
            total: 0,
            started: Instant::now(),
        }
    }

    pub fn add(&mut self, tiles: u64) {
        let now = Instant::now();
        self.total += tiles;
        self.samples.push_back((now, tiles));
        while let Some(&(t, _)) = self.samples.front() {
            if now.duration_since(t) > Duration::from_secs(60) {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn rolling_tiles_per_s(&self) -> f64 {
        if self.samples.len() < 2 {
            return self.average_tiles_per_s();
        }
        let first_t = self.samples.front().unwrap().0;
        let last_t = self.samples.back().unwrap().0;
        let dt = last_t.duration_since(first_t).as_secs_f64();
        if dt <= 0.05 {
            return self.average_tiles_per_s();
        }
        let sum: u64 = self.samples.iter().skip(1).map(|&(_, n)| n).sum();
        (sum as f64) / dt
    }

    pub fn average_tiles_per_s(&self) -> f64 {
        let dt = self.started.elapsed().as_secs_f64();
        if dt <= 0.05 {
            0.0
        } else {
            (self.total as f64) / dt
        }
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }
}

struct MetalEngine {
    metal: MetalContext,
    device_name: String,
    kernel_name: String,
    shape: JobShape,
    fast_path: bool,
    n_bands: usize,
    n_cb: usize,
    region_rows: usize,
    a_buf: MetalBuffer,
    bt_buf: MetalBuffer,
    an_buf: MetalBuffer,
    bnt_buf: MetalBuffer,
    ua_buf: MetalBuffer,
    ub_buf: MetalBuffer,
    pa_buf: MetalBuffer,
    pb_buf: MetalBuffer,
    cb_buf: MetalBuffer,
    rb_slice: MetalBuffer,
    hits_buf: MetalBuffer,
    loaded_b_id: Option<u64>,
}

impl MetalEngine {
    fn new(preferred_shape: Option<JobShape>) -> Result<Self> {
        let metal = MetalContext::new()?;
        let info = metal.device_info()?;
        // Compile probe shape first to detect whether M6 tensor kernel is active
        let probe_shape = JobShape::new(8192, 8192, 2048, 128)?;
        let pm_probe = PmShape::from_patterns(
            probe_shape.k as u32,
            probe_shape.r as u32,
            &probe_shape.rows_pattern,
            &probe_shape.cols_pattern,
        );
        metal.compile(&pm_probe)?;
        let kernel_name = metal.sweep_kernel_name();

        let shape = match preferred_shape {
            Some(s) => s,
            None => JobShape::preset_for_kernel(&kernel_name)?,
        };
        let pm_shape = PmShape::from_patterns(
            shape.k as u32,
            shape.r as u32,
            &shape.rows_pattern,
            &shape.cols_pattern,
        );
        metal.compile(&pm_shape)?;

        let m = shape.m;
        let n = shape.n;
        let k = shape.k;
        let r = shape.r;

        let a_buf = metal.alloc(m * k)?;
        let bt_buf = metal.alloc(n * k)?;
        let an_buf = metal.alloc(m * k)?;
        let bnt_buf = metal.alloc(n * k)?;
        let ua_buf = metal.alloc(m * r)?;
        let ub_buf = metal.alloc(n * r)?;
        let pa_buf = metal.alloc(k * 8)?;
        let pb_buf = metal.alloc(k * 8)?;
        let cb_buf = metal.alloc_u32_slice(&shape.col_bases)?;
        let rb_slice = metal.alloc(shape.row_bases.len() * 4)?;
        let hits_buf = metal.alloc(HITS_BUF_BYTES)?;

        let fast_path = shape.is_fast_path();
        let region_rows = if kernel_name == "pow_sweep_m6_tensor" {
            2048
        } else {
            256
        };

        Ok(Self {
            metal,
            device_name: info.name,
            kernel_name,
            fast_path,
            n_bands: m / 64,
            n_cb: n / 64,
            region_rows,
            a_buf,
            bt_buf,
            an_buf,
            bnt_buf,
            ua_buf,
            ub_buf,
            pa_buf,
            pb_buf,
            cb_buf,
            rb_slice,
            hits_buf,
            shape,
            loaded_b_id: None,
        })
    }

    fn load_grid(&mut self, grid: &Grid) -> Result<()> {
        let reuse_b = self.loaded_b_id == Some(grid.b_state.id);
        if !reuse_b {
            self.loaded_b_id = None;
        }
        let m = self.shape.m as u32;
        let n = self.shape.n as u32;
        self.a_buf.as_mut_slice()[..grid.a_bytes.len()].copy_from_slice(&grid.a_bytes);
        if !reuse_b {
            self.bt_buf.as_mut_slice()[..grid.b_state.bt_bytes.len()]
                .copy_from_slice(&grid.b_state.bt_bytes);
        }

        self.metal
            .noise_uniform(&SEED_LABEL_A, &grid.a_seed, &mut self.ua_buf, m)?;
        self.metal
            .noise_pairs(&SEED_LABEL_A, &grid.a_seed, &mut self.pa_buf)?;
        if !reuse_b {
            self.metal
                .noise_uniform(&SEED_LABEL_B, &grid.b_state.b_seed, &mut self.ub_buf, n)?;
            self.metal
                .noise_pairs(&SEED_LABEL_B, &grid.b_state.b_seed, &mut self.pb_buf)?;
        }

        self.metal
            .noise_apply(&self.a_buf, &self.ua_buf, &self.pa_buf, &mut self.an_buf, m)?;
        if !reuse_b {
            self.metal.noise_apply(
                &self.bt_buf,
                &self.ub_buf,
                &self.pb_buf,
                &mut self.bnt_buf,
                n,
            )?;
        }

        self.loaded_b_id = Some(grid.b_state.id);
        Ok(())
    }

    fn n_regions(&self) -> usize {
        if self.fast_path {
            let bands_per = (self.region_rows / 32).max(1);
            self.n_bands.div_ceil(bands_per)
        } else {
            self.shape.row_bases.len().div_ceil(self.region_rows)
        }
    }

    fn sweep_region(
        &mut self,
        idx: usize,
        a_seed: &[u8; 32],
        bound_le: &[u8; 32],
    ) -> Result<(Vec<(u32, u32)>, u64)> {
        self.hits_buf.as_u32_mut_slice()[0] = 0;
        let n_tiles: u64;
        if self.fast_path {
            let bands_per = (self.region_rows / 32).max(1);
            let band_lo = idx * bands_per;
            let n_bands = bands_per.min(self.n_bands.saturating_sub(band_lo));
            self.metal.pow_sweep2(
                &self.an_buf,
                &self.bnt_buf,
                band_lo as u32,
                n_bands as u32,
                self.n_cb as u32,
                a_seed,
                bound_le,
                &mut self.hits_buf,
                None,
            )?;
            n_tiles = (n_bands * 32 * self.n_cb) as u64;
        } else {
            let lo = idx * self.region_rows;
            let hi = (lo + self.region_rows).min(self.shape.row_bases.len());
            let slice = &self.shape.row_bases[lo..hi];
            self.rb_slice.as_u32_mut_slice()[..slice.len()].copy_from_slice(slice);
            self.metal.pow_sweep(
                &self.an_buf,
                &self.bnt_buf,
                &self.rb_slice,
                slice.len() as u32,
                &self.cb_buf,
                self.shape.col_bases.len() as u32,
                a_seed,
                bound_le,
                &mut self.hits_buf,
                None,
            )?;
            n_tiles = (slice.len() * self.shape.col_bases.len()) as u64;
        }
        let u32s = self.hits_buf.as_u32_slice();
        let count = (u32s[0] as usize).min(HITS_CAPACITY);
        let mut hits = Vec::with_capacity(count);
        for i in 0..count {
            hits.push((u32s[1 + 2 * i], u32s[1 + 2 * i + 1]));
        }
        Ok((hits, n_tiles))
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct HardwareInfo {
    pub device_name: String,
    pub kernel_name: String,
    pub m: usize,
    pub n: usize,
    pub k: usize,
    pub r: usize,
    pub factor: u64,
}

pub fn detect_hardware() -> Result<HardwareInfo> {
    let engine = MetalEngine::new(None)?;
    Ok(HardwareInfo {
        device_name: engine.device_name,
        kernel_name: engine.kernel_name,
        m: engine.shape.m,
        n: engine.shape.n,
        k: engine.shape.k,
        r: engine.shape.r,
        factor: engine.shape.penalized_factor(),
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchmarkReport {
    pub device_name: String,
    pub kernel_name: String,
    pub seconds: f64,
    pub tiles_per_s: f64,
    pub equiv_ths: f64,
    pub power: PowerReading,
}

pub fn run_benchmark(seconds: f64) -> Result<BenchmarkReport> {
    let mut engine = MetalEngine::new(None)?;
    let factory = GridFactory::new();
    let header_bytes = vec![0x42u8; 76];
    let bound_zero = [0u8; 32];
    let mut telemetry = PowerTelemetry::new();

    // Warmup 3 seconds
    let warm_end = Instant::now() + Duration::from_secs(3);
    factory.prefetch(&engine.shape, &header_bytes, 3);
    while Instant::now() < warm_end {
        let grid = factory.take(&engine.shape, &header_bytes, 3)?;
        factory.prefetch(&engine.shape, &header_bytes, 3);
        engine.load_grid(&grid)?;
        for r in 0..engine.n_regions() {
            let _ = engine.sweep_region(r, &grid.a_seed, &bound_zero)?;
            if Instant::now() >= warm_end {
                break;
            }
        }
    }

    let _ = telemetry.sample(&engine.device_name, 100);
    let mut total_tiles: u64 = 0;
    let t0 = Instant::now();
    let deadline = t0 + Duration::from_secs_f64(seconds);
    while Instant::now() < deadline {
        let grid = factory.take(&engine.shape, &header_bytes, 3)?;
        factory.prefetch(&engine.shape, &header_bytes, 3);
        engine.load_grid(&grid)?;
        for r in 0..engine.n_regions() {
            let (_, n_tiles) = engine.sweep_region(r, &grid.a_seed, &bound_zero)?;
            total_tiles += n_tiles;
            if Instant::now() >= deadline {
                break;
            }
        }
    }
    let dt = t0.elapsed().as_secs_f64().max(0.001);
    let tiles_per_s = (total_tiles as f64) / dt;
    let equiv_ths = tiles_per_s * (engine.shape.penalized_factor() as f64) / 1e12;
    let power = telemetry.sample(&engine.device_name, 100);

    Ok(BenchmarkReport {
        device_name: engine.device_name,
        kernel_name: engine.kernel_name,
        seconds: dt,
        tiles_per_s,
        equiv_ths,
        power,
    })
}

fn spawn_caffeinate() -> Option<Child> {
    let pid = std::process::id().to_string();
    Command::new("caffeinate")
        .args(["-is", "-w", &pid])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonEvent {
    StateSnapshot {
        config: MinerConfig,
        wallet: Option<WalletSummary>,
        hardware: Option<HardwareInfo>,
        market: MarketAndPoolSnapshot,
        power: PowerReading,
        mining: bool,
    },
    Status {
        state: String,
        detail: String,
        job_id: Option<String>,
        height: Option<u64>,
        cert_version: Option<u32>,
        uptime_s: u64,
    },
    Metrics {
        tiles_per_s: f64,
        avg_tiles_per_s: f64,
        equiv_ths: f64,
        accepted: u64,
        rejected: u64,
        pending: usize,
        last_share_ago_s: Option<u64>,
        est_next_share_s: Option<f64>,
        intensity_now: u8,
        power: PowerReading,
        economics: EconomicsSnapshot,
    },
    Market {
        market: MarketAndPoolSnapshot,
    },
    Power {
        power: PowerReading,
    },
    WalletUpdated {
        wallet: Option<WalletSummary>,
        revealed_private_key: Option<String>,
    },
    SelfTestResult {
        report: SelfTestReport,
    },
    BenchmarkResult {
        report: BenchmarkReport,
    },
    Log {
        level: String,
        message: String,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum DaemonCommand {
    GetState,
    Start,
    Stop,
    UpdateConfig { config: MinerConfig },
    WalletCreate,
    WalletImportKey { private_key_hex: String },
    WalletImportFile { path: String },
    WalletConnectAddress { address: String },
    WalletRevealKey,
    SelfTest,
    Benchmark { seconds: Option<f64> },
    RefreshMarket,
    Quit,
}

fn emit(out: &Arc<Mutex<std::io::Stdout>>, ev: &DaemonEvent) {
    if let Ok(line) = serde_json::to_string(ev) {
        let mut guard = out.lock().unwrap();
        let _ = writeln!(guard, "{line}");
        let _ = guard.flush();
    }
}

fn current_wallet_summary() -> Option<WalletSummary> {
    let p = default_wallet_path();
    if !p.exists() {
        return None;
    }
    load_and_verify_wallet(&p).ok().map(|w| WalletSummary {
        address: w.address,
        mode: w.mode,
        has_private_key: w.private_key_hex.is_some(),
        created_utc: w.created_utc,
        path: p.display().to_string(),
    })
}

pub fn run_mining_session(
    cfg: MinerConfig,
    address: String,
    stop_flag: Arc<AtomicBool>,
    market_cache: Arc<Mutex<MarketAndPoolSnapshot>>,
    out: Option<Arc<Mutex<std::io::Stdout>>>,
    max_accepted: Option<u64>,
    time_limit_s: Option<f64>,
) -> Result<()> {
    let emit_or_print = |ev: DaemonEvent| {
        if let Some(ref o) = out {
            emit(o, &ev);
        } else {
            match &ev {
                DaemonEvent::Log { level, message } => {
                    println!("[{level}] {message}");
                }
                DaemonEvent::Status { state, detail, .. } => {
                    println!("[status] {state}: {detail}");
                }
                DaemonEvent::Metrics {
                    equiv_ths,
                    accepted,
                    rejected,
                    power,
                    economics,
                    ..
                } => {
                    println!(
                        "[stats] {equiv_ths:.2} TH/s | acc={accepted} rej={rejected} | sys={:.1}W gpu={:.1}W | est={:.3} PRL/d (${:.2}/d)",
                        power.system_watts.unwrap_or(0.0),
                        power.gpu_watts.unwrap_or(0.0),
                        economics.prl_per_day,
                        economics.gross_usd_per_day
                    );
                }
                DaemonEvent::Error { message } => {
                    eprintln!("[error] {message}");
                }
                _ => {}
            }
        }
    };

    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(cfg.cpu_threads.clamp(1, 32))
        .build_global();

    let mut caffeinate_child = if cfg.keep_awake {
        spawn_caffeinate()
    } else {
        None
    };

    let mut engine = MetalEngine::new(None)?;
    let factor = engine.shape.penalized_factor();
    let (dialect, host, port) = cfg.effective_endpoint();

    emit_or_print(DaemonEvent::Log {
        level: "info".into(),
        message: format!(
            "Metal initialized: {} ({}, {}×{}×{})",
            engine.device_name, engine.kernel_name, engine.shape.m, engine.shape.n, engine.shape.k
        ),
    });

    emit_or_print(DaemonEvent::Status {
        state: "connecting".into(),
        detail: format!("{host}:{port}"),
        job_id: None,
        height: None,
        cert_version: None,
        uptime_s: 0,
    });

    let mut conn = PoolConnection::new(dialect, host.clone(), port, address.clone(), cfg.worker.clone());
    if let Err(e) = conn.connect() {
        emit_or_print(DaemonEvent::Status {
            state: "error".into(),
            detail: e.to_string(),
            job_id: None,
            height: None,
            cert_version: None,
            uptime_s: 0,
        });
        if let Some(ref mut c) = caffeinate_child {
            let _ = c.kill();
        }
        return Err(e);
    }

    let factory = GridFactory::new();
    let mut telemetry = PowerTelemetry::new();
    let mut meter = RateMeter::new();
    let mut job: Option<Job> = None;
    let mut grid: Option<Grid> = None;
    let mut bound_le = [0u8; 32];
    let mut exp_tiles: Option<f64> = None;
    let mut row_cursor: usize = 0;
    let mut pending: HashMap<u64, String> = HashMap::new();
    let mut accepted: u64 = 0;
    let mut rejected: u64 = 0;
    let mut last_share_t: Option<Instant> = None;
    let mut reconnect_attempt: u32 = 0;
    let mut last_metrics_t = Instant::now() - Duration::from_secs(10);
    let mut last_batt_t = Instant::now() - Duration::from_secs(BATTERY_POLL_SECS + 1);
    let mut on_batt = false;
    let mut last_idle_t = Instant::now();
    let mut idle_full = false;
    let mut last_power: PowerReading;

    while !stop_flag.load(Ordering::SeqCst) {
        if let Some(limit) = time_limit_s {
            if meter.uptime_secs() as f64 >= limit {
                break;
            }
        }
        if let Some(max_acc) = max_accepted {
            if accepted >= max_acc {
                break;
            }
        }

        if conn.dead.load(Ordering::SeqCst) {
            reconnect_attempt += 1;
            let delay = backoff_delay(reconnect_attempt);
            emit_or_print(DaemonEvent::Status {
                state: "reconnecting".into(),
                detail: format!("Attempt #{reconnect_attempt} in {}s", delay.as_secs()),
                job_id: job.as_ref().map(|j| j.job_id.clone()),
                height: job.as_ref().map(|j| j.height),
                cert_version: job.as_ref().map(|j| j.cert_version),
                uptime_s: meter.uptime_secs(),
            });
            let end_wait = Instant::now() + delay;
            while Instant::now() < end_wait {
                if stop_flag.load(Ordering::SeqCst) {
                    break;
                }
                thread::sleep(Duration::from_millis(200));
            }
            if stop_flag.load(Ordering::SeqCst) {
                break;
            }
            match conn.connect() {
                Ok(()) => {
                    pending.clear();
                    job = None;
                    grid = None;
                    reconnect_attempt = 0;
                    emit_or_print(DaemonEvent::Log {
                        level: "info".into(),
                        message: format!("Reconnected to {host}:{port}"),
                    });
                }
                Err(e) => {
                    emit_or_print(DaemonEvent::Log {
                        level: "warn".into(),
                        message: format!("Reconnect attempt #{reconnect_attempt} failed: {e}"),
                    });
                }
            }
            continue;
        }

        if conn.silence_seconds() > WATCHDOG_MAX_SILENCE_SECS {
            emit_or_print(DaemonEvent::Log {
                level: "warn".into(),
                message: "Pool silence watchdog triggered — reconnecting".into(),
            });
            conn.close();
            continue;
        }

        if last_batt_t.elapsed() >= Duration::from_secs(BATTERY_POLL_SECS) {
            last_batt_t = Instant::now();
            on_batt = crate::power::is_on_battery();
        }

        // Drain pool events
        let mut new_job = false;
        let first_ev = if job.is_none() {
            conn.recv_timeout(Duration::from_millis(250))
        } else {
            conn.try_recv()
        };
        let mut ev_opt = first_ev;
        while let Some(ev) = ev_opt {
            match ev {
                PoolEvent::Job(j) => {
                    if job.as_ref() != Some(&j) {
                        new_job = true;
                        job = Some(j);
                    }
                }
                PoolEvent::Share(sr) => {
                    if let Some(jid) = pending.remove(&sr.msg_id) {
                        if sr.accepted {
                            accepted += 1;
                            last_share_t = Some(Instant::now());
                            emit_or_print(DaemonEvent::Log {
                                level: "info".into(),
                                message: format!("Share ACCEPTED (job {jid}) — total {accepted}"),
                            });
                        } else {
                            rejected += 1;
                            emit_or_print(DaemonEvent::Log {
                                level: "warn".into(),
                                message: format!("Share REJECTED (job {jid}): {}", sr.raw),
                            });
                        }
                    }
                }
                PoolEvent::Disconnected(reason) => {
                    emit_or_print(DaemonEvent::Log {
                        level: "warn".into(),
                        message: format!("Pool disconnected: {reason}"),
                    });
                }
            }
            ev_opt = conn.try_recv();
        }

        let Some(ref cur_job) = job else {
            continue;
        };

        if on_batt && cfg.on_battery == BatteryPolicy::Pause {
            grid = None;
            emit_or_print(DaemonEvent::Status {
                state: "paused_battery".into(),
                detail: "Paused on battery".into(),
                job_id: Some(cur_job.job_id.clone()),
                height: Some(cur_job.height),
                cert_version: Some(cur_job.cert_version),
                uptime_s: meter.uptime_secs(),
            });
            thread::sleep(Duration::from_secs(1));
            continue;
        }

        if new_job || grid.is_none() {
            let Some((b_le, b_u256)) = scaled_bound_le(cur_job.target, &engine.shape.mining_config)
            else {
                emit_or_print(DaemonEvent::Log {
                    level: "warn".into(),
                    message: "Pool target overflowed 2^256; waiting for next job".into(),
                });
                job = None;
                continue;
            };
            bound_le = b_le;
            let bound_f64 = u256_to_f64(b_u256);
            exp_tiles = if bound_f64 > 0.0 {
                Some(2.0f64.powi(256) / bound_f64)
            } else {
                None
            };
            let built = factory.take(&engine.shape, &cur_job.header_bytes, cur_job.cert_version)?;
            factory.prefetch(&engine.shape, &cur_job.header_bytes, cur_job.cert_version);
            engine.load_grid(&built)?;
            row_cursor = 0;
            grid = Some(built);

            if new_job {
                emit_or_print(DaemonEvent::Status {
                    state: "mining".into(),
                    detail: format!("{host}:{port}"),
                    job_id: Some(cur_job.job_id.clone()),
                    height: Some(cur_job.height),
                    cert_version: Some(cur_job.cert_version),
                    uptime_s: meter.uptime_secs(),
                });
            }
        }

        let cur_grid = grid.as_ref().unwrap();
        let t_burst = Instant::now();
        let (hits, n_tiles) = engine.sweep_region(row_cursor, &cur_grid.a_seed, &bound_le)?;
        let burst_dt = t_burst.elapsed();
        meter.add(n_tiles);
        row_cursor += 1;

        for (base_r, base_c) in hits {
            if conn.dead.load(Ordering::SeqCst) {
                break;
            }
            match cur_grid.craft_plain_proof(base_r, base_c) {
                Ok(proof) => {
                    if let Err(e) = verify_share_local(
                        &cur_job.header_bytes,
                        &proof,
                        cur_job.target,
                        cur_job.cert_version,
                    ) {
                        emit_or_print(DaemonEvent::Log {
                            level: "warn".into(),
                            message: format!("Local share verify failed ({base_r},{base_c}): {e}"),
                        });
                        continue;
                    }
                    match plain_proof_to_base64(&proof)
                        .and_then(|b64| conn.submit(&cur_job.job_id, &b64))
                    {
                        Ok(mid) => {
                            pending.insert(mid, cur_job.job_id.clone());
                            emit_or_print(DaemonEvent::Log {
                                level: "info".into(),
                                message: format!(
                                    "Share found ({base_r},{base_c}) -> verified -> submitted (msg {mid})"
                                ),
                            });
                        }
                        Err(e) => {
                            emit_or_print(DaemonEvent::Log {
                                level: "warn".into(),
                                message: format!("Share submit failed: {e}"),
                            });
                        }
                    }
                }
                Err(e) => {
                    emit_or_print(DaemonEvent::Log {
                        level: "warn".into(),
                        message: format!("Proof crafting failed: {e}"),
                    });
                }
            }
        }

        if row_cursor >= engine.n_regions() {
            grid = None;
        }

        let mut intensity = cfg.intensity.clamp(1, 100);
        if cfg.auto_intensity && last_idle_t.elapsed() >= Duration::from_secs(10) {
            last_idle_t = Instant::now();
            idle_full = hid_idle_seconds() > 300.0;
        }
        if cfg.auto_intensity && idle_full {
            intensity = 100;
        }
        if on_batt && cfg.on_battery == BatteryPolicy::Low {
            intensity = intensity.min(BATTERY_LOW_INTENSITY);
        }
        if intensity < 100 {
            let sleep_s = burst_dt.as_secs_f64() * ((100 - intensity) as f64) / (intensity as f64);
            thread::sleep(Duration::from_secs_f64(sleep_s.min(2.0)));
        }

        if last_metrics_t.elapsed() >= Duration::from_secs(2) {
            last_metrics_t = Instant::now();
            last_power = telemetry.sample(&engine.device_name, intensity);
            let rolling = meter.rolling_tiles_per_s();
            let equiv_ths = rolling * (factor as f64) / 1e12;
            let market = market_cache.lock().unwrap().clone();
            let price = market
                .prl_price_usd
                .unwrap_or(cfg.fallback_prl_price_usd);
            let net_ehs = market
                .network_hashrate_ehs
                .unwrap_or(cfg.fallback_network_hashrate_ehs);
            let is_live =
                market.prl_price_usd.is_some() || market.network_hashrate_ehs.is_some();
            let sys_w = last_power.system_watts.unwrap_or(0.0);
            let econ = compute_economics(
                rolling,
                factor,
                sys_w,
                cfg.electricity_usd_per_kwh,
                price,
                net_ehs,
                is_live,
            );

            emit_or_print(DaemonEvent::Status {
                state: "mining".into(),
                detail: format!("{host}:{port}"),
                job_id: Some(cur_job.job_id.clone()),
                height: Some(cur_job.height),
                cert_version: Some(cur_job.cert_version),
                uptime_s: meter.uptime_secs(),
            });
            emit_or_print(DaemonEvent::Metrics {
                tiles_per_s: rolling,
                avg_tiles_per_s: meter.average_tiles_per_s(),
                equiv_ths,
                accepted,
                rejected,
                pending: pending.len(),
                last_share_ago_s: last_share_t.map(|t| t.elapsed().as_secs()),
                est_next_share_s: exp_tiles.and_then(|e| if rolling > 0.0 { Some(e / rolling) } else { None }),
                intensity_now: intensity,
                power: last_power.clone(),
                economics: econ,
            });
        }
    }

    conn.close();
    if let Some(ref mut c) = caffeinate_child {
        let _ = c.kill();
    }
    emit_or_print(DaemonEvent::Status {
        state: "stopped".into(),
        detail: "Ready".into(),
        job_id: None,
        height: None,
        cert_version: None,
        uptime_s: meter.uptime_secs(),
    });
    Ok(())
}

fn u256_to_f64(v: U256) -> f64 {
    let mut res = 0.0f64;
    for i in (0..4).rev() {
        res = res * (2.0f64.powi(64)) + (v.0[i] as f64);
    }
    res
}

pub fn run_daemon() -> Result<()> {
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let cfg_path = default_config_path();
    let cfg = Arc::new(Mutex::new(load_config(&cfg_path)));
    let market = Arc::new(Mutex::new(MarketAndPoolSnapshot::default()));

    let hardware = detect_hardware().ok();
    let dev_name = hardware
        .as_ref()
        .map(|h| h.device_name.clone())
        .unwrap_or_else(|| "Apple Silicon GPU".to_string());

    let mut telemetry = PowerTelemetry::new();
    let init_power = telemetry.sample(&dev_name, 0);

    // Background market + pool balance poller
    {
        let cfg_m = Arc::clone(&cfg);
        let market_m = Arc::clone(&market);
        let out_m = Arc::clone(&out);
        thread::spawn(move || loop {
            let cur_cfg = cfg_m.lock().unwrap().clone();
            if cur_cfg.live_market_sync {
                let (dialect, _, _) = cur_cfg.effective_endpoint();
                let addr = current_wallet_summary().map(|w| w.address);
                let snap = fetch_snapshot(dialect, addr.as_deref());
                *market_m.lock().unwrap() = snap.clone();
                emit(&out_m, &DaemonEvent::Market { market: snap });
            }
            thread::sleep(Duration::from_secs(60));
        });
    }

    // Background idle power sampler (pauses when mining loop owns telemetry)
    let is_mining = Arc::new(AtomicBool::new(false));
    {
        let is_mining_p = Arc::clone(&is_mining);
        let out_p = Arc::clone(&out);
        let dev_p = dev_name.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(2));
            if !is_mining_p.load(Ordering::SeqCst) {
                let p = telemetry.sample(&dev_p, 0);
                emit(&out_p, &DaemonEvent::Power { power: p });
            }
        });
    }

    emit(
        &out,
        &DaemonEvent::StateSnapshot {
            config: cfg.lock().unwrap().clone(),
            wallet: current_wallet_summary(),
            hardware: hardware.clone(),
            market: market.lock().unwrap().clone(),
            power: init_power,
            mining: false,
        },
    );

    let mut mining_stop: Option<Arc<AtomicBool>> = None;
    let mut mining_handle: Option<thread::JoinHandle<()>> = None;

    let stdin = std::io::stdin();
    for line_res in stdin.lock().lines() {
        let Ok(line) = line_res else { break };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let cmd: DaemonCommand = match serde_json::from_str(trimmed) {
            Ok(c) => c,
            Err(e) => {
                emit(
                    &out,
                    &DaemonEvent::Error {
                        message: format!("Invalid command: {e}"),
                    },
                );
                continue;
            }
        };

        match cmd {
            DaemonCommand::GetState => {
                let p = PowerTelemetry::new().sample(&dev_name, 0);
                emit(
                    &out,
                    &DaemonEvent::StateSnapshot {
                        config: cfg.lock().unwrap().clone(),
                        wallet: current_wallet_summary(),
                        hardware: hardware.clone(),
                        market: market.lock().unwrap().clone(),
                        power: p,
                        mining: is_mining.load(Ordering::SeqCst),
                    },
                );
            }
            DaemonCommand::UpdateConfig { config: new_cfg } => {
                if let Err(e) = save_config(&cfg_path, &new_cfg) {
                    emit(
                        &out,
                        &DaemonEvent::Error {
                            message: format!("Failed to save config: {e}"),
                        },
                    );
                } else {
                    *cfg.lock().unwrap() = new_cfg.clone();
                    if new_cfg.live_market_sync {
                        let market_m = Arc::clone(&market);
                        let out_m = Arc::clone(&out);
                        thread::spawn(move || {
                            let (dialect, _, _) = new_cfg.effective_endpoint();
                            let addr = current_wallet_summary().map(|w| w.address);
                            let snap = fetch_snapshot(dialect, addr.as_deref());
                            *market_m.lock().unwrap() = snap.clone();
                            emit(&out_m, &DaemonEvent::Market { market: snap });
                        });
                    }
                }
            }
            DaemonCommand::WalletCreate => match generate_wallet_data() {
                Ok(w) => {
                    let p = default_wallet_path();
                    if let Err(e) = write_wallet_file(&p, &w) {
                        emit(&out, &DaemonEvent::Error { message: e.to_string() });
                    } else {
                        emit(
                            &out,
                            &DaemonEvent::WalletUpdated {
                                wallet: current_wallet_summary(),
                                revealed_private_key: w.private_key_hex,
                            },
                        );
                    }
                }
                Err(e) => emit(&out, &DaemonEvent::Error { message: e.to_string() }),
            },
            DaemonCommand::WalletImportKey { private_key_hex } => {
                match wallet_from_private_key_hex(&private_key_hex) {
                    Ok(w) => {
                        let p = default_wallet_path();
                        if let Err(e) = write_wallet_file(&p, &w) {
                            emit(&out, &DaemonEvent::Error { message: e.to_string() });
                        } else {
                            emit(
                                &out,
                                &DaemonEvent::WalletUpdated {
                                    wallet: current_wallet_summary(),
                                    revealed_private_key: None,
                                },
                            );
                        }
                    }
                    Err(e) => emit(&out, &DaemonEvent::Error { message: e.to_string() }),
                }
            }
            DaemonCommand::WalletImportFile { path } => {
                match load_and_verify_wallet(&PathBuf::from(path)) {
                    Ok(w) => {
                        let p = default_wallet_path();
                        if let Err(e) = write_wallet_file(&p, &w) {
                            emit(&out, &DaemonEvent::Error { message: e.to_string() });
                        } else {
                            emit(
                                &out,
                                &DaemonEvent::WalletUpdated {
                                    wallet: current_wallet_summary(),
                                    revealed_private_key: None,
                                },
                            );
                        }
                    }
                    Err(e) => emit(&out, &DaemonEvent::Error { message: e.to_string() }),
                }
            }
            DaemonCommand::WalletConnectAddress { address } => {
                match wallet_from_watch_address(&address) {
                    Ok(w) => {
                        let p = default_wallet_path();
                        if let Err(e) = write_wallet_file(&p, &w) {
                            emit(&out, &DaemonEvent::Error { message: e.to_string() });
                        } else {
                            emit(
                                &out,
                                &DaemonEvent::WalletUpdated {
                                    wallet: current_wallet_summary(),
                                    revealed_private_key: None,
                                },
                            );
                        }
                    }
                    Err(e) => emit(&out, &DaemonEvent::Error { message: e.to_string() }),
                }
            }
            DaemonCommand::WalletRevealKey => {
                let p = default_wallet_path();
                match load_and_verify_wallet(&p) {
                    Ok(w) => emit(
                        &out,
                        &DaemonEvent::WalletUpdated {
                            wallet: current_wallet_summary(),
                            revealed_private_key: w.private_key_hex,
                        },
                    ),
                    Err(e) => emit(&out, &DaemonEvent::Error { message: e.to_string() }),
                }
            }
            DaemonCommand::RefreshMarket => {
                let cur_cfg = cfg.lock().unwrap().clone();
                let market_m = Arc::clone(&market);
                let out_m = Arc::clone(&out);
                thread::spawn(move || {
                    let (dialect, _, _) = cur_cfg.effective_endpoint();
                    let addr = current_wallet_summary().map(|w| w.address);
                    let snap = fetch_snapshot(dialect, addr.as_deref());
                    *market_m.lock().unwrap() = snap.clone();
                    emit(&out_m, &DaemonEvent::Market { market: snap });
                });
            }
            DaemonCommand::SelfTest => {
                let out_s = Arc::clone(&out);
                thread::spawn(move || match run_self_test() {
                    Ok(report) => emit(&out_s, &DaemonEvent::SelfTestResult { report }),
                    Err(e) => emit(&out_s, &DaemonEvent::Error { message: e.to_string() }),
                });
            }
            DaemonCommand::Benchmark { seconds } => {
                if is_mining.load(Ordering::SeqCst) {
                    emit(
                        &out,
                        &DaemonEvent::Error {
                            message: "Stop mining before running benchmark".into(),
                        },
                    );
                } else {
                    let out_b = Arc::clone(&out);
                    let secs = seconds.unwrap_or(10.0).clamp(3.0, 120.0);
                    thread::spawn(move || match run_benchmark(secs) {
                        Ok(report) => emit(&out_b, &DaemonEvent::BenchmarkResult { report }),
                        Err(e) => emit(&out_b, &DaemonEvent::Error { message: e.to_string() }),
                    });
                }
            }
            DaemonCommand::Start => {
                if is_mining.load(Ordering::SeqCst) {
                    continue;
                }
                let Some(w) = current_wallet_summary() else {
                    emit(
                        &out,
                        &DaemonEvent::Error {
                            message: "No wallet connected. Create or import a wallet first.".into(),
                        },
                    );
                    continue;
                };
                if let Err(e) = validate_payout_address(&w.address) {
                    emit(&out, &DaemonEvent::Error { message: e.to_string() });
                    continue;
                }

                let stop = Arc::new(AtomicBool::new(false));
                mining_stop = Some(Arc::clone(&stop));
                is_mining.store(true, Ordering::SeqCst);

                let cur_cfg = cfg.lock().unwrap().clone();
                let market_m = Arc::clone(&market);
                let out_m = Arc::clone(&out);
                let is_mining_m = Arc::clone(&is_mining);
                mining_handle = Some(thread::spawn(move || {
                    if let Err(e) = run_mining_session(
                        cur_cfg,
                        w.address,
                        stop,
                        market_m,
                        Some(out_m.clone()),
                        None,
                        None,
                    ) {
                        emit(&out_m, &DaemonEvent::Error { message: e.to_string() });
                    }
                    is_mining_m.store(false, Ordering::SeqCst);
                }));
            }
            DaemonCommand::Stop => {
                if let Some(stop) = mining_stop.take() {
                    stop.store(true, Ordering::SeqCst);
                }
                if let Some(h) = mining_handle.take() {
                    let _ = h.join();
                }
                is_mining.store(false, Ordering::SeqCst);
            }
            DaemonCommand::Quit => {
                if let Some(stop) = mining_stop.take() {
                    stop.store(true, Ordering::SeqCst);
                }
                if let Some(h) = mining_handle.take() {
                    let _ = h.join();
                }
                break;
            }
        }
    }

    if let Some(stop) = mining_stop.take() {
        stop.store(true, Ordering::SeqCst);
    }
    if let Some(h) = mining_handle.take() {
        let _ = h.join();
    }
    Ok(())
}
