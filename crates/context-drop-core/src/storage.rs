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

/// Bundle identifier of the desktop app (`tauri.conf.json`). On macOS it is no
/// longer the data directory name — see [`DATA_DIR_LEAF`].
pub const APP_IDENTIFIER: &str = "com.contextdrop.app";

/// Data directory leaf used up to v0.1.10 on every OS (equal to the bundle
/// identifier), and still used on Windows and Linux.
pub const LEGACY_DATA_DIR_LEAF: &str = "com.contextdrop.app";

/// Leaf directory name under the OS data dir.
///
/// On macOS this is deliberately *not* the bundle identifier: a directory whose
/// name ends in `.app` is an application bundle to LaunchServices. The data
/// dir's top level changes every time SQLite opens the database (`-wal`/`-shm`
/// files come and go), so Finder and open/save panel services kept asking `lsd`
/// to re-register an "app" with no executable. That pinned `lsd` at 100% CPU
/// and made copy/paste slow system-wide (v0.1.10 and earlier). Other OSes have
/// no such notion and keep the old name, so nothing moves there.
#[cfg(target_os = "macos")]
pub const DATA_DIR_LEAF: &str = "com.contextdrop.desktop";
#[cfg(not(target_os = "macos"))]
pub const DATA_DIR_LEAF: &str = LEGACY_DATA_DIR_LEAF;

/// Environment variable that overrides the resolved data directory.
pub const DATA_DIR_ENV: &str = "CONTEXT_DROP_DATA_DIR";

/// Resolve the Context Drop data directory (does not create or move it).
///
/// `CONTEXT_DROP_DATA_DIR` wins. Otherwise `<os data dir>/<DATA_DIR_LEAF>`,
/// except while only the pre-0.1.11 directory exists on macOS (the desktop app
/// has not moved it yet): then that one, so the CLI keeps seeing the user's
/// packets.
pub fn data_dir() -> Result<PathBuf> {
    if let Some(explicit) = env_override() {
        return Ok(explicit);
    }
    let base = dirs::data_dir().ok_or(CoreError::NoDataDir)?;
    Ok(select_data_dir_in(
        &base,
        DATA_DIR_LEAF,
        LEGACY_DATA_DIR_LEAF,
    ))
}

fn env_override() -> Option<PathBuf> {
    match std::env::var_os(DATA_DIR_ENV) {
        Some(v) if !v.is_empty() => Some(PathBuf::from(v)),
        _ => None,
    }
}

/// Pick `base/<current>` unless it is absent and `base/<legacy>` is a
/// directory (or a link to one). Never changes the filesystem.
pub fn select_data_dir_in(base: &Path, current: &str, legacy: &str) -> PathBuf {
    let cur = base.join(current);
    let old = base.join(legacy);
    if current != legacy && !exists_no_follow(&cur) && holds_legacy_data(&old, current) {
        return old;
    }
    cur
}

/// Result of [`migrate_legacy_data_dir`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataDirMigration {
    /// `CONTEXT_DROP_DATA_DIR` is set; nothing was looked at.
    Overridden,
    /// Nothing to move: not macOS, a fresh install, or already moved.
    NotNeeded,
    /// The old directory was renamed to the current location. `link_left` is
    /// true when the old name was put back as a symlink to the new directory.
    Moved {
        from: PathBuf,
        to: PathBuf,
        link_left: bool,
    },
    /// Both exist as separate directories; the current one is used and the old
    /// one is left untouched (never merged or deleted automatically).
    BothPresent { legacy: PathBuf },
    /// The rename failed; the old directory stays in use and the move is
    /// retried on the next start.
    Failed { legacy: PathBuf, error: String },
    /// Already moved, but the link at the old name was missing and was put
    /// back (`ok`) or could not be (retried on the next start).
    LinkRestored { ok: bool },
}

/// macOS: move the pre-0.1.11 data directory `com.contextdrop.app` to
/// [`DATA_DIR_LEAF`] and leave `com.contextdrop.app` behind as a symlink to it.
///
/// The symlink keeps every old reference working — a v0.1.10 Claude Code
/// plugin's `claim.sh`, the `~/.local/bin/context-drop` link, a CLI that
/// resolved the old path just before the move — while a *symlink* named
/// `.app` does not make `lsd` spin (measured: a churning real `.app` dir 92
/// failed registrations / 40 s, a `.app` symlink to a churning dir 0).
///
/// Called by the desktop app once at startup, before anything opens the
/// database. The CLI never moves anything. Idempotent; never deletes data.
pub fn migrate_legacy_data_dir() -> DataDirMigration {
    if env_override().is_some() {
        return DataDirMigration::Overridden;
    }
    if DATA_DIR_LEAF == LEGACY_DATA_DIR_LEAF {
        return DataDirMigration::NotNeeded;
    }
    let Some(base) = dirs::data_dir() else {
        return DataDirMigration::NotNeeded;
    };
    migrate_legacy_data_dir_in(&base, DATA_DIR_LEAF, LEGACY_DATA_DIR_LEAF)
}

/// [`migrate_legacy_data_dir`] for explicit leaf names under `base`.
#[cfg(unix)]
pub fn migrate_legacy_data_dir_in(base: &Path, current: &str, legacy: &str) -> DataDirMigration {
    if current == legacy {
        return DataDirMigration::NotNeeded;
    }
    let cur = base.join(current);
    let old = base.join(legacy);
    let tmp = base.join(format!(".{legacy}.moving"));
    // Leftover from a start that stopped between the two renames.
    if fs::symlink_metadata(&tmp).is_ok_and(|m| m.file_type().is_symlink()) {
        let _ = fs::remove_file(&tmp);
    }
    if !exists_no_follow(&old) {
        // Already moved but the link is missing (a start that stopped half way,
        // or the link could not be made): put it back on every start.
        if is_real_dir(&cur) {
            let ok = std::os::unix::fs::symlink(current, &old).is_ok();
            return DataDirMigration::LinkRestored { ok };
        }
        return DataDirMigration::NotNeeded;
    }
    if is_our_link(&old, current) || !holds_legacy_data(&old, current) {
        return DataDirMigration::NotNeeded; // done earlier, or a plain file
    }
    if exists_no_follow(&cur) {
        let same = fs::canonicalize(&old).ok() == fs::canonicalize(&cur).ok();
        if same {
            return DataDirMigration::NotNeeded;
        }
        return DataDirMigration::BothPresent { legacy: old };
    }
    // Prepare the replacement link first, so the old name is missing for only
    // the instant between the two renames. Relative, so it survives the base
    // directory being reached through another path.
    let link_ready = std::os::unix::fs::symlink(current, &tmp).is_ok();
    // A user's own symlink at the old name (even one whose target is offline)
    // moves as a link; the data it points to stays where it is.
    if let Err(e) = fs::rename(&old, &cur) {
        let _ = fs::remove_file(&tmp);
        return DataDirMigration::Failed {
            legacy: old,
            error: e.to_string(),
        };
    }
    // rename() of a link onto an existing directory fails, so this never
    // replaces a directory something recreated in that instant. A missing link
    // is put back on the next start (LinkRestored).
    let link_left = link_ready && fs::rename(&tmp, &old).is_ok();
    if !link_left {
        let _ = fs::remove_file(&tmp);
    }
    DataDirMigration::Moved {
        from: old,
        to: cur,
        link_left,
    }
}

#[cfg(not(unix))]
pub fn migrate_legacy_data_dir_in(_base: &Path, _current: &str, _legacy: &str) -> DataDirMigration {
    DataDirMigration::NotNeeded
}

fn exists_no_follow(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok()
}

fn is_dir_following_links(p: &Path) -> bool {
    fs::metadata(p).map(|m| m.is_dir()).unwrap_or(false)
}

fn is_real_dir(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_dir())
}

/// The link this module leaves at the old name (relative, to `current`).
fn is_our_link(old: &Path, current: &str) -> bool {
    fs::read_link(old).is_ok_and(|t| t == Path::new(current))
}

/// The old name holds the user's data: a directory, or a user's own symlink
/// (followed when its target is reachable; kept even when it is offline, so a
/// fresh empty directory is never started in its place).
fn holds_legacy_data(old: &Path, current: &str) -> bool {
    if is_our_link(old, current) {
        return false;
    }
    is_dir_following_links(old)
        || fs::symlink_metadata(old).is_ok_and(|m| m.file_type().is_symlink())
}

/// Filesystem paths for a resolved data directory.
#[derive(Debug, Clone)]
pub struct Storage {
    root: PathBuf,
    /// Resolved to the pre-0.1.11 macOS location: it must already exist, and
    /// is never created (see [`Storage::check_root`]).
    legacy_root: bool,
}

impl Storage {
    /// Construct a `Storage` rooted at the standard data directory.
    pub fn resolve() -> Result<Storage> {
        let root = data_dir()?;
        let legacy_root = env_override().is_none()
            && DATA_DIR_LEAF != LEGACY_DATA_DIR_LEAF
            && root.file_name() == Some(std::ffi::OsStr::new(LEGACY_DATA_DIR_LEAF));
        Ok(Storage { root, legacy_root })
    }

    /// Construct a `Storage` rooted at an explicit directory.
    pub fn at(root: impl Into<PathBuf>) -> Storage {
        Storage {
            root: root.into(),
            legacy_root: false,
        }
    }

    /// When this process picked the old macOS location (the desktop app had
    /// not moved it yet) and it has since gone, the desktop app has just moved
    /// it. Fail instead of creating a real `com.contextdrop.app` directory again,
    /// which would split the data and bring back the `.app` folder; the next
    /// run resolves the new location.
    fn check_root(&self) -> Result<()> {
        if self.legacy_root && !exists_no_follow(&self.root) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "the Context Drop data directory was just moved to com.contextdrop.desktop; run the command again",
            )
            .into());
        }
        Ok(())
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
        self.check_root()?;
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
        self.check_root()?;
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

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_data_dir_leaf_is_not_an_app_bundle_name() {
        // macOS treats any directory ending in `.app` as an application bundle;
        // the data dir must never be one (v0.1.10 lsd/copy-paste regression).
        assert!(!DATA_DIR_LEAF.ends_with(".app"));
        assert_ne!(DATA_DIR_LEAF, LEGACY_DATA_DIR_LEAF);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn other_os_keeps_the_old_leaf_and_never_moves() {
        assert_eq!(DATA_DIR_LEAF, LEGACY_DATA_DIR_LEAF);
    }

    // The tests below use explicit leaf names so they run on every Unix CI.
    const NEW: &str = "com.example.desktop";
    const OLD: &str = "com.example.app";

    #[test]
    fn select_prefers_current_and_falls_back_to_an_existing_legacy_dir() {
        let base = tempfile::tempdir().unwrap();
        assert_eq!(
            select_data_dir_in(base.path(), NEW, OLD),
            base.path().join(NEW)
        );
        fs::create_dir(base.path().join(OLD)).unwrap();
        assert_eq!(
            select_data_dir_in(base.path(), NEW, OLD),
            base.path().join(OLD)
        );
        fs::create_dir(base.path().join(NEW)).unwrap();
        assert_eq!(
            select_data_dir_in(base.path(), NEW, OLD),
            base.path().join(NEW)
        );
        // Same leaf (Windows/Linux): always that one.
        assert_eq!(
            select_data_dir_in(base.path(), OLD, OLD),
            base.path().join(OLD)
        );
    }

    #[cfg(unix)]
    mod migrate {
        use super::*;

        fn seed_old(base: &Path) -> PathBuf {
            let old = base.join(OLD);
            fs::create_dir_all(old.join("packets/p1")).unwrap();
            fs::create_dir_all(old.join("bin")).unwrap();
            fs::write(old.join("context-drop.db"), b"db").unwrap();
            fs::write(old.join("bin/context-drop"), b"cli").unwrap();
            old
        }

        #[test]
        fn fresh_install_needs_nothing_and_creates_nothing() {
            let base = tempfile::tempdir().unwrap();
            assert_eq!(
                migrate_legacy_data_dir_in(base.path(), NEW, OLD),
                DataDirMigration::NotNeeded
            );
            assert_eq!(fs::read_dir(base.path()).unwrap().count(), 0);
        }

        #[test]
        fn moves_the_old_dir_and_leaves_a_link_that_old_paths_still_reach() {
            let base = tempfile::tempdir().unwrap();
            let old = seed_old(base.path());
            let new = base.path().join(NEW);
            assert_eq!(
                migrate_legacy_data_dir_in(base.path(), NEW, OLD),
                DataDirMigration::Moved {
                    from: old.clone(),
                    to: new.clone(),
                    link_left: true
                }
            );
            assert!(
                fs::symlink_metadata(&new).unwrap().is_dir(),
                "data is a real dir at the new name"
            );
            assert!(
                fs::symlink_metadata(&old).unwrap().file_type().is_symlink(),
                "old name is a link"
            );
            assert_eq!(
                fs::read_link(&old).unwrap(),
                PathBuf::from(NEW),
                "relative link"
            );
            // Old paths (v0.1.10 plugin, ~/.local/bin link) still work.
            assert_eq!(fs::read(old.join("bin/context-drop")).unwrap(), b"cli");
            // Writes by the old path land in the new dir (no split).
            fs::write(old.join("packets/p1/x"), b"via-old").unwrap();
            assert_eq!(fs::read(new.join("packets/p1/x")).unwrap(), b"via-old");
            assert_eq!(select_data_dir_in(base.path(), NEW, OLD), new);
            assert!(!base.path().join(format!(".{OLD}.moving")).exists());
            // Idempotent on the next start.
            assert_eq!(
                migrate_legacy_data_dir_in(base.path(), NEW, OLD),
                DataDirMigration::NotNeeded
            );
        }

        #[test]
        fn both_real_dirs_keep_current_and_never_touch_the_old_one() {
            let base = tempfile::tempdir().unwrap();
            let old = seed_old(base.path());
            let new = base.path().join(NEW);
            fs::create_dir(&new).unwrap();
            fs::write(new.join("context-drop.db"), b"new").unwrap();
            assert_eq!(
                migrate_legacy_data_dir_in(base.path(), NEW, OLD),
                DataDirMigration::BothPresent {
                    legacy: old.clone()
                }
            );
            assert_eq!(fs::read(old.join("context-drop.db")).unwrap(), b"db");
            assert_eq!(fs::read(new.join("context-drop.db")).unwrap(), b"new");
            assert_eq!(select_data_dir_in(base.path(), NEW, OLD), new);
        }

        #[test]
        fn a_users_own_link_at_the_old_name_keeps_pointing_at_their_data() {
            let base = tempfile::tempdir().unwrap();
            let elsewhere = tempfile::tempdir().unwrap();
            fs::write(elsewhere.path().join("context-drop.db"), b"relocated").unwrap();
            let old = base.path().join(OLD);
            std::os::unix::fs::symlink(elsewhere.path(), &old).unwrap();
            // Before the move the CLI follows the link, as v0.1.10 did.
            assert_eq!(select_data_dir_in(base.path(), NEW, OLD), old);
            let out = migrate_legacy_data_dir_in(base.path(), NEW, OLD);
            assert!(
                matches!(
                    out,
                    DataDirMigration::Moved {
                        link_left: true,
                        ..
                    }
                ),
                "{out:?}"
            );
            let new = base.path().join(NEW);
            assert_eq!(fs::read(new.join("context-drop.db")).unwrap(), b"relocated");
            assert_eq!(fs::read(old.join("context-drop.db")).unwrap(), b"relocated");
            assert_eq!(
                fs::read(elsewhere.path().join("context-drop.db")).unwrap(),
                b"relocated"
            );
        }

        #[test]
        fn a_plain_file_at_the_old_name_is_left_alone() {
            let base = tempfile::tempdir().unwrap();
            let old = base.path().join(OLD);
            fs::write(&old, b"x").unwrap();
            assert_eq!(
                migrate_legacy_data_dir_in(base.path(), NEW, OLD),
                DataDirMigration::NotNeeded
            );
            assert_eq!(fs::read(&old).unwrap(), b"x");
            assert!(!base.path().join(NEW).exists());
            assert_eq!(
                select_data_dir_in(base.path(), NEW, OLD),
                base.path().join(NEW)
            );
        }

        #[test]
        fn a_missing_link_after_a_half_finished_move_is_put_back() {
            let base = tempfile::tempdir().unwrap();
            let old = seed_old(base.path());
            migrate_legacy_data_dir_in(base.path(), NEW, OLD);
            // Simulate a start that stopped between the renames.
            fs::remove_file(&old).unwrap();
            let tmp = base.path().join(format!(".{OLD}.moving"));
            std::os::unix::fs::symlink(NEW, &tmp).unwrap();
            assert_eq!(
                migrate_legacy_data_dir_in(base.path(), NEW, OLD),
                DataDirMigration::LinkRestored { ok: true }
            );
            assert_eq!(fs::read_link(&old).unwrap(), PathBuf::from(NEW));
            assert_eq!(fs::read(old.join("bin/context-drop")).unwrap(), b"cli");
            assert!(
                fs::symlink_metadata(&tmp).is_err(),
                "stale tmp link removed"
            );
            assert_eq!(
                migrate_legacy_data_dir_in(base.path(), NEW, OLD),
                DataDirMigration::NotNeeded
            );
        }

        #[test]
        fn a_users_link_whose_target_is_offline_is_kept_not_replaced_by_an_empty_dir() {
            let base = tempfile::tempdir().unwrap();
            let gone = base.path().join("unmounted-volume/cd");
            let old = base.path().join(OLD);
            std::os::unix::fs::symlink(&gone, &old).unwrap();
            // Selection keeps the user's link (opening it fails loudly, as in v0.1.10).
            assert_eq!(select_data_dir_in(base.path(), NEW, OLD), old);
            let out = migrate_legacy_data_dir_in(base.path(), NEW, OLD);
            assert!(
                matches!(
                    out,
                    DataDirMigration::Moved {
                        link_left: true,
                        ..
                    }
                ),
                "{out:?}"
            );
            let new = base.path().join(NEW);
            assert_eq!(
                fs::read_link(&new).unwrap(),
                gone,
                "the user's link moved as is"
            );
            assert_eq!(select_data_dir_in(base.path(), NEW, OLD), new);
            // When the volume comes back, both names reach the user's data.
            fs::create_dir_all(&gone).unwrap();
            fs::write(gone.join("context-drop.db"), b"theirs").unwrap();
            assert_eq!(fs::read(new.join("context-drop.db")).unwrap(), b"theirs");
            assert_eq!(fs::read(old.join("context-drop.db")).unwrap(), b"theirs");
        }

        #[test]
        fn our_link_with_the_data_deleted_is_left_alone() {
            let base = tempfile::tempdir().unwrap();
            let old = base.path().join(OLD);
            std::os::unix::fs::symlink(NEW, &old).unwrap();
            assert_eq!(
                migrate_legacy_data_dir_in(base.path(), NEW, OLD),
                DataDirMigration::NotNeeded
            );
            assert_eq!(
                select_data_dir_in(base.path(), NEW, OLD),
                base.path().join(NEW)
            );
            assert!(!base.path().join(NEW).exists(), "no self-referencing move");
        }

        #[test]
        fn a_storage_on_the_old_location_never_recreates_it_after_the_move() {
            let base = tempfile::tempdir().unwrap();
            let old = base.path().join(OLD);
            fs::create_dir(&old).unwrap();
            let st = Storage {
                root: old.clone(),
                legacy_root: true,
            };
            st.ensure_layout().unwrap();
            // The desktop app moves it before the link is in place.
            fs::rename(&old, base.path().join(NEW)).unwrap();
            assert!(st.ensure_layout().is_err());
            assert!(st.ensure_packet_dirs("p1").is_err());
            assert!(st.write_item_atomic("p1", 1, "txt", b"x").is_err());
            assert!(
                fs::symlink_metadata(&old).is_err(),
                "no real old dir recreated"
            );
            // Once the link is back, the same storage works through it.
            std::os::unix::fs::symlink(NEW, &old).unwrap();
            st.ensure_packet_dirs("p1").unwrap();
            assert!(base.path().join(NEW).join("packets/p1/items").is_dir());
        }

        #[test]
        fn same_leaf_never_moves() {
            let base = tempfile::tempdir().unwrap();
            seed_old(base.path());
            assert_eq!(
                migrate_legacy_data_dir_in(base.path(), OLD, OLD),
                DataDirMigration::NotNeeded
            );
            assert!(fs::symlink_metadata(base.path().join(OLD))
                .unwrap()
                .is_dir());
        }
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
