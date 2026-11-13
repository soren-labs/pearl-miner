pub fn gpu_watts_fallback(device_name: &str, intensity: u8) -> f64 {
    let table: &[(&str, f64)] = &[
        ("M6 Ultra", 80.0),
        ("M6 Max", 50.0),
        ("M6 Pro", 32.0),
        ("M6", 27.0),
        ("M5 Ultra", 80.0),
        ("M5 Max", 45.0),
        ("M5 Pro", 22.0),
        ("M5", 14.0),
        ("M4 Ultra", 80.0),
        ("M4 Max", 45.0),
        ("M4 Pro", 22.0),
        ("M4", 14.0),
        ("M3 Ultra", 80.0),
        ("M3 Max", 45.0),
        ("M3 Pro", 20.0),
        ("M3", 12.0),
        ("M2 Ultra", 75.0),
        ("M2 Max", 40.0),
        ("M2 Pro", 20.0),
        ("M2", 12.0),
        ("M1 Ultra", 60.0),
        ("M1 Max", 40.0),
        ("M1 Pro", 17.0),
        ("M1", 10.0),
    ];
    let full = table
        .iter()
        .find(|(chip, _)| device_name.contains(chip))
        .map(|(_, w)| *w)
        .unwrap_or(20.0);
    full * (intensity.min(100) as f64) / 100.0
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct EconomicsSnapshot {
    pub earnings_available: bool,
    pub usd_available: bool,
    pub fee_included: bool,
    pub prl_per_day: f64,
    pub prl_per_month: f64,
    pub gross_usd_per_day: f64,
    pub gross_usd_per_month: f64,
    pub power_cost_usd_per_day: f64,
    pub net_usd_per_day: f64,
    pub kwh_per_day: f64,
    pub prl_price_usd: f64,
    pub network_hashrate_ehs: f64,
    pub is_live_market: bool,
}

pub fn compute_economics(
    tiles_per_s: f64,
    factor: u64,
    system_watts: f64,
    electricity_usd_per_kwh: f64,
    cfg: &crate::config::MinerConfig,
    market: &crate::pool_stats::MarketAndPoolSnapshot,
) -> EconomicsSnapshot {
    let fresh = market.is_fresh();
    let (price, network, reward, interval) = if cfg.live_market_sync && fresh {
        (
            market.prl_price_usd,
            market.network_hashrate_ehs,
            market.block_reward_prl,
            market.block_time_s,
        )
    } else {
        (None, None, None, None)
    };
    let earnings_available = network.is_some() && reward.is_some() && interval.is_some();
    let usd_available = earnings_available && price.is_some();
    let network_hashrate_ehs = network.unwrap_or(0.0);
    let prl_price_usd = price.unwrap_or(0.0);
    let fee = if fresh {
        market.pool_fee_fraction.unwrap_or(0.0)
    } else {
        0.0
    };
    let prl_day = if earnings_available {
        tiles_per_s.max(0.0) * factor as f64 / (network_hashrate_ehs * 1e18)
            * (86400.0 / interval.unwrap())
            * reward.unwrap()
            * (1.0 - fee)
    } else {
        0.0
    };
    let prl_month = prl_day * 30.0;
    let gross_day = prl_day * prl_price_usd;
    let gross_month = prl_month * prl_price_usd;
    let kwh_day = (system_watts.max(0.0) / 1000.0) * 24.0;
    let power_cost_day = kwh_day * electricity_usd_per_kwh.max(0.0);
    let net_day = gross_day - power_cost_day;
    EconomicsSnapshot {
        earnings_available,
        usd_available,
        fee_included: fresh && market.pool_fee_fraction.is_some(),
        prl_per_day: prl_day,
        prl_per_month: prl_month,
        gross_usd_per_day: gross_day,
        gross_usd_per_month: gross_month,
        power_cost_usd_per_day: power_cost_day,
        net_usd_per_day: net_day,
        kwh_per_day: kwh_day,
        prl_price_usd,
        network_hashrate_ehs,
        is_live_market: cfg.live_market_sync && usd_available,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_and_stale_live_data_never_use_manual_fallbacks() {
        let cfg = crate::config::MinerConfig::default();
        let mut m = crate::pool_stats::MarketAndPoolSnapshot::default();
        assert!(!compute_economics(1e7, 262144, 60.0, 0.15, &cfg, &m).earnings_available);
        m.network_hashrate_ehs = Some(90.0);
        m.prl_price_usd = Some(1.5);
        m.block_reward_prl = Some(2172.0);
        m.block_time_s = Some(120.0);
        assert!(!compute_economics(1e7, 262144, 60.0, 0.15, &cfg, &m).earnings_available);
        m.fetched_at_unix = crate::pool_stats::now_unix();
        let e = compute_economics(1e7, 262144, 60.0, 0.15, &cfg, &m);
        assert!(e.is_live_market && e.prl_per_day > 0.0 && e.prl_per_day < 1.0);
        assert!((e.power_cost_usd_per_day - 0.216).abs() < 1e-10);
        m.prl_price_usd = None;
        let e = compute_economics(1e7, 262144, 60.0, 0.15, &cfg, &m);
        assert!(e.earnings_available && !e.usd_available && !e.is_live_market);
    }
}
