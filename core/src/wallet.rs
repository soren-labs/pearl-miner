use anyhow::{bail, Context, Result};
use bech32::{segwit, Fe32, Hrp};
use k256::elliptic_curve::ops::Reduce;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::{AffinePoint, ProjectivePoint, Scalar, SecretKey, U256 as K256U256};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

pub const HRP: &str = "prl";
pub const WITNESS_VERSION: u8 = 1;
pub const PROGRAM_BYTES: usize = 32;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalletFile {
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internal_key_x_hex: Option<String>,
    pub created_utc: String,
    #[serde(default)]
    pub mode: WalletMode,
    #[serde(default)]
    pub note: String,
    #[serde(default = "legacy_backup_confirmed")]
    pub backup_confirmed: bool,
}

fn legacy_backup_confirmed() -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WalletMode {
    #[default]
    FullKeypair,
    WatchOnly,
}

#[derive(Clone, Debug, Serialize)]
pub struct WalletSummary {
    pub address: String,
    pub mode: WalletMode,
    pub has_private_key: bool,
    pub created_utc: String,
    pub path: String,
    pub needs_backup: bool,
}

fn tagged_hash(tag: &str, data: &[u8]) -> [u8; 32] {
    let tag_hash = Sha256::digest(tag.as_bytes());
    let mut hasher = Sha256::new();
    hasher.update(tag_hash);
    hasher.update(tag_hash);
    hasher.update(data);
    hasher.finalize().into()
}

pub fn encode_taproot_address(output_x: &[u8; 32]) -> Result<String> {
    let hrp = Hrp::parse(HRP).map_err(|e| anyhow::anyhow!("invalid hrp: {e}"))?;
    let addr = segwit::encode(hrp, Fe32::P, output_x)
        .map_err(|e| anyhow::anyhow!("bech32m encode failed: {e}"))?;
    Ok(addr)
}

pub fn validate_payout_address(addr: &str) -> Result<String> {
    let trimmed = addr.trim();
    if trimmed.is_empty() {
        bail!("address is empty");
    }
    if trimmed != trimmed.to_lowercase() && trimmed != trimmed.to_uppercase() {
        bail!("mixed upper/lower case — a valid address must be all one case");
    }
    let lower = trimmed.to_lowercase();
    if lower.len() > 90 {
        bail!("address is too long");
    }
    let (hrp, version, program) =
        segwit::decode(&lower).map_err(|e| anyhow::anyhow!("invalid bech32m address: {e}"))?;
    if hrp.as_str() != HRP {
        let hint = if hrp.as_str() == "bc" || hrp.as_str() == "tb" {
            " (that prefix belongs to Bitcoin)"
        } else {
            ""
        };
        bail!(
            "prefix '{}' is not a Pearl address{} — a Pearl address starts with '{}1'",
            hrp,
            hint,
            HRP
        );
    }
    if version != Fe32::P {
        bail!(
            "witness version {} is not Taproot (expected version 1, starting with '{}1p')",
            version.to_u8(),
            HRP
        );
    }
    if program.len() != PROGRAM_BYTES {
        bail!(
            "decoded witness program has invalid length {}",
            program.len()
        );
    }
    Ok(lower)
}

/// Derive the even-Y normalized private key, internal key x, and BIP-341 Taproot `prl1p...` address.
/// Matches `pearl_metal_miner/wallet.py::derive_address` bit-for-bit.
pub fn derive_address_from_key_bytes(raw_key: &[u8; 32]) -> Result<([u8; 32], [u8; 32], String)> {
    let sk = SecretKey::from_slice(raw_key)
        .map_err(|_| anyhow::anyhow!("private key out of secp256k1 range [1, N-1]"))?;
    let mut scalar = *sk.to_nonzero_scalar();
    let mut point = (ProjectivePoint::GENERATOR * scalar.as_ref()).to_affine();
    let encoded = point.to_encoded_point(false);
    let y_bytes = encoded.y().context("point at infinity")?;
    if y_bytes[31] & 1 == 1 {
        scalar = -scalar;
        point = (ProjectivePoint::GENERATOR * scalar.as_ref()).to_affine();
    }
    let encoded_even = point.to_encoded_point(false);
    let internal_x: [u8; 32] = (*encoded_even.x().context("missing x coordinate")?).into();

    let tweak_bytes = tagged_hash("TapTweak", &internal_x);
    let tweak_u256 = K256U256::from_be_slice(&tweak_bytes);
    let tweak_scalar = <Scalar as Reduce<K256U256>>::reduce(tweak_u256);
    if tweak_bytes != <[u8; 32]>::from(tweak_scalar.to_bytes()) {
        bail!("tweak out of range");
    }

    let tweaked_point: AffinePoint =
        (ProjectivePoint::from(point) + ProjectivePoint::GENERATOR * tweak_scalar).to_affine();
    let tweaked_encoded = tweaked_point.to_encoded_point(false);
    let output_x: [u8; 32] = (*tweaked_encoded.x().context("tweaked point missing x")?).into();

    let address = encode_taproot_address(&output_x)?;
    validate_payout_address(&address)?;

    let normalized_priv: [u8; 32] = scalar.to_bytes().into();
    Ok((normalized_priv, internal_x, address))
}

pub fn generate_wallet_data() -> Result<WalletFile> {
    let mut rng = rand::rng();
    loop {
        let mut key = [0u8; 32];
        rng.fill_bytes(&mut key);
        if let Ok((norm_priv, internal_x, address)) = derive_address_from_key_bytes(&key) {
            return Ok(WalletFile {
                address,
                private_key_hex: Some(hex::encode(norm_priv)),
                internal_key_x_hex: Some(hex::encode(internal_x)),
                created_utc: utc_now_iso(),
                mode: WalletMode::FullKeypair,
                backup_confirmed: false,
                note: "Payout wallet created by PearlMiner. Keep the private key secret and backed up. To spend, import the private key into a taproot-capable tr() descriptor wallet.".to_string(),
            });
        }
    }
}

pub fn wallet_from_private_key_hex(input: &str) -> Result<WalletFile> {
    let clean = input
        .trim()
        .strip_prefix("0x")
        .or_else(|| input.trim().strip_prefix("0X"))
        .unwrap_or(input.trim());
    let bytes = hex::decode(clean).context("private key must be 64 hex characters")?;
    if bytes.len() != 32 {
        bail!(
            "private key must be exactly 32 bytes (64 hex chars), got {} bytes",
            bytes.len()
        );
    }
    let arr: [u8; 32] = bytes.try_into().unwrap();
    let (norm_priv, internal_x, address) = derive_address_from_key_bytes(&arr)?;
    Ok(WalletFile {
        address,
        private_key_hex: Some(hex::encode(norm_priv)),
        internal_key_x_hex: Some(hex::encode(internal_x)),
        created_utc: utc_now_iso(),
        mode: WalletMode::FullKeypair,
        backup_confirmed: true,
        note: "Imported into PearlMiner from private key.".to_string(),
    })
}

pub fn wallet_from_watch_address(addr: &str) -> Result<WalletFile> {
    let normalized = validate_payout_address(addr)?;
    Ok(WalletFile {
        address: normalized,
        private_key_hex: None,
        internal_key_x_hex: None,
        created_utc: utc_now_iso(),
        mode: WalletMode::WatchOnly,
        backup_confirmed: true,
        note: "External payout address connected to PearlMiner (watch/payout only; private key is managed elsewhere).".to_string(),
    })
}

pub fn write_wallet_file(path: &Path, wallet: &WalletFile) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        crate::storage::private_dir(parent)?;
    }
    // Serialize replacement/backup across the desktop daemon and headless CLI.
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path.with_extension("json.lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let json = serde_json::to_string_pretty(wallet)? + "\n";
    crate::storage::backup_existing_wallet(path)
        .context("Cannot back up existing wallet; replacement cancelled")?;
    crate::storage::atomic_write(path, json.as_bytes())
}

pub fn load_and_verify_wallet(path: &Path) -> Result<WalletFile> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("cannot read wallet file {}", path.display()))?;
    let mut data: WalletFile = serde_json::from_str(&raw).context("invalid wallet.json format")?;
    data.address = validate_payout_address(&data.address)?;
    if let Some(ref pk_hex) = data.private_key_hex {
        let derived = wallet_from_private_key_hex(pk_hex)?;
        if derived.address != data.address {
            bail!(
                "wallet file integrity error: stored address {} does not match private key address {}",
                data.address,
                derived.address
            );
        }
        data.mode = WalletMode::FullKeypair;
    } else {
        data.mode = WalletMode::WatchOnly;
    }
    Ok(data)
}

pub fn utc_now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Format YYYY-MM-DDTHH:MM:SSZ without extra chrono dependency
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let hour = rem / 3600;
    let min = (rem % 3600) / 60;
    let sec = rem % 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taproot_derivation_matches_bip341_and_python_reference() {
        // Known private key 1 -> G, verified against BIP-341 / pearl_metal_miner/wallet.py
        let mut priv1 = [0u8; 32];
        priv1[31] = 1;
        let (norm, internal_x, addr) = derive_address_from_key_bytes(&priv1).unwrap();
        assert_eq!(
            hex::encode(internal_x),
            "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
        );
        assert_eq!(norm, priv1);
        assert_eq!(
            addr,
            "prl1pmfr3p9j00pfxjh0zmgp99y8zftmd3s5pmedqhyptwy6lm87hf5sse9xfq3"
        );
        assert_eq!(validate_payout_address(&addr).unwrap(), addr);

        let w_deadbeef = wallet_from_private_key_hex(
            "00000000000000000000000000000000000000000000000000000000deadbeef",
        )
        .unwrap();
        assert_eq!(
            w_deadbeef.address,
            "prl1pyqrlta4lgwsv3wmwqzzpn9ppmwd5vzuwmxyx7t2vnm4hvn9rsn8qxunm7k"
        );

        // Reject bitcoin prefix and invalid checksum
        assert!(validate_payout_address(
            "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0"
        )
        .is_err());
        let mut corrupted = addr.clone();
        let last = corrupted.pop().unwrap();
        corrupted.push(if last == 'q' { 'p' } else { 'q' });
        assert!(validate_payout_address(&corrupted).is_err());
    }

    #[test]
    fn key_generation_roundtrips_and_validates() {
        let w = generate_wallet_data().unwrap();
        let imported = wallet_from_private_key_hex(w.private_key_hex.as_ref().unwrap()).unwrap();
        assert_eq!(w.address, imported.address);
    }
}
