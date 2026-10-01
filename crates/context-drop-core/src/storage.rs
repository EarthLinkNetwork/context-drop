//! On-disk layout and atomic item storage.
//!
//! Layout (per OS user):
//! ```text
//! <data-dir>/
//! ├─ context-drop.db
//! ├─ config.json
//! ├─ packets/<packet-id>/manifest.json
//! │                      └─ items/0001.png, 0002.txt, ...
//! ├─ logs/
//! └─ bin/
//! ```
//!
//! The data directory is global per OS user and is shared by the desktop app
//! and the CLI. It is resolved identically by both so they always agree; the
//! `CONTEXT_DROP_DATA_DIR` environment variable overrides it (used by tests and
//! to let the desktop pin an explicit path).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::error::{CoreError, Result};
use crate::hash::sha256_hex;

/// Bundle identifier; also the leaf directory name under the OS data dir.
/// Chosen to match Tauri v2's `app_data_dir()` (`dirs::data_dir()/<identifier>`).
pub const APP_IDENTIFIER: &str = "com.contextdrop.app";

/// Environment variable that overrides the resolved data directory.
pub const DATA_DIR_ENV: &str = "CONTEXT_DROP_DATA_DIR";

/// Resolve the Context Drop data directory (does not create it).
pub fn data_dir() -> Result<PathBuf> {
    if let Some(explicit) = std::env::var_os(DATA_DIR_ENV) {
        if !explicit.is_empty() {
            return Ok(PathBuf::from(explicit));
        }
    }
    let base = dirs::data_dir().ok_or(CoreError::NoDataDir)?;
    Ok(base.join(APP_IDENTIFIER))
}

/// Filesystem paths for a resolved data directory.
#[derive(Debug, Clone)]
pub struct Storage {
    root: PathBuf,
}

impl Storage {
    /// Construct a `Storage` rooted at the standard data directory.
    pub fn resolve() -> Result<Storage> {
        Ok(Storage { root: data_dir()? })
    }

    /// Construct a `Storage` rooted at an explicit directory.
    pub fn at(root: impl Into<PathBuf>) -> Storage {
        Storage { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn db_path(&self) -> PathBuf {
        self.root.join("context-drop.db")
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    pub fn packets_dir(&self) -> PathBuf {
        self.root.join("packets")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn bin_dir(&self) -> PathBuf {
        self.root.join("bin")
    }

    pub fn packet_dir(&self, packet_id: &str) -> PathBuf {
        self.packets_dir().join(packet_id)
    }

    pub fn items_dir(&self, packet_id: &str) -> PathBuf {
        self.packet_dir(packet_id).join("items")
    }

    pub fn manifest_path(&self, packet_id: &str) -> PathBuf {
        self.packet_dir(packet_id).join("manifest.json")
    }

    /// Create the base directory tree with restrictive per-user permissions
    /// where the platform supports it (0700 on Unix).
    pub fn ensure_layout(&self) -> Result<()> {
        for dir in [
            self.root.clone(),
            self.packets_dir(),
            self.logs_dir(),
            self.bin_dir(),
        ] {
            create_dir_private(&dir)?;
        }
        Ok(())
    }

    /// Ensure a packet's `items/` directory exists.
    pub fn ensure_packet_dirs(&self, packet_id: &str) -> Result<()> {
        create_dir_private(&self.packet_dir(packet_id))?;
        create_dir_private(&self.items_dir(packet_id))?;
        Ok(())
    }

    /// Atomically store one item's bytes under `items/<seq:04>.<ext>`.
    ///
    /// Writes to a temp file in the same directory, fsyncs, then renames into
    /// place, so the manifest never points at a partially written file. Returns
    /// `(relative_path, byte_size, sha256)`.
    pub fn write_item_atomic(
        &self,
        packet_id: &str,
        seq: i64,
        ext: &str,
        bytes: &[u8],
    ) -> Result<StoredItem> {
        self.ensure_packet_dirs(packet_id)?;
        let items_dir = self.items_dir(packet_id);
        let ext = sanitize_ext(ext);
        let file_name = format!("{seq:04}.{ext}");
        let final_path = items_dir.join(&file_name);
        // Unique temp name so concurrent writers never fight over the same path.
        let tmp_path = items_dir.join(format!(".{file_name}.tmp-{}", Uuid::now_v7().simple()));

        {
            let mut f = fs::File::create(&tmp_path)?;
            f.write_all(bytes)?;
            f.flush()?;
            f.sync_all()?; // fsync before rename
        }
        set_file_private(&tmp_path)?;
        fs::rename(&tmp_path, &final_path)?;

        Ok(StoredItem {
            relative_path: format!("items/{file_name}"),
            byte_size: bytes.len() as i64,
            sha256: sha256_hex(bytes),
        })
    }

    /// Atomically write the manifest for a packet.
    pub fn write_manifest_atomic(&self, packet_id: &str, json: &str) -> Result<()> {
        self.ensure_packet_dirs(packet_id)?;
        let final_path = self.manifest_path(packet_id);
        // Unique temp name: append and claim can both refresh a packet's
        // manifest concurrently; last atomic rename wins, none clobbers another.
        let tmp_path = self
            .packet_dir(packet_id)
            .join(format!(".manifest.json.tmp-{}", Uuid::now_v7().simple()));
        {
            let mut f = fs::File::create(&tmp_path)?;
            f.write_all(json.as_bytes())?;
            f.flush()?;
            f.sync_all()?;
        }
        set_file_private(&tmp_path)?;
        fs::rename(&tmp_path, &final_path)?;
        Ok(())
    }

    /// Remove a packet's entire directory (used by cleanup/clear).
    pub fn remove_packet_dir(&self, packet_id: &str) -> Result<()> {
        let dir = self.packet_dir(packet_id);
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        Ok(())
    }
}

/// Result of storing one item to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredItem {
    pub relative_path: String,
    pub byte_size: i64,
    pub sha256: String,
}

/// A conservative extension sanitizer: keep short alphanumerics, else `bin`.
fn sanitize_ext(ext: &str) -> String {
    let cleaned: String = ext
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect::<String>()
        .to_ascii_lowercase();
    if cleaned.is_empty() {
        "bin".to_string()
    } else {
        cleaned
    }
}

#[cfg(unix)]
fn create_dir_private(path: &Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if path.exists() {
        // Secure an existing directory too — it may have been created 0755 (e.g.
        // recursively under umask 022 before we ran), which would let other
        // local users traverse into it and read routing metadata.
        let md = fs::metadata(path)?;
        if md.is_dir() && md.permissions().mode() & 0o777 != 0o700 {
            let mut perms = md.permissions();
            perms.set_mode(0o700);
            // Propagate: if we cannot make the data dir private, the caller must
            // know rather than silently write metadata into a traversable dir.
            fs::set_permissions(path, perms)?;
        }
        return Ok(());
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    Ok(())
}

#[cfg(not(unix))]
fn create_dir_private(path: &Path) -> Result<()> {
    // On Windows, per-user restriction comes from the profile directory ACLs
    // (the app data dir lives under %APPDATA%). See docs/security.md.
    fs::create_dir_all(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_file_private(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_file_private(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_respects_env_override() {
        // Note: env is process-global; this test sets and clears it locally.
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(DATA_DIR_ENV, dir.path());
        let resolved = data_dir().unwrap();
        assert_eq!(resolved, dir.path());
        std::env::remove_var(DATA_DIR_ENV);
    }

    #[test]
    fn atomic_write_produces_correct_bytes_and_hash() {
        let dir = tempfile::tempdir().unwrap();
        let s = Storage::at(dir.path());
        let stored = s.write_item_atomic("pkt", 1, "txt", b"hello").unwrap();
        assert_eq!(stored.relative_path, "items/0001.txt");
        assert_eq!(stored.byte_size, 5);
        assert_eq!(stored.sha256, sha256_hex(b"hello"));
        let path = s.items_dir("pkt").join("0001.txt");
        assert_eq!(fs::read(path).unwrap(), b"hello");
        // No temp files left behind.
        let leftover: Vec<_> = fs::read_dir(s.items_dir("pkt"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(leftover.is_empty());
    }

    #[test]
    fn extension_is_sanitized() {
        let dir = tempfile::tempdir().unwrap();
        let s = Storage::at(dir.path());
        // A normal extension is preserved.
        assert_eq!(
            s.write_item_atomic("pkt", 1, "png", b"x")
                .unwrap()
                .relative_path,
            "items/0001.png"
        );
        // A path-traversal attempt is neutralized: only alphanumerics survive,
        // so no separators or `..` can escape the items directory.
        let stored = s
            .write_item_atomic("pkt", 2, "../evil/../png", b"x")
            .unwrap();
        assert!(stored.relative_path.starts_with("items/0002."));
        assert_eq!(stored.relative_path.matches('/').count(), 1);
        assert!(!stored.relative_path.contains(".."));
        // An empty/garbage extension falls back to `bin`.
        assert_eq!(
            s.write_item_atomic("pkt", 3, "", b"x")
                .unwrap()
                .relative_path,
            "items/0003.bin"
        );
        assert_eq!(
            s.write_item_atomic("pkt", 4, "!!!", b"x")
                .unwrap()
                .relative_path,
            "items/0004.bin"
        );
    }

    #[cfg(unix)]
    #[test]
    fn directories_are_private_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let s = Storage::at(dir.path().join("root"));
        s.ensure_layout().unwrap();
        let mode = fs::metadata(s.root()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
}
