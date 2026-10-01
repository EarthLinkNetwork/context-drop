//! CLI integration tests: run the built `context-drop` binary as a subprocess
//! with a controlled data directory and environment.

use std::path::{Path, PathBuf};
use std::process::Command;

use context_drop_core::{
    append_snapshot, create_draft, sha256_hex, CapturedItem, CapturedSnapshot, Db, ItemKind,
    Limits, Storage,
};

const BIN: &str = env!("CARGO_BIN_EXE_context-drop");

/// Seed a DRAFT packet with a text item whose content is `text`.
fn seed_packet(data_dir: &Path, text: &str) -> String {
    let storage = Storage::at(data_dir);
    storage.ensure_layout().unwrap();
    let mut db = Db::open(storage.db_path()).unwrap();
    let id = create_draft(&mut db, &storage).unwrap();
    let bytes = text.as_bytes().to_vec();
    let snap = CapturedSnapshot {
        snapshot_sha256: sha256_hex(&bytes),
        items: vec![CapturedItem {
            kind: ItemKind::Text,
            mime_type: "text/plain".into(),
            ext: "txt".into(),
            bytes,
        }],
    };
    append_snapshot(&mut db, &storage, &id, &snap, &Limits::default()).unwrap();
    id
}

/// Run the CLI with the given args, data dir, and optional session id.
fn run(data_dir: &Path, session: Option<&str>, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(BIN);
    cmd.arg("--data-dir").arg(data_dir);
    cmd.args(args);
    // Never touch the real ~/.local/bin: keep any CLI symlink inside the temp
    // data dir (install-claude installs the CLI + a PATH symlink).
    cmd.env("CONTEXT_DROP_LOCAL_BIN", data_dir.join("localbin"));
    // Start from a clean session env, then set only what the test wants.
    cmd.env_remove("CLAUDE_CODE_SESSION_ID");
    if let Some(s) = session {
        cmd.env("CLAUDE_CODE_SESSION_ID", s);
    }
    cmd.output().expect("failed to run context-drop")
}

#[test]
fn claim_json_is_parseable_and_metadata_only() {
    let dir = tempfile::tempdir().unwrap();
    let secret = "SUPER-SECRET-LOG-LINE-42";
    seed_packet(dir.path(), secret);

    let out = run(dir.path(), Some("sess-123"), &["claim", "--json"]);
    assert!(out.status.success(), "claim should succeed");
    let stdout = String::from_utf8_lossy(&out.stdout);

    // Parseable JSON.
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["itemCount"], serde_json::json!(1));
    assert_eq!(v["sessionId"], serde_json::json!("sess-123"));
    assert!(v["packetId"].is_string());
    assert!(v["manifestPath"].is_string());

    // Absolutely no raw content on stdout.
    assert!(
        !stdout.contains(secret),
        "claim stdout must not contain raw packet content"
    );
}

#[test]
fn no_packet_exits_with_code_3() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), Some("sess-1"), &["claim", "--json"]);
    assert_eq!(out.status.code(), Some(3));
    let v: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
    assert_eq!(v["ok"], serde_json::json!(false));
    assert_eq!(v["error"], serde_json::json!("NO_PACKET"));
}

#[test]
fn missing_session_exits_with_code_4_and_does_not_route() {
    let dir = tempfile::tempdir().unwrap();
    seed_packet(dir.path(), "some content");
    // No session id in env.
    let out = run(dir.path(), None, &["claim", "--json"]);
    assert_eq!(out.status.code(), Some(4));
    let v: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
    assert_eq!(v["error"], serde_json::json!("MISSING_SESSION_ID"));

    // The packet must NOT have been claimed (still claimable afterward).
    let ok = run(dir.path(), Some("sess-later"), &["claim", "--json"]);
    assert!(ok.status.success());
}

#[test]
fn status_and_list_json_are_parseable() {
    let dir = tempfile::tempdir().unwrap();
    seed_packet(dir.path(), "hello");

    let status = run(dir.path(), Some("s"), &["status", "--json"]);
    assert!(status.status.success());
    let sv: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&status.stdout).trim()).unwrap();
    assert_eq!(sv["ok"], serde_json::json!(true));
    assert!(sv["currentDraft"]["itemCount"] == serde_json::json!(1));

    let list = run(dir.path(), Some("s"), &["list", "--json"]);
    assert!(list.status.success());
    let lv: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&list.stdout).trim()).unwrap();
    assert!(lv.is_array());
    assert_eq!(lv.as_array().unwrap().len(), 1);
}

#[test]
fn git_and_non_git_project_detection_via_claim_metadata() {
    // Non-git: project root is the cwd we pass.
    let dir = tempfile::tempdir().unwrap();
    seed_packet(dir.path(), "x");
    let workdir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        Some("sess"),
        &["claim", "--json", "--cwd", workdir.path().to_str().unwrap()],
    );
    assert!(out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
    // Non-git => projectRoot == cwd.
    assert_eq!(v["projectRoot"], v["cwd"]);

    // Git repo: project root is the git top-level even from a subdirectory.
    let git_ok = Command::new("git")
        .arg("-C")
        .arg(workdir.path())
        .arg("init")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if git_ok {
        let sub = workdir.path().join("pkg").join("app");
        std::fs::create_dir_all(&sub).unwrap();
        let dir2 = tempfile::tempdir().unwrap();
        seed_packet(dir2.path(), "y");
        let out2 = run(
            dir2.path(),
            Some("sess"),
            &["claim", "--json", "--cwd", sub.to_str().unwrap()],
        );
        assert!(out2.status.success());
        let v2: serde_json::Value =
            serde_json::from_str(String::from_utf8_lossy(&out2.stdout).trim()).unwrap();
        // projectRoot is the git top-level (the workdir), not the subdirectory.
        let root = v2["projectRoot"].as_str().unwrap();
        let cwd = v2["cwd"].as_str().unwrap();
        assert_ne!(root, cwd, "monorepo subdir must resolve to the git root");
        assert!(cwd.contains("app"));
    }
}

#[test]
fn doctor_runs_and_reports() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), Some("s"), &["doctor"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Context Drop doctor"));
    assert!(stdout.contains("data dir:"));
    assert!(stdout.contains("Claude Code integrations:"));
}

#[test]
fn install_claude_lays_down_plugin_into_config_dir() {
    let data = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let plugin_src = repo_plugin_dir();
    if !plugin_src.join(".claude-plugin/plugin.json").is_file() {
        eprintln!(
            "plugin source not found at {}; skipping",
            plugin_src.display()
        );
        return;
    }
    let out = run(
        data.path(),
        Some("s"),
        &[
            "install-claude",
            "--config-dir",
            cfg.path().to_str().unwrap(),
            "--from",
            plugin_src.to_str().unwrap(),
            "--short-alias",
        ],
    );
    assert!(
        out.status.success(),
        "install-claude failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Plugin marketplace laid down.
    let installed = cfg
        .path()
        .join("plugins/marketplaces/context-drop/plugins/context-drop/.claude-plugin/plugin.json");
    assert!(installed.is_file(), "plugin.json should be installed");
    // Short alias installed.
    assert!(cfg.path().join("skills/cd/SKILL.md").is_file());
}

fn repo_plugin_dir() -> PathBuf {
    // crates/context-drop-cli -> repo root -> integrations/claude-code
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../integrations/claude-code")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("integrations/claude-code"))
}
