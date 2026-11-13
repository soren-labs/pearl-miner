use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PoolKind {
    #[default]
    Luckypool,
    KryptexEu,
    KryptexUs,
    Custom,
}

impl PoolKind {
    pub fn default_endpoint(self) -> (&'static str, u16) {
        match self {
            PoolKind::Luckypool => ("pearl-eu1.luckypool.io", 3360),
            PoolKind::KryptexEu => ("prl-eu.kryptex.network", 7048),
            PoolKind::KryptexUs => ("prl-us.kryptex.network", 7048),
            PoolKind::Custom => ("pearl-eu1.luckypool.io", 3360),
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            PoolKind::Luckypool => "LuckyPool",
            PoolKind::KryptexEu => "Kryptex (EU)",
            PoolKind::KryptexUs => "Kryptex (US)",
            PoolKind::Custom => "Custom",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BatteryPolicy {
    #[default]
    Pause,
    Low,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DialectKind {
    #[default]
    Luckypool,
    Kryptex,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MinerConfig {
    pub language: String,
    pub pool: PoolKind,
    pub custom_dialect: DialectKind,
    pub custom_host: String,
    pub custom_port: u16,
    pub worker: String,
    pub intensity: u8,
    pub auto_intensity: bool,
    pub on_battery: BatteryPolicy,
    pub keep_awake: bool,
    pub cpu_threads: usize,
    pub electricity_usd_per_kwh: f64,
    pub live_market_sync: bool,
    pub fallback_prl_price_usd: f64,
    pub fallback_network_hashrate_ehs: f64,
}

impl Default for MinerConfig {
    fn default() -> Self {
        let host = hostname_short();
        Self {
            language: "en".to_string(),
            pool: PoolKind::Luckypool,
            custom_dialect: DialectKind::Luckypool,
            custom_host: "pearl-eu1.luckypool.io".to_string(),
            custom_port: 3360,
            worker: host,
            intensity: 100,
            auto_intensity: false,
            on_battery: BatteryPolicy::Pause,
            keep_awake: true,
            cpu_threads: 4,
            electricity_usd_per_kwh: 0.15,
            live_market_sync: true,
            fallback_prl_price_usd: 1.40,
            fallback_network_hashrate_ehs: 35.0,
        }
    }
}

impl MinerConfig {
    pub fn effective_endpoint(&self) -> (DialectKind, String, u16) {
        match self.pool {
            PoolKind::Luckypool => (
                DialectKind::Luckypool,
                "pearl-eu1.luckypool.io".to_string(),
                3360,
            ),
            PoolKind::KryptexEu => (
                DialectKind::Kryptex,
                "prl-eu.kryptex.network".to_string(),
                7048,
            ),
            PoolKind::KryptexUs => (
                DialectKind::Kryptex,
                "prl-us.kryptex.network".to_string(),
                7048,
            ),
            PoolKind::Custom => (
                self.custom_dialect,
                self.custom_host.clone(),
                self.custom_port,
            ),
        }
    }
}

pub fn hostname_short() -> String {
    let mut buf = [0u8; 128];
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
    if rc == 0 {
        let s = std::ffi::CStr::from_bytes_until_nul(&buf)
            .ok()
            .and_then(|c| c.to_str().ok())
            .unwrap_or("mac");
        let short = s.split('.').next().unwrap_or("mac").trim();
        if !short.is_empty() {
            return short.to_string();
        }
    }
    "mac".to_string()
}

pub fn app_support_dir() -> PathBuf {
    if let Ok(override_dir) = std::env::var("PEARL_MINER_HOME") {
        return PathBuf::from(override_dir);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("PearlMiner")
}

pub fn default_config_path() -> PathBuf {
    app_support_dir().join("config.json")
}

pub fn default_wallet_path() -> PathBuf {
    app_support_dir().join("wallet.json")
}

pub fn load_config(path: &Path) -> MinerConfig {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => MinerConfig::default(),
    }
}

pub fn save_config(path: &Path, cfg: &MinerConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(cfg)? + "\n";
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json)?;
    fs::rename(&tmp, path)?;
    Ok(())
}
