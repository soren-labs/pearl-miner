use pearl_miner_core::{
    config::{load_config, save_config, MinerConfig},
    storage::atomic_write,
    wallet::{generate_wallet_data, load_and_verify_wallet, write_wallet_file},
};
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::PathBuf,
};
struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("pearl-regression-{:x}", rand::random::<u64>()));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn replacing_wallet_preserves_the_original_key_and_private_permissions() {
    let d = TestDir::new();
    let p = d.0.join("wallet.json");
    let old = generate_wallet_data().unwrap();
    let new = generate_wallet_data().unwrap();
    write_wallet_file(&p, &old).unwrap();
    write_wallet_file(&p, &new).unwrap();
    assert_eq!(load_and_verify_wallet(&p).unwrap().address, new.address);
    let backups: Vec<_> = fs::read_dir(d.0.join("wallet-backups"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(backups.len(), 1);
    let saved = load_and_verify_wallet(&backups[0]).unwrap();
    assert_eq!(saved.private_key_hex, old.private_key_hex);
    for f in [p, backups[0].clone()] {
        assert_eq!(fs::metadata(f).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
#[test]
fn a_failed_backup_never_replaces_the_existing_wallet() {
    let d = TestDir::new();
    let p = d.0.join("wallet.json");
    let old = generate_wallet_data().unwrap();
    write_wallet_file(&p, &old).unwrap();
    fs::write(d.0.join("wallet-backups"), "blocking file").unwrap();
    assert!(write_wallet_file(&p, &generate_wallet_data().unwrap()).is_err());
    assert_eq!(load_and_verify_wallet(&p).unwrap().address, old.address);
}
#[test]
fn malformed_config_is_reported_and_symlinks_are_not_overwritten() {
    let d = TestDir::new();
    let p = d.0.join("config.json");
    assert!(load_config(&p).is_ok());
    fs::write(&p, "broken json").unwrap();
    assert!(load_config(&p).is_err());
    let mut cfg = MinerConfig {
        intensity: 0,
        ..Default::default()
    };
    assert!(save_config(&p, &cfg).is_err());
    cfg.intensity = 25;
    cfg.electricity_usd_per_kwh = f64::NAN;
    assert!(cfg.validate().is_err());
    let target = d.0.join("target");
    fs::write(&target, "original").unwrap();
    let link = d.0.join("link");
    symlink(&target, &link).unwrap();
    assert!(atomic_write(&link, b"changed").is_err());
    assert_eq!(fs::read(&target).unwrap(), b"original");
}

#[test]
fn backup_acknowledgement_survives_restart_and_legacy_imports_remain_compatible() {
    let d = TestDir::new();
    let p = d.0.join("wallet.json");
    let mut w = generate_wallet_data().unwrap();
    assert!(!w.backup_confirmed);
    write_wallet_file(&p, &w).unwrap();
    assert!(!load_and_verify_wallet(&p).unwrap().backup_confirmed);
    w.backup_confirmed = true;
    write_wallet_file(&p, &w).unwrap();
    assert!(load_and_verify_wallet(&p).unwrap().backup_confirmed);
    let mut legacy = serde_json::to_value(&w).unwrap();
    legacy.as_object_mut().unwrap().remove("backup_confirmed");
    fs::write(&p, serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert!(load_and_verify_wallet(&p).unwrap().backup_confirmed);
}

#[test]
fn concurrent_wallet_writers_preserve_every_previous_wallet() {
    let d = TestDir::new();
    let p = d.0.join("wallet.json");
    let original = generate_wallet_data().unwrap();
    write_wallet_file(&p, &original).unwrap();
    let a = generate_wallet_data().unwrap();
    let b = generate_wallet_data().unwrap();
    let expected = [original.address, a.address.clone(), b.address.clone()];
    let p1 = p.clone();
    let p2 = p.clone();
    let t1 = std::thread::spawn(move || write_wallet_file(&p1, &a).unwrap());
    let t2 = std::thread::spawn(move || write_wallet_file(&p2, &b).unwrap());
    t1.join().unwrap();
    t2.join().unwrap();
    let mut actual = vec![load_and_verify_wallet(&p).unwrap().address];
    for f in fs::read_dir(d.0.join("wallet-backups")).unwrap() {
        actual.push(load_and_verify_wallet(&f.unwrap().path()).unwrap().address);
    }
    assert_eq!(actual.len(), 3);
    for addr in expected {
        assert!(actual.contains(&addr));
    }
}
