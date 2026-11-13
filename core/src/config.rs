use anyhow::{ensure, Context, Result};
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
        }
    }
}

impl MinerConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            matches!(self.language.as_str(), "en" | "zh-Hans"),
            "Unsupported language"
        );
        ensure!(
            (25..=100).contains(&self.intensity),
            "GPU intensity must be 25–100%"
        );
        ensure!(
            (1..=32).contains(&self.cpu_threads),
            "CPU threads must be 1–32"
        );
        ensure!(
            !self.worker.is_empty()
                && self.worker.len() <= 64
                && self
                    .worker
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)),
            "Worker must contain 1–64 letters, numbers, dots, hyphens or underscores"
        );
        ensure!(
            self.electricity_usd_per_kwh.is_finite() && self.electricity_usd_per_kwh >= 0.0,
            "Electricity rate must be a finite, nonnegative number"
        );
        if self.pool == PoolKind::Custom {
            ensure!(
                !self.custom_host.is_empty()
                    && self.custom_host.trim() == self.custom_host
                    && !self
                        .custom_host
                        .chars()
                        .any(|c| c.is_whitespace() || "/\\".contains(c)),
                "Enter a hostname or IP address without a URL scheme"
            );
            ensure!(self.custom_port > 0, "Pool port must be 1–65535");
        }
        Ok(())
    }

    pub fn same_connection(&self, other: &Self) -> bool {
        self.pool == other.pool
            && self.effective_endpoint() == other.effective_endpoint()
            && self.worker == other.worker
    }
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
            let safe: String = short
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                        c
                    } else {
                        '-'
                    }
                })
                .take(64)
                .collect();
            return safe;
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

pub fn load_config(path: &Path) -> Result<MinerConfig> {
    match fs::read_to_string(path) {
        Ok(raw) => {
            let cfg: MinerConfig = serde_json::from_str(&raw)
                .with_context(|| format!("Invalid configuration: {}", path.display()))?;
            cfg.validate()?;
            Ok(cfg)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(MinerConfig::default()),
        Err(e) => Err(e.into()),
    }
}

pub fn save_config(path: &Path, cfg: &MinerConfig) -> Result<()> {
    cfg.validate()?;
    let json = serde_json::to_string_pretty(cfg)? + "\n";
    crate::storage::atomic_write(path, json.as_bytes())
}
