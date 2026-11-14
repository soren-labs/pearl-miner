use crate::config::{MinerConfig, PoolKind};
use serde::Serialize;
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Default, Serialize)]
pub struct MarketAndPoolSnapshot {
    pub prl_price_usd: Option<f64>,
    pub network_hashrate_ehs: Option<f64>,
    pub block_reward_prl: Option<f64>,
    pub block_time_s: Option<f64>,
    pub pool_fee_fraction: Option<f64>,
    pub pool_confirmed_prl: Option<f64>,
    pub pool_pending_prl: Option<f64>,
    pub pool_paid_prl: Option<f64>,
    pub pool_threshold_prl: Option<f64>,
    pub wallet_address: Option<String>,
    pub last_updated_utc: Option<String>,
    pub fetched_at_unix: u64,
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
impl MarketAndPoolSnapshot {
    pub fn is_fresh(&self) -> bool {
        self.fetched_at_unix > 0 && now_unix().saturating_sub(self.fetched_at_unix) <= 180
    }
}
pub fn number(v: &Value) -> Option<f64> {
    let n = v.as_f64().or_else(|| v.as_str()?.parse().ok())?;
    (n.is_finite() && n >= 0.0).then_some(n)
}
fn positive(v: Option<&Value>) -> Option<f64> {
    v.and_then(number).filter(|v| *v > 0.0)
}
// Difficulty and hashes/second have different units; never guess a conversion.
pub fn apply_network_info(snap: &mut MarketAndPoolSnapshot, v: &Value) {
    snap.network_hashrate_ehs = positive(
        v.get("net_hashrate")
            .or_else(|| v.get("network_hashrate"))
            .or_else(|| v.get("networkHashrate")),
    )
    .map(|n| n / 1e18);
    snap.block_reward_prl = positive(v.get("block_reward"));
    snap.block_time_s = positive(v.get("block_time"));
}

pub fn apply_luckypool_balance(snap: &mut MarketAndPoolSnapshot, v: &Value) {
    if let Some(stats) = v.get("stats") {
        let atomic = |key: &str| stats.get(key).and_then(number).map(|n| n / 1e8);
        snap.pool_pending_prl = atomic("locked");
        // LuckyPool names matured, unpaid funds `unlocked`. `balance` is a
        // legacy fallback; absence must remain unknown rather than imply zero.
        snap.pool_confirmed_prl = atomic("unlocked").or_else(|| atomic("balance"));
        snap.pool_paid_prl = atomic("paid");
        snap.pool_threshold_prl = atomic("paymentThreshold");
    }
}

pub fn fetch_snapshot(cfg: &MinerConfig, address: Option<&str>) -> MarketAndPoolSnapshot {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(4))
        .timeout(Duration::from_secs(6))
        .user_agent("PearlMiner/0.1")
        .build();
    let get = |url: &str| -> Option<Value> { agent.get(url).call().ok()?.into_json().ok() };
    let mut snap = MarketAndPoolSnapshot {
        wallet_address: address.map(str::to_owned),
        ..Default::default()
    };
    if let Some(v) = get("https://api.gateio.ws/api/v4/spot/tickers?currency_pair=PEARL_USDT") {
        snap.prl_price_usd = positive(
            v.as_array()
                .and_then(|a| a.first())
                .and_then(|v| v.get("last")),
        );
    }
    if let Some(v) = get("https://pool.kryptex.com/prl/api/v1/pool/info") {
        apply_network_info(&mut snap, &v);
        if matches!(cfg.pool, PoolKind::KryptexEu | PoolKind::KryptexUs) {
            snap.pool_fee_fraction = v.get("fee").and_then(number).filter(|v| *v < 1.0);
            snap.pool_threshold_prl = positive(v.get("minpay"));
        }
    }
    if let Some(addr) = address {
        match cfg.pool {
            PoolKind::Luckypool => {
                if let Some(v) = get(&format!(
                    "https://pearl.luckypool.io/api/stats_address?address={addr}"
                )) {
                    apply_luckypool_balance(&mut snap, &v);
                }
            }
            PoolKind::KryptexEu | PoolKind::KryptexUs => {
                if let Some(v) = get(&format!(
                    "https://pool.kryptex.com/prl/api/v1/miner/balance/{addr}"
                )) {
                    snap.pool_pending_prl = v.get("unconfirmed").and_then(number);
                    snap.pool_confirmed_prl = v.get("confirmed").and_then(number);
                    snap.pool_paid_prl = v.get("paid").and_then(number);
                }
            }
            PoolKind::Custom => {}
        }
    }
    snap.fetched_at_unix = now_unix();
    snap.last_updated_utc = Some(crate::wallet::utc_now_iso());
    snap
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn luckypool_matured_funds_are_not_lost_when_balance_field_is_absent() {
        let mut s = MarketAndPoolSnapshot::default();
        apply_luckypool_balance(
            &mut s,
            &json!({"stats": {
                "locked": 25000000, "unlocked": "75000000", "paid": 0,
                "paymentThreshold": 500000000
            }}),
        );
        assert_eq!(s.pool_pending_prl, Some(0.025000000));
        assert_eq!(s.pool_confirmed_prl, Some(0.075000000));
        assert_eq!(s.pool_paid_prl, Some(0.0));
        assert_eq!(s.pool_threshold_prl, Some(5.0));
        let total = s.pool_pending_prl.unwrap() + s.pool_confirmed_prl.unwrap();
        assert!((total - 1.0).abs() < 1e-12);
    }

    #[test]
    fn luckypool_legacy_balance_and_zero_unlocked_are_handled_without_guessing() {
        let mut s = MarketAndPoolSnapshot::default();
        apply_luckypool_balance(&mut s, &json!({"stats": {"balance": "100000000"}}));
        assert_eq!(s.pool_confirmed_prl, Some(1.0));
        assert!(s.pool_pending_prl.is_none());
        apply_luckypool_balance(
            &mut s,
            &json!({"stats": {"unlocked": 0, "balance": "100000000"}}),
        );
        assert_eq!(s.pool_confirmed_prl, Some(0.0));
        apply_luckypool_balance(
            &mut s,
            &json!({"stats": {"unlocked": "NaN", "locked": -1, "paid": null}}),
        );
        assert!(s.pool_confirmed_prl.is_none());
        assert!(s.pool_pending_prl.is_none());
        assert!(s.pool_paid_prl.is_none());
    }
    #[test]
    fn real_kryptex_schema_uses_hashrate_not_difficulty() {
        let mut s = MarketAndPoolSnapshot::default();
        apply_network_info(
            &mut s,
            &json!({"net_hashrate": "90880010812145580000", "block_reward": 2172.14972345, "block_time": 194, "difficulty": 2097152}),
        );
        assert!((s.network_hashrate_ehs.unwrap() - 90.88001081214558).abs() < 1e-10);
        assert_eq!(s.block_reward_prl, Some(2172.14972345));
        apply_network_info(&mut s, &json!({"difficulty": 123456}));
        assert!(s.network_hashrate_ehs.is_none());
        for v in [json!("NaN"), json!("inf"), json!(-1), json!(null)] {
            assert!(number(&v).is_none());
        }
    }
}
