//! `context-drop` — the companion CLI.
//!
//! Coordinates with the desktop app purely through the shared SQLite database
//! and packet filesystem (no daemon, no network). The most important guarantee:
//! `claim --json` prints packet **metadata only** — never any raw captured
//! content. End users never need Node.js to run this binary.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use context_drop_core::clock::ms_to_rfc3339;
use context_drop_core::integration as install;
use context_drop_core::{
    claim, consume, detect_project, last_dispatch, list, mark_processing, release, status, undo,
    ClaimContext, CoreError, Db, Storage, DEFAULT_UNDO_WINDOW_MS,
};
use serde_json::json;

// Stable exit codes so scripts and the Skill can branch on them.
const EXIT_OK: u8 = 0;
const EXIT_ERROR: u8 = 1;
const EXIT_NO_PACKET: u8 = 3;
const EXIT_MISSING_SESSION: u8 = 4;
const EXIT_NOTHING_TO_UNDO: u8 = 5;

#[derive(Parser)]
#[command(
    name = "context-drop",
    version,
    about = "Context Drop companion CLI — route captured clipboard packets into a Claude Code session."
)]
struct Cli {
    /// Override the Context Drop data directory (also via CONTEXT_DROP_DATA_DIR).
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show a metadata-only status summary.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Claim the highest-priority packet for the current Claude Code session.
    Claim {
        #[arg(long)]
        json: bool,
        /// Session id (defaults to $CLAUDE_CODE_SESSION_ID).
        #[arg(long)]
        session_id: Option<String>,
        /// Working directory to route from (defaults to the process cwd).
        #[arg(long)]
        cwd: Option<PathBuf>,
    },
    /// Mark a claimed packet as PROCESSING (a subagent has started). Protects it
    /// from TTL cleanup while it is being investigated.
    Processing {
        packet_id: String,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        claim_id: Option<String>,
    },
    /// Mark a packet consumed (normal completion) for this session.
    Consume {
        packet_id: String,
        /// Session id (defaults to $CLAUDE_CODE_SESSION_ID).
        #[arg(long)]
        session_id: Option<String>,
        /// The claim id returned by `claim --json` (scopes consume to the exact
        /// claim, so a re-claim of the same packet is never consumed by mistake).
        #[arg(long)]
        claim_id: Option<String>,
    },
    /// Release a packet back to READY (routing undone; no code rollback).
    Release { packet_id: String },
    /// Undo the most recent eligible claim for this session.
    Undo {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        session_id: Option<String>,
    },
    /// List packets (metadata only).
    List {
        #[arg(long)]
        json: bool,
    },
    /// Diagnose the installation, data store, and Claude integrations.
    Doctor,
    /// Install the Claude Code integration into one or more config roots.
    InstallClaude {
        /// A specific config root (defaults to detected CLAUDE_CONFIG_DIR + ~/.claude).
        #[arg(long)]
        config_dir: Option<PathBuf>,
        /// Plugin source dir (defaults to bundled/relative or $CONTEXT_DROP_PLUGIN_DIR).
        #[arg(long)]
        from: Option<PathBuf>,
        /// Also install the optional `/cd` short-alias skill.
        #[arg(long)]
        short_alias: bool,
        /// Overwrite an existing `/cd` skill (short alias) if present.
        #[arg(long)]
        force: bool,
    },
    /// (internal, dev-tools feature) Append a text item to the current DRAFT
    /// packet for headless testing/scripting. Not present in release builds.
    #[cfg(feature = "dev-tools")]
    #[command(name = "__seed", hide = true)]
    Seed {
        #[arg(long)]
        text: Option<String>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let code = match run(&cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("context-drop: {e}");
            EXIT_ERROR
        }
    };
    ExitCode::from(code)
}

fn run(cli: &Cli) -> Result<u8, String> {
    match &cli.command {
        Command::Status { json } => cmd_status(cli, *json),
        Command::Claim {
            json,
            session_id,
            cwd,
        } => cmd_claim(cli, *json, session_id.clone(), cwd.clone()),
        Command::Processing {
            packet_id,
            session_id,
            claim_id,
        } => cmd_processing(cli, packet_id, session_id.clone(), claim_id.clone()),
        Command::Consume {
            packet_id,
            session_id,
            claim_id,
        } => cmd_consume(cli, packet_id, session_id.clone(), claim_id.clone()),
        Command::Release { packet_id } => cmd_release(cli, packet_id),
        Command::Undo { json, session_id } => cmd_undo(cli, *json, session_id.clone()),
        Command::List { json } => cmd_list(cli, *json),
        Command::Doctor => cmd_doctor(cli),
        Command::InstallClaude {
            config_dir,
            from,
            short_alias,
            force,
        } => cmd_install_claude(cli, config_dir.clone(), from.clone(), *short_alias, *force),
        #[cfg(feature = "dev-tools")]
        Command::Seed { text } => cmd_seed(cli, text.clone()),
    }
}

/// Internal (dev-tools): append a text item to the current DRAFT (creating one
/// if needed), applying the configured size limits and reporting the outcome.
#[cfg(feature = "dev-tools")]
fn cmd_seed(cli: &Cli, text: Option<String>) -> Result<u8, String> {
    use context_drop_core::{
        append_snapshot, create_draft, current_draft_id, snapshot_content_hash, AppendOutcome,
        CapturedItem, CapturedSnapshot, ItemKind,
    };
    use std::io::Read;

    let (mut db, storage) = open_store(cli)?;
    let limits = db.load_settings().map_err(|e| e.to_string())?.limits();

    // Bound stdin to the per-item limit so a runaway pipe cannot exhaust memory.
    let text = match text {
        Some(t) => t,
        None => {
            let cap = limits.max_item_bytes.max(0) as u64;
            let mut buf = String::new();
            std::io::stdin()
                .take(cap + 1)
                .read_to_string(&mut buf)
                .map_err(|e| e.to_string())?;
            buf
        }
    };
    if text.trim().is_empty() {
        return Err("no text to seed (pass --text or pipe stdin)".to_string());
    }

    let draft = match current_draft_id(&db).map_err(|e| e.to_string())? {
        Some(id) => id,
        None => create_draft(&mut db, &storage).map_err(|e| e.to_string())?,
    };
    let items = vec![CapturedItem {
        kind: ItemKind::Text,
        mime_type: "text/plain".into(),
        ext: "txt".into(),
        bytes: text.into_bytes(),
    }];
    let snapshot = CapturedSnapshot {
        snapshot_sha256: snapshot_content_hash(&items),
        items,
    };
    match append_snapshot(&mut db, &storage, &draft, &snapshot, &limits)
        .map_err(|e| e.to_string())?
    {
        AppendOutcome::Added { item_count, .. } => {
            println!("seeded draft {} ({item_count} item(s))", short(&draft));
            Ok(EXIT_OK)
        }
        other => Err(format!("seed not stored: {other:?}")),
    }
}

/// Resolve the storage root, create the layout, and open the database.
fn open_store(cli: &Cli) -> Result<(Db, Storage), String> {
    let storage = match &cli.data_dir {
        Some(dir) => Storage::at(dir),
        None => Storage::resolve().map_err(|e| e.to_string())?,
    };
    storage.ensure_layout().map_err(|e| e.to_string())?;
    let db = Db::open(storage.db_path()).map_err(|e| e.to_string())?;
    Ok((db, storage))
}

fn cmd_status(cli: &Cli, as_json: bool) -> Result<u8, String> {
    let (db, _storage) = open_store(cli)?;
    let report = status(&db).map_err(|e| e.to_string())?;
    if as_json {
        let out = json!({
            "ok": true,
            "currentDraft": report.current_draft.as_ref().map(|d| json!({
                "packetId": d.id,
                "itemCount": d.item_count,
                "state": d.state.as_str(),
            })),
            "readyCount": report.ready_count,
            "totalPackets": report.total_packets,
            "lastDispatch": report.last_dispatch.as_ref().map(|l| json!({
                "packetId": l.packet_id,
                "projectName": l.project_name,
                "itemCount": l.item_count,
                "claimedAt": ms_to_rfc3339(l.claimed_at_ms),
                "state": l.packet_state.as_str(),
            })),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        match &report.current_draft {
            Some(d) => println!("Capture: DRAFT {} — {} item(s)", short(&d.id), d.item_count),
            None => println!("Capture: (no active draft)"),
        }
        println!("Ready packets: {}", report.ready_count);
        println!("Total packets: {}", report.total_packets);
        match &report.last_dispatch {
            Some(l) => println!(
                "Last dispatch: {} — {} item(s) — {} — {}",
                l.project_name,
                l.item_count,
                l.packet_state.as_str(),
                ms_to_rfc3339(l.claimed_at_ms)
            ),
            None => println!("Last dispatch: (none)"),
        }
    }
    Ok(EXIT_OK)
}

fn cmd_claim(
    cli: &Cli,
    as_json: bool,
    session_override: Option<String>,
    cwd_override: Option<PathBuf>,
) -> Result<u8, String> {
    let (mut db, storage) = open_store(cli)?;

    // Routing identity: session id (env by default) + cwd + git project root.
    let session_id = session_override.or_else(|| std::env::var("CLAUDE_CODE_SESSION_ID").ok());
    let cwd = cwd_override
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let project = detect_project(&cwd);
    let config_dir = std::env::var("CLAUDE_CONFIG_DIR").ok();

    let ctx = ClaimContext {
        session_id,
        cwd: project.cwd.clone(),
        project_root: project.project_root.clone(),
        project_name: project.project_name.clone(),
        config_dir,
    };

    match claim(&mut db, &storage, &ctx) {
        Ok(res) => {
            // Metadata only — never raw content.
            let out = json!({
                "ok": true,
                "packetId": res.packet_id,
                "claimId": res.claim_id,
                "manifestPath": res.manifest_path,
                "itemCount": res.item_count,
                "sessionId": res.session_id,
                "cwd": res.cwd,
                "projectRoot": res.project_root,
                "projectName": res.project_name,
            });
            if as_json {
                println!("{}", serde_json::to_string(&out).unwrap());
            } else {
                println!(
                    "Claimed packet {} ({} item(s)) for {}",
                    short(&res.packet_id),
                    res.item_count,
                    res.project_name
                );
                println!("Manifest: {}", res.manifest_path);
            }
            Ok(EXIT_OK)
        }
        Err(CoreError::MissingSessionId) => {
            emit_error(
                as_json,
                "MISSING_SESSION_ID",
                "No Claude Code session id (CLAUDE_CODE_SESSION_ID) is available. \
                 Context Drop routes by session and refuses to route by project alone. \
                 Run this from inside a Claude Code session, or pass --session-id.",
            );
            Ok(EXIT_MISSING_SESSION)
        }
        Err(CoreError::NoPacket) => {
            emit_error(
                as_json,
                "NO_PACKET",
                "No Context Drop packet is ready. Start Capture and copy the materials first.",
            );
            Ok(EXIT_NO_PACKET)
        }
        Err(e) => {
            emit_error(as_json, "ERROR", &e.to_string());
            Ok(EXIT_ERROR)
        }
    }
}

fn cmd_processing(
    cli: &Cli,
    packet_id: &str,
    session_override: Option<String>,
    claim_id: Option<String>,
) -> Result<u8, String> {
    let (mut db, storage) = open_store(cli)?;
    let session_id = session_override
        .or_else(|| std::env::var("CLAUDE_CODE_SESSION_ID").ok())
        .filter(|s| !s.trim().is_empty());
    let Some(session_id) = session_id else {
        eprintln!("No Claude Code session id available; cannot mark processing.");
        return Ok(EXIT_MISSING_SESSION);
    };
    match mark_processing(
        &mut db,
        &storage,
        packet_id,
        &session_id,
        claim_id.as_deref(),
    ) {
        Ok(()) => {
            println!("Processing {}", short(packet_id));
            Ok(EXIT_OK)
        }
        Err(CoreError::ClaimNotOwned { .. }) => {
            eprintln!(
                "Packet {} is not claimed by this session; not marking processing.",
                short(packet_id)
            );
            Ok(EXIT_ERROR)
        }
        Err(e) => Err(e.to_string()),
    }
}

fn cmd_consume(
    cli: &Cli,
    packet_id: &str,
    session_override: Option<String>,
    claim_id: Option<String>,
) -> Result<u8, String> {
    let (mut db, storage) = open_store(cli)?;
    let session_id = session_override
        .or_else(|| std::env::var("CLAUDE_CODE_SESSION_ID").ok())
        .filter(|s| !s.trim().is_empty());
    let Some(session_id) = session_id else {
        eprintln!(
            "No Claude Code session id (CLAUDE_CODE_SESSION_ID) available; \
             consume is scoped to the claiming session."
        );
        return Ok(EXIT_MISSING_SESSION);
    };
    match consume(
        &mut db,
        &storage,
        packet_id,
        &session_id,
        claim_id.as_deref(),
    ) {
        Ok(()) => {
            println!("Consumed {}", short(packet_id));
            Ok(EXIT_OK)
        }
        Err(CoreError::ClaimNotOwned { .. }) => {
            eprintln!(
                "Packet {} is no longer claimed by this session (it was released and re-claimed); \
                 not consuming.",
                short(packet_id)
            );
            Ok(EXIT_ERROR)
        }
        Err(e) => Err(e.to_string()),
    }
}

fn cmd_release(cli: &Cli, packet_id: &str) -> Result<u8, String> {
    let (mut db, storage) = open_store(cli)?;
    release(&mut db, &storage, packet_id).map_err(|e| e.to_string())?;
    println!("Released {} back to READY", short(packet_id));
    Ok(EXIT_OK)
}

fn cmd_undo(cli: &Cli, as_json: bool, session_override: Option<String>) -> Result<u8, String> {
    let (mut db, storage) = open_store(cli)?;
    let session_id = session_override.or_else(|| std::env::var("CLAUDE_CODE_SESSION_ID").ok());
    let Some(session_id) = session_id.filter(|s| !s.trim().is_empty()) else {
        emit_error(
            as_json,
            "MISSING_SESSION_ID",
            "No Claude Code session id available; cannot scope undo to this session.",
        );
        return Ok(EXIT_MISSING_SESSION);
    };
    match undo(&mut db, &storage, &session_id, DEFAULT_UNDO_WINDOW_MS) {
        Ok(res) => {
            if as_json {
                let out = json!({
                    "ok": true,
                    "packetId": res.packet_id,
                    "projectName": res.project_name,
                    "itemCount": res.item_count,
                });
                println!("{}", serde_json::to_string(&out).unwrap());
            } else {
                println!(
                    "Undid dispatch of packet {} ({} item(s)); returned to READY. \
                     Note: this undoes routing only, not any code changes already made.",
                    short(&res.packet_id),
                    res.item_count
                );
            }
            Ok(EXIT_OK)
        }
        Err(CoreError::NothingToUndo) => {
            emit_error(
                as_json,
                "NOTHING_TO_UNDO",
                "No recent claim for this session is eligible to undo.",
            );
            Ok(EXIT_NOTHING_TO_UNDO)
        }
        Err(e) => {
            emit_error(as_json, "ERROR", &e.to_string());
            Ok(EXIT_ERROR)
        }
    }
}

fn cmd_list(cli: &Cli, as_json: bool) -> Result<u8, String> {
    let (db, _storage) = open_store(cli)?;
    let packets = list(&db).map_err(|e| e.to_string())?;
    if as_json {
        let arr: Vec<_> = packets
            .iter()
            .map(|p| {
                json!({
                    "packetId": p.id,
                    "state": p.state.as_str(),
                    "itemCount": p.item_count,
                    "totalBytes": p.total_bytes,
                    "createdAt": ms_to_rfc3339(p.created_at_ms),
                    "updatedAt": ms_to_rfc3339(p.updated_at_ms),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json!(arr)).unwrap());
    } else if packets.is_empty() {
        println!("(no packets)");
    } else {
        for p in &packets {
            println!(
                "{}  {:<10} {:>3} item(s)  {:>10} B  {}",
                short(&p.id),
                p.state.as_str(),
                p.item_count,
                p.total_bytes,
                ms_to_rfc3339(p.created_at_ms)
            );
        }
    }
    Ok(EXIT_OK)
}

fn cmd_doctor(cli: &Cli) -> Result<u8, String> {
    println!("Context Drop doctor");
    println!("===================");

    // Data store.
    let storage = match &cli.data_dir {
        Some(dir) => Storage::at(dir),
        None => match Storage::resolve() {
            Ok(s) => s,
            Err(e) => {
                println!("data dir:         UNRESOLVED ({e})");
                return Ok(EXIT_OK);
            }
        },
    };
    let root = storage.root();
    println!("data dir:         {}", root.display());
    println!("  exists:         {}", root.exists());
    println!("  db path:        {}", storage.db_path().display());
    match Db::open(storage.db_path()) {
        Ok(db) => match db.schema_version() {
            Ok(v) => println!("  db schema:      v{v} (ok)"),
            Err(e) => println!("  db schema:      ERROR ({e})"),
        },
        Err(e) => println!("  db open:        ERROR ({e})"),
    }
    #[cfg(unix)]
    if root.exists() {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(md) = std::fs::metadata(root) {
            println!("  perms:          {:o}", md.permissions().mode() & 0o777);
        }
    }

    // Last dispatch (metadata only).
    if let Ok((db, _)) = open_store(cli) {
        if let Ok(Some(l)) = last_dispatch(&db) {
            println!(
                "last dispatch:    {} — {} item(s) — {}",
                l.project_name,
                l.item_count,
                l.packet_state.as_str()
            );
        }
    }

    // Session id availability.
    match std::env::var("CLAUDE_CODE_SESSION_ID") {
        Ok(s) if !s.is_empty() => println!("session id:       present"),
        _ => println!("session id:       NOT set (claims from here will be refused)"),
    }

    // Claude config dirs + integration status.
    println!("Claude Code integrations:");
    let dirs = install::detect_config_dirs();
    if dirs.is_empty() {
        println!("  (no config dirs detected)");
    }
    for dir in dirs {
        let status = if install::is_plugin_installed(&dir) {
            "Installed"
        } else {
            "Not installed"
        };
        println!("  {:<40} {}", dir.display(), status);
    }
    Ok(EXIT_OK)
}

fn cmd_install_claude(
    cli: &Cli,
    config_dir: Option<PathBuf>,
    from: Option<PathBuf>,
    short_alias: bool,
    force: bool,
) -> Result<u8, String> {
    let targets: Vec<PathBuf> = match config_dir {
        Some(d) => vec![d],
        None => install::detect_config_dirs(),
    };
    if targets.is_empty() {
        return Err("no Claude config directory found; pass --config-dir <path>".to_string());
    }

    // Resolve the effective data dir first, then the plugin source (so the
    // staged-plugin fallback honors --data-dir), then install the CLI + stage
    // the plugin so the skills can always find `context-drop`.
    let storage = match &cli.data_dir {
        Some(dir) => Storage::at(dir),
        None => Storage::resolve().map_err(|e| e.to_string())?,
    };
    storage.ensure_layout().map_err(|e| e.to_string())?;
    let data_dir = storage.root();
    let plugin_src = install::resolve_plugin_source(from.as_deref(), Some(data_dir))?;
    let exe = std::env::current_exe().map_err(|e| format!("cannot locate this executable: {e}"))?;
    // The canonical CLI copy and plugin staging are essential — fail the install
    // if they do not succeed (PATH symlink stays best-effort inside install_cli).
    let report = install::install_cli(data_dir, &exe)?;
    println!("Installed CLI at {}", report.installed_path.display());
    if let Some(link) = &report.symlinked {
        println!("  symlinked: {}", link.display());
    }
    if let Some(hint) = &report.path_hint {
        println!("  {hint}");
    }
    install::stage_plugin(data_dir, &plugin_src)?;

    let mut alias_failed = false;
    for dir in &targets {
        let report = install::install_plugin(dir, &plugin_src)?;
        let verb = if report.already_present {
            "Refreshed"
        } else {
            "Installed"
        };
        println!(
            "{verb} Context Drop plugin under {}",
            report.config_dir.display()
        );
        println!("  marketplace: {}", report.marketplace_dir.display());
        println!("  Enable it in a Claude Code session started with this config dir:");
        println!(
            "    /plugin marketplace add {}",
            report.marketplace_dir.display()
        );
        println!("    /plugin install context-drop@context-drop");

        if short_alias {
            let alias_src = plugin_src.join("alias").join("cd");
            match install::install_short_alias(dir, &alias_src, force) {
                Ok(r) if r.skipped_existing => println!(
                    "  /cd short alias: SKIPPED — a /cd skill already exists at {} (use --force to overwrite)",
                    r.skill_path.display()
                ),
                Ok(r) => println!(
                    "  /cd short alias: {} at {}",
                    if r.overwritten { "overwritten" } else { "installed" },
                    r.skill_path.display()
                ),
                Err(e) => {
                    // A requested alias install that failed must not report overall success.
                    eprintln!("  /cd short alias: ERROR ({e})");
                    alias_failed = true;
                }
            }
        }
    }
    println!(
        "\nThe Context Drop data store is global per OS user and is shared by all \
         config dirs; nothing packet-related was duplicated."
    );
    if alias_failed {
        return Ok(EXIT_ERROR);
    }
    Ok(EXIT_OK)
}

/// Print a machine-readable error (JSON to stdout, or a message to stderr).
fn emit_error(as_json: bool, code: &str, message: &str) {
    if as_json {
        let out = json!({ "ok": false, "error": code, "message": message });
        println!("{}", serde_json::to_string(&out).unwrap());
    } else {
        eprintln!("{message}");
    }
}

/// A short id prefix for human-readable output.
fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}
