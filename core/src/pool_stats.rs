use crate::config::DialectKind;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Debug, Default, Serialize)]
pub struct MarketAndPoolSnapshot {
    pub prl_price_usd: Option<f64>,
    pub network_hashrate_ehs: Option<f64>,
    pub pool_confirmed_prl: Option<f64>,
    pub pool_pending_prl: Option<f64>,
    pub pool_paid_prl: Option<f64>,
    pub pool_threshold_prl: Option<f64>,
    pub last_updated_utc: Option<String>,
}

fn http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(6))
        .timeout(Duration::from_secs(10))
        .user_agent("PearlMiner/0.1")
        .build()
}

pub fn fetch_snapshot(dialect: DialectKind, address: Option<&str>) -> MarketAndPoolSnapshot {
    let agent = http_agent();
    let mut snap = MarketAndPoolSnapshot {
        last_updated_utc: Some(crate::wallet::utc_now_iso()),
        ..Default::default()
    };

    // 1. Live PRL/USDT spot price from Gate.io
    if let Ok(resp) = agent
        .get("https://api.gateio.ws/api/v4/spot/tickers?currency_pair=PEARL_USDT")
        .call()
    {
        if let Ok(val) = resp.into_json::<Value>() {
            if let Some(arr) = val.as_array() {
                if let Some(first) = arr.first() {
                    if let Some(last_str) = first.get("last").and_then(|v| v.as_str()) {
                        if let Ok(p) = last_str.parse::<f64>() {
                            if p.is_finite() && p > 0.0 {
                                snap.prl_price_usd = Some(p);
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Network hashrate + pool info from Kryptex public API (and LuckyPool fallback)
    if let Ok(resp) = agent
        .get("https://pool.kryptex.com/prl/api/v1/pool/info")
        .call()
    {
        if let Ok(val) = resp.into_json::<Value>() {
            let net_hs = val
                .get("network_hashrate")
                .or_else(|| val.get("networkHashrate"))
                .and_then(|v| v.as_f64());
            if let Some(hs) = net_hs {
                if hs.is_finite() && hs > 0.0 {
                    snap.network_hashrate_ehs = Some(hs / 1e18);
                }
            }
        }
    }
    if snap.network_hashrate_ehs.is_none() {
        if let Ok(resp) = agent.get("https://pearl.luckypool.io/api/stats").call() {
            if let Ok(val) = resp.into_json::<Value>() {
                let diff = val
                    .get("network")
                    .and_then(|n| n.get("difficulty"))
                    .and_then(|v| v.as_f64());
                if let Some(d) = diff {
                    if d.is_finite() && d > 0.0 {
                        // hashrate ≈ difficulty / block_time (120s)
                        snap.network_hashrate_ehs = Some((d / 120.0) / 1e18);
                    }
                }
            }
        }
    }

    // 3. Wallet pool balance if address is set
    if let Some(addr) = address {
        match dialect {
            DialectKind::Luckypool => {
                let url = format!("https://pearl.luckypool.io/api/stats_address?address={addr}");
                if let Ok(resp) = agent.get(&url).call() {
                    if let Ok(val) = resp.into_json::<Value>() {
                        if let Some(stats) = val.get("stats") {
                            let atomic = |k: &str| -> Option<f64> {
                                let raw = stats.get(k)?;
                                let num = if let Some(s) = raw.as_str() {
                                    s.parse::<f64>().ok()?
                                } else {
                                    raw.as_f64()?
                                };
                                Some(num / 1e8)
                            };
                            snap.pool_pending_prl = atomic("locked").or(Some(0.0));
                            snap.pool_confirmed_prl = atomic("unlocked").or(Some(0.0));
                            snap.pool_paid_prl = atomic("paid").or(Some(0.0));
                            snap.pool_threshold_prl = stats
                                .get("minPayoutLevel")
                                .and_then(|v| v.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| v.as_f64()))
                                .map(|v| if v > 1000.0 { v / 1e8 } else { v })
                                .or(Some(5.0));
                        }
                    }
                }
            }
            DialectKind::Kryptex => {
                let url = format!("https://pool.kryptex.com/prl/api/v1/miner/balance/{addr}");
                if let Ok(resp) = agent.get(&url).call() {
                    if let Ok(val) = resp.into_json::<Value>() {
                        snap.pool_pending_prl = val.get("unconfirmed").and_then(|v| v.as_f64());
                        snap.pool_confirmed_prl = val.get("confirmed").and_then(|v| v.as_f64());
                        snap.pool_paid_prl = val.get("paid").and_then(|v| v.as_f64()).or(Some(0.0));
                        snap.pool_threshold_prl = val.get("threshold").and_then(|v| v.as_f64()).or(Some(1.0));
                    }
                }
            }
        }
    }

    snap
}
