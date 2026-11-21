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
/// The public GitHub marketplace slug users add with `/plugin marketplace add`.
pub const MARKETPLACE_GITHUB: &str = "EarthLinkNetwork/context-drop";

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
            "url": "https://github.com/EarthLinkNetwork/context-drop"
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

/// Install the optional `/cd` short-alias skill at the user level: every file
/// in `alias_src` (SKILL.md plus its `claim.sh`). An existing `/cd` that is
/// Context Drop's own alias (any version) is upgraded in place; a user's own
/// `/cd` skill is never overwritten unless `force` is set.
pub fn install_short_alias(
    config_dir: &Path,
    alias_src: &Path,
    force: bool,
) -> Result<AliasInstallReport, String> {
    let skill_dir = config_dir.join("skills").join("cd");
    let skill_path = skill_dir.join("SKILL.md");
    let existed = skill_path.exists();
    if existed && !force && !is_context_drop_alias(&skill_path) {
        return Ok(AliasInstallReport {
            skill_path,
            overwritten: false,
            skipped_existing: true,
        });
    }
    copy_dir_recursive(alias_src, &skill_dir)?;
    adapt_skill_file_for_os(&skill_path, cfg!(windows))?;
    Ok(AliasInstallReport {
        skill_path,
        overwritten: existed,
        skipped_existing: false,
    })
}

/// Refresh an ALREADY-installed Context Drop `/cd` from `alias_src` (desktop app
/// startup), so existing users get alias fixes without re-clicking Install.
/// Never installs `/cd` where it is absent (it is opt-in) and never touches a
/// user's own `/cd`. Returns whether it rewrote the alias.
pub fn refresh_short_alias(config_dir: &Path, alias_src: &Path) -> Result<bool, String> {
    let skill_path = config_dir.join("skills").join("cd").join("SKILL.md");
    if !is_context_drop_alias(&skill_path) {
        return Ok(false);
    }
    install_short_alias(config_dir, alias_src, false).map(|r| !r.skipped_existing)
}

/// The claim lines of the `/cd` SKILL.md. The repository file carries the
/// POSIX form (`sh claim.sh`); on Windows the installer rewrites them to run
/// `claim.ps1` through `powershell -File`, which means the same thing whether
/// Claude Code runs the `!` injection in Git Bash, PowerShell, or cmd.
const POSIX_ALLOWED_TOOLS: &str = r#"allowed-tools: Bash(sh "${CLAUDE_SKILL_DIR}/claim.sh" *)"#;
const POSIX_INJECTION: &str = r#"!`sh "${CLAUDE_SKILL_DIR}/claim.sh" "${CLAUDE_SESSION_ID}"`"#;
const WINDOWS_CLAIM_CMD: &str =
    r#"powershell -NoProfile -ExecutionPolicy Bypass -File "${CLAUDE_SKILL_DIR}/claim.ps1""#;

/// Rewrite the `/cd` SKILL.md for the target OS. (The plugin's pull skill has
/// no OS-specific lines: a GitHub-marketplace install never passes through this
/// installer, so pull has the model run the claim script itself.) A no-op for POSIX, and
/// idempotent (an already-adapted file has no POSIX lines left to replace).
pub fn adapt_skill_for_os(content: &str, windows: bool) -> String {
    if !windows {
        return content.to_string();
    }
    let allowed = format!(
        "allowed-tools:\n  - Bash({WINDOWS_CLAIM_CMD} *)\n  - PowerShell({WINDOWS_CLAIM_CMD} *)"
    );
    let injection = format!(r#"!`{WINDOWS_CLAIM_CMD} "${{CLAUDE_SESSION_ID}}"`"#);
    content
        .replace(POSIX_ALLOWED_TOOLS, &allowed)
        .replace(POSIX_INJECTION, &injection)
}

fn adapt_skill_file_for_os(path: &Path, windows: bool) -> Result<(), String> {
    if !windows || !path.is_file() {
        return Ok(());
    }
    let content = fs::read_to_string(path).map_err(io_err)?;
    let adapted = adapt_skill_for_os(&content, windows);
    if adapted != content {
        fs::write(path, adapted).map_err(io_err)?;
    }
    Ok(())
}

/// Whether an installed `/cd` SKILL.md is Context Drop's own short alias (every
/// released version names `/context-drop:pull` in its description).
fn is_context_drop_alias(skill_path: &Path) -> bool {
    fs::read_to_string(skill_path)
        .map(|s| s.contains("Short alias for /context-drop:pull"))
        .unwrap_or(false)
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

/// Whether the Context Drop plugin marketplace FILES are staged under a config
/// root (i.e. the desktop app's "Install" laid them down locally). This is not
/// the same as the plugin being enabled in Claude Code — see `is_plugin_enabled`.
pub fn is_plugin_installed(config_dir: &Path) -> bool {
    config_dir
        .join("plugins/marketplaces")
        .join(MARKETPLACE_NAME)
        .join("plugins")
        .join(PLUGIN_NAME)
        .join(".claude-plugin/plugin.json")
        .is_file()
}

/// The local marketplace directory the app stages under a config root (the path
/// you pass to `/plugin marketplace add` for the local, offline install path).
pub fn local_marketplace_dir(config_dir: &Path) -> PathBuf {
    config_dir
        .join("plugins")
        .join("marketplaces")
        .join(MARKETPLACE_NAME)
}

/// Whether the plugin is actually REGISTERED in Claude Code (the user ran
/// `/plugin install`), by inspecting `<config_dir>/plugins/installed_plugins.json`
/// for a `context-drop@<marketplace>` entry. Returns false if the file is absent
/// or unreadable.
pub fn is_plugin_enabled(config_dir: &Path) -> bool {
    let path = config_dir.join("plugins").join("installed_plugins.json");
    let Ok(raw) = fs::read_to_string(&path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return false;
    };
    let prefix = format!("{PLUGIN_NAME}@");
    value
        .get("plugins")
        .and_then(|p| p.as_object())
        .map(|m| m.keys().any(|k| k.starts_with(&prefix)))
        .unwrap_or(false)
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

    // Tier A: an older Context Drop /cd (SKILL.md only, no claim.sh) must be
    // upgraded in place WITHOUT force — otherwise users keep the old alias whose
    // model-driven claim could be skipped when /cd is given an instruction.
    #[test]
    fn short_alias_upgrades_own_older_alias_and_copies_claim_script() {
        let src = tempfile::tempdir().unwrap();
        fs::write(
            src.path().join("SKILL.md"),
            "---\nname: cd\ndescription: Short alias for /context-drop:pull. v2\n---\nnew",
        )
        .unwrap();
        fs::write(src.path().join("claim.sh"), "#!/bin/sh\n").unwrap();
        let cfg = tempfile::tempdir().unwrap();
        let skill_dir = cfg.path().join("skills").join("cd");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: cd\ndescription: Short alias for /context-drop:pull. v1\n---\nold",
        )
        .unwrap();

        let r = install_short_alias(cfg.path(), src.path(), false).unwrap();
        assert!(!r.skipped_existing);
        assert!(r.overwritten);
        assert!(fs::read_to_string(&r.skill_path).unwrap().ends_with("new"));
        assert!(skill_dir.join("claim.sh").is_file());
    }

    // Tier A: on Windows the claim must run claim.ps1 via `powershell -File`
    // (no `sh` without Git Bash) and allowed-tools must grant exactly that
    // command; POSIX content is left untouched. Runs on every OS against the
    // shipped SKILL.md so a wording change that breaks the rewrite is caught.
    #[test]
    fn shipped_cd_alias_adapts_for_windows() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../integrations/claude-code");
        let shipped = fs::read_to_string(root.join("alias/cd/SKILL.md")).unwrap();

        assert_eq!(adapt_skill_for_os(&shipped, false), shipped);

        let win = adapt_skill_for_os(&shipped, true);
        assert!(
            !win.contains(POSIX_ALLOWED_TOOLS),
            "POSIX allowed-tools left"
        );
        assert!(!win.contains(POSIX_INJECTION), "POSIX injection left");
        let inject = r#"!`powershell -NoProfile -ExecutionPolicy Bypass -File "${CLAUDE_SKILL_DIR}/claim.ps1" "${CLAUDE_SESSION_ID}"`"#;
        assert_eq!(win.matches(inject).count(), 1);
        assert!(win.contains(
            r#"  - Bash(powershell -NoProfile -ExecutionPolicy Bypass -File "${CLAUDE_SKILL_DIR}/claim.ps1" *)"#
        ));
        assert!(win.contains(
            r#"  - PowerShell(powershell -NoProfile -ExecutionPolicy Bypass -File "${CLAUDE_SKILL_DIR}/claim.ps1" *)"#
        ));
        // allowed-tools stays inside the frontmatter.
        let front_end = win.find("\n---").unwrap();
        assert!(win.find("allowed-tools:").unwrap() < front_end);
        // Idempotent.
        assert_eq!(adapt_skill_for_os(&win, true), win);
    }

    // Tier A: startup refresh upgrades only Context Drop's own /cd — never
    // installs one where absent, never touches a user's own /cd.
    #[test]
    fn refresh_short_alias_touches_only_our_installed_alias() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../integrations/claude-code");
        let src = root.join("alias/cd");

        let absent = tempfile::tempdir().unwrap();
        assert!(!refresh_short_alias(absent.path(), &src).unwrap());
        assert!(!absent.path().join("skills/cd").exists());

        let own = tempfile::tempdir().unwrap();
        let own_skill = own.path().join("skills/cd/SKILL.md");
        fs::create_dir_all(own_skill.parent().unwrap()).unwrap();
        fs::write(&own_skill, "my own cd").unwrap();
        assert!(!refresh_short_alias(own.path(), &src).unwrap());
        assert_eq!(fs::read_to_string(&own_skill).unwrap(), "my own cd");

        let ours = tempfile::tempdir().unwrap();
        let ours_skill = ours.path().join("skills/cd/SKILL.md");
        fs::create_dir_all(ours_skill.parent().unwrap()).unwrap();
        fs::write(
            &ours_skill,
            "description: Short alias for /context-drop:pull. old",
        )
        .unwrap();
        assert!(refresh_short_alias(ours.path(), &src).unwrap());
        assert!(fs::read_to_string(&ours_skill).unwrap().contains("claim"));
        assert!(ours.path().join("skills/cd/claim.ps1").is_file());
    }

    // Tier A: installing the shipped /cd writes the claim line for THIS OS
    // (claim.ps1 on Windows, claim.sh elsewhere) and ships both scripts.
    #[test]
    fn installed_shipped_alias_uses_this_os_claim_script() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../integrations/claude-code");
        let cfg = tempfile::tempdir().unwrap();
        let r = install_short_alias(cfg.path(), &root.join("alias/cd"), false).unwrap();
        let installed = fs::read_to_string(&r.skill_path).unwrap();
        let dir = r.skill_path.parent().unwrap();
        assert!(dir.join("claim.sh").is_file() && dir.join("claim.ps1").is_file());
        if cfg!(windows) {
            assert!(installed
                .contains(r#"-File "${CLAUDE_SKILL_DIR}/claim.ps1" "${CLAUDE_SESSION_ID}"`"#));
            assert!(!installed.contains(POSIX_INJECTION));
        } else {
            assert!(installed.contains(POSIX_INJECTION));
            assert!(installed.contains(POSIX_ALLOWED_TOOLS));
        }
    }

    // Tier A: the two entry points share the claim scripts byte-for-byte; /cd
    // claims at expansion (installer-adapted per OS), while the plugin's pull
    // (installable straight from GitHub, never adapted) must NOT inject a
    // POSIX-only command — that would stop the skill loading on Windows without
    // Git Bash — and instead makes the claim the model's first tool call.
    #[test]
    fn shipped_skills_claim_safely_on_every_os() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../integrations/claude-code");
        let read = |p: &str| fs::read_to_string(root.join(p)).unwrap();
        for script in ["claim.sh", "claim.ps1"] {
            assert_eq!(
                fs::read(root.join("alias/cd").join(script)).unwrap(),
                fs::read(root.join("skills/pull").join(script)).unwrap(),
                "{script} copies differ"
            );
        }
        // `sh` rejects CRLF scripts: .gitattributes must keep them LF everywhere.
        assert!(
            !read("skills/pull/claim.sh").contains('\r'),
            "claim.sh must be LF-only"
        );

        let cd = read("alias/cd/SKILL.md");
        assert_eq!(cd.matches(POSIX_INJECTION).count(), 1);
        assert!(cd.contains(POSIX_ALLOWED_TOOLS));

        let pull = read("skills/pull/SKILL.md");
        assert!(!pull.contains("!`"), "pull must not use loader injection");
        assert!(pull.contains("first tool call is always the claim"));
        assert!(pull.contains(r#"sh "${CLAUDE_SKILL_DIR}/claim.sh" "${CLAUDE_SESSION_ID}""#));
        assert!(pull.contains(r#"-File "${CLAUDE_SKILL_DIR}/claim.ps1" "${CLAUDE_SESSION_ID}""#));
        // Shared rules stay in both.
        for s in [&cd, &pull] {
            assert!(s.contains("MUST NOT read"));
            assert!(s.contains("always about the captured packet"));
            assert!(s.contains("PROCESSING_EXIT="));
            assert!(s.contains("consume <packetId> --claim-id <claimId>"));
        }
    }

    #[test]
    fn resolve_rejects_non_plugin_dir() {
        let empty = tempfile::tempdir().unwrap();
        assert!(resolve_plugin_source(Some(empty.path()), None).is_err());
    }
}
