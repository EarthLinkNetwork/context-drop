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

/// Bundle identifier of the desktop app (`tauri.conf.json`). No longer used as
/// the data directory name — see [`DATA_DIR_LEAF`].
pub const APP_IDENTIFIER: &str = "com.contextdrop.app";

/// Leaf directory name under the OS data dir.
///
/// Deliberately *not* the bundle identifier: on macOS a directory whose name
/// ends in `.app` is an application bundle to LaunchServices. The data dir's
/// top level changes every time SQLite opens the database (`-wal`/`-shm`
/// files come and go), so Finder and every open/save panel service kept asking
/// `lsd` to re-register an "app" with no executable. That pinned `lsd` at
/// 100% CPU and made copy/paste slow system-wide (v0.1.10 and earlier).
pub const DATA_DIR_LEAF: &str = "com.contextdrop.desktop";

/// Data directory leaf used up to v0.1.10 (equal to the bundle identifier).
/// The desktop app moves it to [`DATA_DIR_LEAF`] on startup
/// ([`migrate_legacy_data_dir`]).
pub const LEGACY_DATA_DIR_LEAF: &str = "com.contextdrop.app";

/// Environment variable that overrides the resolved data directory.
pub const DATA_DIR_ENV: &str = "CONTEXT_DROP_DATA_DIR";

/// Resolve the Context Drop data directory (does not create or move it).
///
/// `CONTEXT_DROP_DATA_DIR` wins. Otherwise the current location
/// (`<os data dir>/com.contextdrop.desktop`) is used, except while only the
/// pre-0.1.11 directory exists (the desktop app has not migrated it yet): then
/// that one, so the CLI keeps seeing the user's packets.
pub fn data_dir() -> Result<PathBuf> {
    if let Some(explicit) = env_override() {
        return Ok(explicit);
    }
    let base = dirs::data_dir().ok_or(CoreError::NoDataDir)?;
    Ok(select_data_dir_in(&base))
}

fn env_override() -> Option<PathBuf> {
    match std::env::var_os(DATA_DIR_ENV) {
        Some(v) if !v.is_empty() => Some(PathBuf::from(v)),
        _ => None,
    }
}

/// Pick the data dir under `base` without changing the filesystem.
pub fn select_data_dir_in(base: &Path) -> PathBuf {
    let current = base.join(DATA_DIR_LEAF);
    let legacy = base.join(LEGACY_DATA_DIR_LEAF);
    if !exists_no_follow(&current) && is_real_dir(&legacy) {
        return legacy;
    }
    current
}

/// Result of [`migrate_legacy_data_dir`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataDirMigration {
    /// `CONTEXT_DROP_DATA_DIR` is set; nothing was looked at.
    Overridden,
    /// No pre-0.1.11 directory to move (fresh install or already migrated).
    NotNeeded,
    /// The legacy directory was renamed to the current location.
    Moved { from: PathBuf, to: PathBuf },
    /// Both directories exist; the current one is used and the legacy one is
    /// left untouched (never merged or deleted automatically).
    BothPresent { legacy: PathBuf },
    /// The rename failed (e.g. a file is held open on Windows); the legacy
    /// directory stays in use and the move is retried on the next start.
    Failed { legacy: PathBuf, error: String },
}

/// Move the pre-0.1.11 data directory (`com.contextdrop.app`) to
/// [`DATA_DIR_LEAF`], and repoint the `~/.local/bin/context-drop` convenience
/// symlink if it pointed into the old directory.
///
/// Called by the desktop app once at startup, before anything opens the
/// database. The CLI never moves the directory (an older desktop app may still
/// be running against it); it only follows [`data_dir`]'s selection.
/// Idempotent and best-effort: never deletes data.
pub fn migrate_legacy_data_dir() -> DataDirMigration {
    if env_override().is_some() {
        return DataDirMigration::Overridden;
    }
    let Some(base) = dirs::data_dir() else {
        return DataDirMigration::NotNeeded;
    };
    let outcome = migrate_legacy_data_dir_in(&base);
    // Runs on every start, so it also repairs a link left behind by a start
    // that renamed the directory but stopped before fixing the link.
    if !matches!(outcome, DataDirMigration::Failed { .. }) {
        crate::integration::retarget_local_bin_link(
            &base.join(LEGACY_DATA_DIR_LEAF),
            &base.join(DATA_DIR_LEAF),
        );
    }
    outcome
}

/// [`migrate_legacy_data_dir`] against an explicit OS data dir `base`
/// (no symlink handling).
pub fn migrate_legacy_data_dir_in(base: &Path) -> DataDirMigration {
    let current = base.join(DATA_DIR_LEAF);
    let legacy = base.join(LEGACY_DATA_DIR_LEAF);
    if !is_real_dir(&legacy) {
        return DataDirMigration::NotNeeded;
    }
    if exists_no_follow(&current) {
        return DataDirMigration::BothPresent { legacy };
    }
    match fs::rename(&legacy, &current) {
        Ok(()) => DataDirMigration::Moved {
            from: legacy,
            to: current,
        },
        // Another process won the race: the data is where it should be.
        Err(_) if exists_no_follow(&current) && !exists_no_follow(&legacy) => {
            DataDirMigration::Moved {
                from: legacy,
                to: current,
            }
        }
        Err(e) => DataDirMigration::Failed {
            legacy,
            error: e.to_string(),
        },
    }
}

fn exists_no_follow(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok()
}

fn is_real_dir(p: &Path) -> bool {
    fs::symlink_metadata(p)
        .map(|m| m.file_type().is_dir())
        .unwrap_or(false)
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
    fn data_dir_leaf_is_not_an_app_bundle_name() {
        // macOS treats any directory ending in `.app` as an application bundle;
        // the data dir must never be one (v0.1.10 lsd/copy-paste regression).
        assert!(!DATA_DIR_LEAF.ends_with(".app"));
        assert_ne!(DATA_DIR_LEAF, LEGACY_DATA_DIR_LEAF);
    }

    #[test]
    fn fresh_install_selects_current_and_migration_is_not_needed() {
        let base = tempfile::tempdir().unwrap();
        assert_eq!(
            select_data_dir_in(base.path()),
            base.path().join(DATA_DIR_LEAF)
        );
        assert_eq!(
            migrate_legacy_data_dir_in(base.path()),
            DataDirMigration::NotNeeded
        );
        assert!(!base.path().join(DATA_DIR_LEAF).exists(), "must not create");
    }

    #[test]
    fn legacy_only_is_selected_until_the_desktop_migrates_it() {
        let base = tempfile::tempdir().unwrap();
        let legacy = base.path().join(LEGACY_DATA_DIR_LEAF);
        fs::create_dir_all(legacy.join("packets/p1")).unwrap();
        fs::write(legacy.join("context-drop.db"), b"db").unwrap();
        // The CLI's view: keep using the old directory, do not move it.
        assert_eq!(select_data_dir_in(base.path()), legacy);
        assert!(legacy.is_dir());

        let current = base.path().join(DATA_DIR_LEAF);
        assert_eq!(
            migrate_legacy_data_dir_in(base.path()),
            DataDirMigration::Moved {
                from: legacy.clone(),
                to: current.clone()
            }
        );
        assert!(!legacy.exists(), "legacy dir must be gone after the move");
        assert_eq!(fs::read(current.join("context-drop.db")).unwrap(), b"db");
        assert!(current.join("packets/p1").is_dir());
        assert_eq!(select_data_dir_in(base.path()), current);
        // Idempotent.
        assert_eq!(
            migrate_legacy_data_dir_in(base.path()),
            DataDirMigration::NotNeeded
        );
    }

    #[test]
    fn both_present_keeps_current_and_never_touches_legacy() {
        let base = tempfile::tempdir().unwrap();
        let legacy = base.path().join(LEGACY_DATA_DIR_LEAF);
        let current = base.path().join(DATA_DIR_LEAF);
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("context-drop.db"), b"old").unwrap();
        fs::create_dir_all(&current).unwrap();
        fs::write(current.join("context-drop.db"), b"new").unwrap();

        assert_eq!(select_data_dir_in(base.path()), current);
        assert_eq!(
            migrate_legacy_data_dir_in(base.path()),
            DataDirMigration::BothPresent {
                legacy: legacy.clone()
            }
        );
        assert_eq!(fs::read(legacy.join("context-drop.db")).unwrap(), b"old");
        assert_eq!(fs::read(current.join("context-drop.db")).unwrap(), b"new");
    }

    #[cfg(unix)]
    #[test]
    fn legacy_that_is_a_symlink_or_file_is_not_moved() {
        let base = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let legacy = base.path().join(LEGACY_DATA_DIR_LEAF);
        std::os::unix::fs::symlink(elsewhere.path(), &legacy).unwrap();
        assert_eq!(
            migrate_legacy_data_dir_in(base.path()),
            DataDirMigration::NotNeeded
        );
        assert_eq!(
            select_data_dir_in(base.path()),
            base.path().join(DATA_DIR_LEAF)
        );
        assert!(fs::symlink_metadata(&legacy).is_ok());
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
