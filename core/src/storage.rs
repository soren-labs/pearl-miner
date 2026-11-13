//! Private, atomic local state writes. Never follow a predictable temporary-file symlink.
use anyhow::{bail, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub fn private_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    private_dir(parent)?;
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("Refusing to replace a symbolic link: {}", path.display());
    }
    let tmp = parent.join(format!(".pearl-{:016x}.tmp", rand::random::<u64>()));
    let result = (|| -> Result<()> {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path)?;
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub fn backup_existing_wallet(path: &Path) -> Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let dir = path
        .parent()
        .unwrap_or(Path::new("."))
        .join("wallet-backups");
    private_dir(&dir)?;
    let backup = dir.join(format!("wallet-{:016x}.json", rand::random::<u64>()));
    // Preserve the original bytes, even if an older wallet can no longer be parsed.
    let bytes = fs::read(path)?;
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&backup)?;
    f.write_all(&bytes)?;
    f.sync_all()?;
    fs::File::open(&dir)?.sync_all()?;
    Ok(Some(backup))
}
