//! Claude Code integration installer.
//!
//! Design constraints (spec §38–§40):
//! - Idempotent and additive; never wholesale-overwrite a user's settings.
//! - Works with multiple `CLAUDE_CONFIG_DIR` roots; the Context Drop data store
//!   stays global per OS user (installing into a config root never touches it).
//! - Enabling the plugin uses Claude Code's supported `/plugin` mechanism. We
//!   lay down a self-contained local marketplace and print the exact commands,
//!   rather than mutating an undocumented internal registry across a user's
//!   many accounts.

use std::fs;
use std::path::{Path, PathBuf};

/// Marketplace + plugin identifier used on disk.
pub const MARKETPLACE_NAME: &str = "context-drop";
pub const PLUGIN_NAME: &str = "context-drop";

#[derive(Debug)]
pub struct PluginInstallReport {
    pub config_dir: PathBuf,
    pub marketplace_dir: PathBuf,
    pub already_present: bool,
}

#[derive(Debug)]
pub struct AliasInstallReport {
    pub skill_path: PathBuf,
    pub overwritten: bool,
    pub skipped_existing: bool,
}

/// Resolve the plugin source directory (the repo's `integrations/claude-code`,
/// or the bundled copy shipped with the desktop app).
pub fn resolve_plugin_source(
    explicit: Option<&Path>,
    data_dir: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        return validate_plugin_dir(p);
    }
    if let Some(env) = std::env::var_os("CONTEXT_DROP_PLUGIN_DIR") {
        if !env.is_empty() {
            return validate_plugin_dir(Path::new(&env));
        }
    }
    // Prefer a FRESH source (bundle/repo, relative to the running executable)
    // over the staged copy, so a newer plugin always wins.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for rel in [
                "../share/context-drop/claude-code",
                "../Resources/claude-code",
                "integrations/claude-code",
                // Dev builds: repo/target/debug/context-drop -> repo/integrations
                "../../integrations/claude-code",
                // Cross-target dev: repo/target/<triple>/debug -> repo/integrations
                "../../../integrations/claude-code",
            ] {
                let cand = dir.join(rel);
                if cand.join(".claude-plugin/plugin.json").is_file() {
                    return Ok(cand);
                }
            }
        }
    }
    // Last resort: the plugin tree staged under the effective data dir (the
    // caller's --data-dir when given, else the global default). Lets a CLI
    // relocated to `<data-dir>/bin` still install into other config roots.
    let effective_dd = data_dir
        .map(|d| d.to_path_buf())
        .or_else(|| crate::storage::data_dir().ok());
    if let Some(dd) = effective_dd {
        let cand = dd.join("claude-code");
        if cand.join(".claude-plugin/plugin.json").is_file() {
            return Ok(cand);
        }
    }
    Err("could not locate the Context Drop plugin source; pass --from <dir>".to_string())
}

fn validate_plugin_dir(p: &Path) -> Result<PathBuf, String> {
    if p.join(".claude-plugin/plugin.json").is_file() {
        Ok(p.to_path_buf())
    } else {
        Err(format!(
            "{} is not a Context Drop plugin directory (missing .claude-plugin/plugin.json)",
            p.display()
        ))
    }
}

/// Install (or refresh) the plugin as a local marketplace under a config root.
pub fn install_plugin(config_dir: &Path, plugin_src: &Path) -> Result<PluginInstallReport, String> {
    let marketplace_dir = config_dir
        .join("plugins")
        .join("marketplaces")
        .join(MARKETPLACE_NAME);
    let dest_plugin = marketplace_dir.join("plugins").join(PLUGIN_NAME);
    let already_present = dest_plugin.join(".claude-plugin/plugin.json").is_file();

    // Guard against `--from` pointing at the destination itself: removing then
    // recopying would delete the source and produce an empty tree.
    let same_src = fs::canonicalize(plugin_src)
        .ok()
        .zip(fs::canonicalize(&dest_plugin).ok())
        .map(|(a, b)| a == b)
        .unwrap_or(false);

    // Skip the copy when the source IS the destination, but still (re)write the
    // marketplace manifest below so a partial install is repaired.
    if !same_src {
        copy_tree_atomic(plugin_src, &dest_plugin)?;
    }

    // Write the marketplace manifest.
    let version = read_plugin_version(plugin_src).unwrap_or_else(|| "0.1.0".to_string());
    let manifest = serde_json::json!({
        "$schema": "https://anthropic.com/claude-code/marketplace.schema.json",
        "name": MARKETPLACE_NAME,
        "description": "Context Drop — local clipboard-to-subagent context routing for Claude Code.",
        "owner": {
            "name": "Context Drop contributors",
            "url": "https://github.com/context-drop/context-drop"
        },
        "plugins": [
            {
                "name": PLUGIN_NAME,
                "source": "./plugins/context-drop",
                "description": "Route a captured Context Drop packet into this Claude Code session and process it in an isolated subagent.",
                "version": version,
                "category": "productivity"
            }
        ]
    });
    let cp_dir = marketplace_dir.join(".claude-plugin");
    fs::create_dir_all(&cp_dir).map_err(io_err)?;
    fs::write(
        cp_dir.join("marketplace.json"),
        serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(io_err)?;

    Ok(PluginInstallReport {
        config_dir: config_dir.to_path_buf(),
        marketplace_dir,
        already_present,
    })
}

/// Install the optional `/cd` short-alias skill at the user level.
/// Refuses to overwrite an existing `/cd` skill unless `force` is set.
pub fn install_short_alias(
    config_dir: &Path,
    alias_src: &Path,
    force: bool,
) -> Result<AliasInstallReport, String> {
    let skill_dir = config_dir.join("skills").join("cd");
    let skill_path = skill_dir.join("SKILL.md");
    let existed = skill_path.exists();
    if existed && !force {
        return Ok(AliasInstallReport {
            skill_path,
            overwritten: false,
            skipped_existing: true,
        });
    }
    fs::create_dir_all(&skill_dir).map_err(io_err)?;
    let src = alias_src.join("SKILL.md");
    fs::copy(&src, &skill_path).map_err(io_err)?;
    Ok(AliasInstallReport {
        skill_path,
        overwritten: existed,
        skipped_existing: false,
    })
}

/// Candidate Claude config directories: `CLAUDE_CONFIG_DIR` (which may name
/// several, comma-separated) plus the default `~/.claude`.
pub fn detect_config_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(env) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        let s = env.to_string_lossy().to_string();
        for part in s.split([',', ';']).map(str::trim).filter(|p| !p.is_empty()) {
            dirs.push(PathBuf::from(part));
        }
    }
    if let Some(home) = dirs::home_dir() {
        let default = home.join(".claude");
        if !dirs.iter().any(|d| d == &default) {
            dirs.push(default);
        }
    }
    dirs
}

/// Whether the Context Drop plugin marketplace is present under a config root.
pub fn is_plugin_installed(config_dir: &Path) -> bool {
    config_dir
        .join("plugins/marketplaces")
        .join(MARKETPLACE_NAME)
        .join("plugins")
        .join(PLUGIN_NAME)
        .join(".claude-plugin/plugin.json")
        .is_file()
}

fn read_plugin_version(plugin_src: &Path) -> Option<String> {
    let raw = fs::read_to_string(plugin_src.join(".claude-plugin/plugin.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("version")?.as_str().map(|s| s.to_string())
}

/// Copy a plugin tree into `dest` via a temp dir + swap, so an interrupted or
/// failed copy never leaves a partial installation in place: build the new tree
/// under a temp sibling, validate it, then remove the old and rename into place.
fn copy_tree_atomic(src: &Path, dest: &Path) -> Result<(), String> {
    let parent = dest
        .parent()
        .ok_or_else(|| "destination has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(io_err)?;
    let leaf = dest
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "plugin".to_string());
    let tmp = parent.join(format!(".{leaf}.tmp-{}", uuid::Uuid::now_v7().simple()));
    if tmp.exists() {
        let _ = fs::remove_dir_all(&tmp);
    }
    if let Err(e) = copy_dir_recursive(src, &tmp) {
        let _ = fs::remove_dir_all(&tmp);
        return Err(e);
    }
    // Validate the staged tree before publishing it.
    if !tmp.join(".claude-plugin/plugin.json").is_file() {
        let _ = fs::remove_dir_all(&tmp);
        return Err("staged plugin tree is incomplete (missing plugin.json)".to_string());
    }
    // Move the existing tree aside to a backup, publish the new one, then drop
    // the backup — restoring it if the publish rename fails, so a failure never
    // destroys the previous working installation.
    let backup = parent.join(format!(".{leaf}.bak-{}", uuid::Uuid::now_v7().simple()));
    let had_dest = dest.exists();
    if had_dest {
        if let Err(e) = fs::rename(dest, &backup) {
            let _ = fs::remove_dir_all(&tmp);
            return Err(io_err(e));
        }
    }
    match fs::rename(&tmp, dest) {
        Ok(()) => {
            if had_dest {
                let _ = fs::remove_dir_all(&backup);
            }
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            if had_dest {
                if let Err(re) = fs::rename(&backup, dest) {
                    // Both failed: surface the preserved backup path so the user
                    // can recover the previous installation manually.
                    return Err(format!(
                        "publish failed ({e}); rollback also failed ({re}); \
                         the previous installation is preserved at {}",
                        backup.display()
                    ));
                }
            }
            Err(io_err(e))
        }
    }
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(io_err)?;
    for entry in fs::read_dir(src).map_err(io_err)? {
        let entry = entry.map_err(io_err)?;
        let file_type = entry.file_type().map_err(io_err)?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if file_type.is_file() {
            fs::copy(&from, &to).map_err(io_err)?;
        }
        // Symlinks and other exotic entries are intentionally skipped.
    }
    Ok(())
}

fn io_err(e: std::io::Error) -> String {
    e.to_string()
}

// ---- CLI distribution (see docs/cli-distribution.md) --------------------

/// The CLI's file name on this platform.
pub fn cli_file_name() -> &'static str {
    if cfg!(windows) {
        "context-drop.exe"
    } else {
        "context-drop"
    }
}

/// Whether an installed CLI copy exists and is runnable (executable on Unix).
pub fn is_usable_cli(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// The canonical installed CLI path: `<data-dir>/bin/context-drop[.exe]`.
/// This is the guaranteed fallback the Claude Code skills resolve to; it lives
/// under the SAME data dir the core resolver returns on every OS.
pub fn cli_install_path(data_dir: &Path) -> PathBuf {
    data_dir.join("bin").join(cli_file_name())
}

/// Result of installing the CLI onto the local machine.
#[derive(Debug)]
pub struct CliInstallReport {
    /// The canonical copy at `<data-dir>/bin/context-drop[.exe]`.
    pub installed_path: PathBuf,
    /// A convenience symlink created on PATH (Unix `~/.local/bin`), if any.
    pub symlinked: Option<PathBuf>,
    /// Whether a directory holding the CLI is already on `PATH`.
    pub on_path: bool,
    /// A one-line hint for the user to add the CLI to `PATH`, when not present.
    pub path_hint: Option<String>,
}

/// Install (copy) the CLI to the canonical `<data-dir>/bin` location and, on
/// Unix, best-effort symlink it into `~/.local/bin`. `source_exe` is the CLI
/// binary to copy (the desktop's bundled sidecar, or the running CLI itself).
///
/// Idempotent and additive; never edits shell rc files or system PATH.
pub fn install_cli(data_dir: &Path, source_exe: &Path) -> Result<CliInstallReport, String> {
    // The canonical copy is essential; a failure here is a hard error.
    let dest = refresh_cli(data_dir, source_exe)?;
    let bin_dir = dest.parent().unwrap_or(data_dir).to_path_buf();

    // Symlink into ~/.local/bin is a best-effort PATH convenience. Anchor it to
    // the canonicalized destination so a relative data-dir override still yields
    // an absolute, valid link target.
    let link_target = fs::canonicalize(&dest).unwrap_or_else(|_| dest.clone());
    let symlinked = symlink_into_local_bin(&link_target);
    let (on_path, path_hint) = path_status(&bin_dir, symlinked.as_deref());

    Ok(CliInstallReport {
        installed_path: dest,
        symlinked,
        on_path,
        path_hint,
    })
}

/// Copy (refresh) the canonical CLI copy at `<data-dir>/bin/context-drop[.exe]`
/// without touching PATH/symlinks. Used on desktop startup so the managed copy
/// always matches the running app version (design D5). Skips a self-copy.
pub fn refresh_cli(data_dir: &Path, source_exe: &Path) -> Result<PathBuf, String> {
    let dest = cli_install_path(data_dir);
    let bin_dir = dest.parent().unwrap_or(data_dir).to_path_buf();
    fs::create_dir_all(&bin_dir).map_err(io_err)?;
    let same = fs::canonicalize(source_exe)
        .ok()
        .zip(fs::canonicalize(&dest).ok())
        .map(|(a, b)| a == b)
        .unwrap_or(false);
    if !same {
        copy_executable(source_exe, &dest)?;
    }
    Ok(dest)
}

/// Stage the plugin tree under `<data-dir>/claude-code` so a relocated CLI can
/// still resolve the plugin source for `install-claude` into other config roots.
pub fn stage_plugin(data_dir: &Path, plugin_src: &Path) -> Result<PathBuf, String> {
    let dest = data_dir.join("claude-code");
    // Never wipe the staged copy when it IS the source (e.g. a relocated CLI
    // resolved the plugin from the staged copy). That would delete then recreate
    // an empty tree.
    let same = fs::canonicalize(plugin_src)
        .ok()
        .zip(fs::canonicalize(&dest).ok())
        .map(|(a, b)| a == b)
        .unwrap_or(false);
    if same {
        return Ok(dest);
    }
    copy_tree_atomic(plugin_src, &dest)?;
    Ok(dest)
}

/// Atomically copy an executable, preserving the executable bit on Unix.
fn copy_executable(src: &Path, dest: &Path) -> Result<(), String> {
    let bytes = fs::read(src).map_err(io_err)?;
    let tmp = dest.with_file_name(format!(
        ".{}.tmp-{}",
        cli_file_name(),
        uuid::Uuid::now_v7().simple()
    ));
    if let Err(e) = fs::write(&tmp, &bytes).and_then(|()| {
        set_executable(&tmp).map_err(std::io::Error::other)?;
        fs::rename(&tmp, dest)
    }) {
        // Never leave an orphan temp executable behind (e.g. a Windows sharing
        // violation renaming over a locked destination).
        let _ = fs::remove_file(&tmp);
        return Err(io_err(e));
    }
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path).map_err(io_err)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).map_err(io_err)
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Best-effort symlink `~/.local/bin/context-drop` -> `target` (Unix only).
/// Collision rules: skip if `~/.local/bin` is a non-directory; keep an existing
/// correct link; never clobber an unrelated file/link.
#[cfg(unix)]
fn symlink_into_local_bin(target: &Path) -> Option<PathBuf> {
    // `CONTEXT_DROP_LOCAL_BIN` overrides the symlink directory (tests point this
    // at a temp dir so the real ~/.local/bin is never touched).
    let local_bin = match std::env::var_os("CONTEXT_DROP_LOCAL_BIN") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => dirs::home_dir()?.join(".local").join("bin"),
    };
    match fs::symlink_metadata(&local_bin) {
        Ok(md) if !md.is_dir() => return None, // non-directory: leave it alone
        Ok(_) => {}
        Err(_) => {
            fs::create_dir_all(&local_bin).ok()?;
        }
    }
    let link = local_bin.join("context-drop");
    match fs::symlink_metadata(&link) {
        Ok(md) => {
            // Already present: only accept it if it is our symlink to `target`.
            if md.file_type().is_symlink() {
                if let Ok(dst) = fs::read_link(&link) {
                    if dst == target {
                        return Some(link);
                    }
                }
            }
            None // unrelated file/link: do not clobber
        }
        Err(_) => {
            std::os::unix::fs::symlink(target, &link).ok()?;
            Some(link)
        }
    }
}

#[cfg(not(unix))]
fn symlink_into_local_bin(_target: &Path) -> Option<PathBuf> {
    None
}

/// Whether the CLI is reachable via PATH, and a hint to add it if not.
fn path_status(bin_dir: &Path, symlinked: Option<&Path>) -> (bool, Option<String>) {
    let on_path = dir_on_path(bin_dir)
        || symlinked
            .and_then(|l| l.parent())
            .map(dir_on_path)
            .unwrap_or(false);
    if on_path {
        (true, None)
    } else {
        let hint = if cfg!(windows) {
            format!("Add this directory to your PATH: {}", bin_dir.display())
        } else {
            let d = symlinked
                .and_then(|l| l.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| bin_dir.to_path_buf());
            format!("Add to PATH, e.g.: export PATH=\"{}:$PATH\"", d.display())
        };
        (false, Some(hint))
    }
}

fn dir_on_path(dir: &Path) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|p| p == dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Serializes tests that mutate the process-global CONTEXT_DROP_LOCAL_BIN.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn install_cli_copies_to_data_bin_and_symlinks_into_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        let src = tempfile::tempdir().unwrap();
        let exe = src.path().join("context-drop-fake");
        fs::write(&exe, b"#!/bin/sh\necho hi\n").unwrap();
        let data = tempfile::tempdir().unwrap();
        // Never touch the real ~/.local/bin.
        let local_bin = tempfile::tempdir().unwrap();
        std::env::set_var("CONTEXT_DROP_LOCAL_BIN", local_bin.path());

        let report = install_cli(data.path(), &exe).unwrap();
        assert_eq!(report.installed_path, cli_install_path(data.path()));
        assert!(report.installed_path.is_file());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&report.installed_path)
                .unwrap()
                .permissions()
                .mode();
            assert!(mode & 0o111 != 0, "installed CLI must be executable");
            let link = report.symlinked.expect("symlink created in override dir");
            assert!(fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink());
        }

        // Idempotent: a second run keeps a valid installed copy.
        let again = install_cli(data.path(), &exe).unwrap();
        assert!(again.installed_path.is_file());
    }

    #[test]
    fn install_cli_does_not_copy_a_file_onto_itself() {
        let _guard = ENV_LOCK.lock().unwrap();
        let data = tempfile::tempdir().unwrap();
        let dest = cli_install_path(data.path());
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::write(&dest, b"self-copy-marker").unwrap();
        let local_bin = tempfile::tempdir().unwrap();
        std::env::set_var("CONTEXT_DROP_LOCAL_BIN", local_bin.path());

        let report = install_cli(data.path(), &dest).unwrap();
        assert_eq!(
            fs::read(&report.installed_path).unwrap(),
            b"self-copy-marker"
        );
    }

    #[test]
    fn stage_plugin_copies_the_tree() {
        let src = tempfile::tempdir().unwrap();
        fake_plugin(src.path());
        let data = tempfile::tempdir().unwrap();
        let staged = stage_plugin(data.path(), src.path()).unwrap();
        assert!(staged.join(".claude-plugin/plugin.json").is_file());
        assert!(staged.join("skills/pull/SKILL.md").is_file());
    }

    /// Build a minimal fake plugin source tree.
    fn fake_plugin(dir: &Path) {
        fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
        fs::write(
            dir.join(".claude-plugin/plugin.json"),
            r#"{"name":"context-drop","version":"9.9.9","description":"x"}"#,
        )
        .unwrap();
        fs::create_dir_all(dir.join("skills/pull")).unwrap();
        fs::write(
            dir.join("skills/pull/SKILL.md"),
            "---\nname: pull\n---\nbody",
        )
        .unwrap();
    }

    #[test]
    fn install_is_idempotent_and_additive() {
        let src = tempfile::tempdir().unwrap();
        fake_plugin(src.path());
        let cfg = tempfile::tempdir().unwrap();

        // Pre-existing unrelated content must survive.
        let unrelated = cfg.path().join("settings.json");
        fs::write(&unrelated, "{\"keep\":true}").unwrap();

        let r1 = install_plugin(cfg.path(), src.path()).unwrap();
        assert!(!r1.already_present);
        assert!(is_plugin_installed(cfg.path()));
        // marketplace.json embeds the plugin version.
        let mp =
            fs::read_to_string(r1.marketplace_dir.join(".claude-plugin/marketplace.json")).unwrap();
        assert!(mp.contains("\"9.9.9\""));
        assert!(mp.contains("./plugins/context-drop"));
        // The copied skill exists.
        assert!(r1
            .marketplace_dir
            .join("plugins/context-drop/skills/pull/SKILL.md")
            .is_file());

        // Second run: idempotent, still additive.
        let r2 = install_plugin(cfg.path(), src.path()).unwrap();
        assert!(r2.already_present);
        assert_eq!(fs::read_to_string(&unrelated).unwrap(), "{\"keep\":true}");
    }

    #[test]
    fn short_alias_refuses_overwrite_without_force() {
        let src = tempfile::tempdir().unwrap();
        fs::write(src.path().join("SKILL.md"), "---\nname: cd\n---\nalias").unwrap();
        let cfg = tempfile::tempdir().unwrap();

        let r1 = install_short_alias(cfg.path(), src.path(), false).unwrap();
        assert!(!r1.skipped_existing);
        assert!(r1.skill_path.is_file());

        // Simulate a user's own /cd skill.
        fs::write(&r1.skill_path, "my own cd").unwrap();
        let r2 = install_short_alias(cfg.path(), src.path(), false).unwrap();
        assert!(r2.skipped_existing, "must not clobber existing /cd");
        assert_eq!(fs::read_to_string(&r1.skill_path).unwrap(), "my own cd");

        // With force, it overwrites.
        let r3 = install_short_alias(cfg.path(), src.path(), true).unwrap();
        assert!(r3.overwritten);
        assert!(fs::read_to_string(&r1.skill_path)
            .unwrap()
            .contains("alias"));
    }

    #[test]
    fn resolve_rejects_non_plugin_dir() {
        let empty = tempfile::tempdir().unwrap();
        assert!(resolve_plugin_source(Some(empty.path()), None).is_err());
    }
}
