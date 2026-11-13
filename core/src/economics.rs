pub const BLOCK_TIME_S: f64 = 120.0;
pub const BLOCK_REWARD_PRL: f64 = 2460.0;

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

pub fn prl_per_day(tiles_per_s: f64, factor: u64, network_hashrate_ehs: f64) -> f64 {
    if network_hashrate_ehs <= 0.0 || tiles_per_s <= 0.0 {
        return 0.0;
    }
    let my_hashes_per_s = tiles_per_s * (factor as f64);
    let net_hashes_per_s = network_hashrate_ehs * 1e18;
    let blocks_per_day = 86400.0 / BLOCK_TIME_S;
    (my_hashes_per_s / net_hashes_per_s) * blocks_per_day * BLOCK_REWARD_PRL
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct EconomicsSnapshot {
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
    prl_price_usd: f64,
    network_hashrate_ehs: f64,
    is_live_market: bool,
) -> EconomicsSnapshot {
    let prl_day = prl_per_day(tiles_per_s, factor, network_hashrate_ehs);
    let prl_month = prl_day * 30.0;
    let gross_day = prl_day * prl_price_usd;
    let gross_month = prl_month * prl_price_usd;
    let kwh_day = (system_watts.max(0.0) / 1000.0) * 24.0;
    let power_cost_day = kwh_day * electricity_usd_per_kwh.max(0.0);
    let net_day = gross_day - power_cost_day;
    EconomicsSnapshot {
        prl_per_day: prl_day,
        prl_per_month: prl_month,
        gross_usd_per_day: gross_day,
        gross_usd_per_month: gross_month,
        power_cost_usd_per_day: power_cost_day,
        net_usd_per_day: net_day,
        kwh_per_day: kwh_day,
        prl_price_usd,
        network_hashrate_ehs,
        is_live_market,
    }
}
