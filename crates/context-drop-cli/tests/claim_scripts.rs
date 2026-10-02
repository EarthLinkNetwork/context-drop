//! Run the skill's bundled claim scripts (`claim.sh` / `claim.ps1`) against the
//! built CLI, the way the Claude Code skill loader runs them when `/cd` or
//! `/context-drop:pull` expands. The binary is resolved the production way:
//! from `$CONTEXT_DROP_DATA_DIR/bin/` (no `$CONTEXT_DROP_BIN`).

use std::path::{Path, PathBuf};
use std::process::Command;

use context_drop_core::{
    append_snapshot, create_draft, packet_summary, sha256_hex, CapturedItem, CapturedSnapshot, Db,
    ItemKind, Limits, PacketState, Storage,
};

const BIN: &str = env!("CARGO_BIN_EXE_context-drop");

fn skill_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../integrations/claude-code/skills/pull")
}

/// A data dir holding one DRAFT packet and a copy of the CLI under `bin/`. The
/// path is non-ASCII so the claim JSON carries it: on a non-UTF-8 Windows code
/// page this catches claim.ps1 decoding the CLI output in the wrong encoding.
fn setup() -> (tempfile::TempDir, PathBuf, String) {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("データ漢字");
    let storage = Storage::at(&data);
    storage.ensure_layout().unwrap();
    let mut db = Db::open(storage.db_path()).unwrap();
    let id = create_draft(&mut db, &storage).unwrap();
    let bytes = br#"C:\Users\x "quoted" log line"#.to_vec();
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

    let bin_dir = data.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let name = if cfg!(windows) {
        "context-drop.exe"
    } else {
        "context-drop"
    };
    std::fs::copy(BIN, bin_dir.join(name)).unwrap();
    (tmp, data, id)
}

fn run_script(mut cmd: Command, data_dir: &Path) -> String {
    cmd.env("CONTEXT_DROP_DATA_DIR", data_dir)
        .env_remove("CONTEXT_DROP_BIN")
        .env_remove("CLAUDE_CODE_SESSION_ID");
    let out = cmd.output().expect("failed to run claim script");
    assert!(out.status.success(), "script must always exit 0: {out:?}");
    String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n")
}

/// The script claimed the packet for `session`, marked it PROCESSING, and
/// printed metadata-only output with the resolved binary.
fn assert_claimed(stdout: &str, data_dir: &Path, packet_id: &str, session: &str) {
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines[0].starts_with("CONTEXT_DROP_BIN=") && lines[0].contains("context-drop"),
        "{stdout}"
    );
    assert_eq!(lines[1], "CLAIM_EXIT=0", "{stdout}");
    let json: serde_json::Value = serde_json::from_str(lines[2]).expect("claim JSON intact");
    assert_eq!(json["packetId"], packet_id);
    assert_eq!(json["sessionId"], session);
    assert!(!stdout.contains("quoted"), "packet content must not leak");
    assert_eq!(lines[3], "PROCESSING_EXIT=0", "{stdout}");

    let db = Db::open(Storage::at(data_dir).db_path()).unwrap();
    let summary = packet_summary(&db, packet_id).unwrap().unwrap();
    assert_eq!(summary.state, PacketState::Processing);
}

fn has(program: &str) -> bool {
    Command::new(program)
        .args(["-NoProfile", "-Command", "exit 0"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// Tier A: the POSIX script the skill loader runs on macOS/Linux claims and
// marks PROCESSING (fails if resolution, parsing, or the processing call break).
#[cfg(unix)]
#[test]
fn claim_sh_claims_and_marks_processing() {
    let (_tmp, data, id) = setup();
    let mut cmd = Command::new("sh");
    cmd.arg(skill_dir().join("claim.sh")).arg("sess-sh");
    let stdout = run_script(cmd, &data);
    assert_claimed(&stdout, &data, &id, "sess-sh");
}

// Tier A: the Windows script, invoked exactly as the installer writes it into
// SKILL.md (`powershell -NoProfile -ExecutionPolicy Bypass -File`). Runs with
// Windows PowerShell on Windows, and with pwsh wherever it is installed.
#[test]
fn claim_ps1_claims_and_marks_processing() {
    let shells: Vec<&str> = ["powershell", "pwsh"]
        .into_iter()
        .filter(|s| has(s))
        .collect();
    if cfg!(windows) {
        assert!(
            shells.contains(&"powershell"),
            "Windows PowerShell must exist"
        );
    }
    for shell in shells {
        let (_tmp, data, id) = setup();
        let mut cmd = Command::new(shell);
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(skill_dir().join("claim.ps1"))
            .arg("sess-ps1");
        let stdout = run_script(cmd, &data);
        assert_claimed(&stdout, &data, &id, "sess-ps1");

        // A second run finds nothing to claim and reports NO_PACKET (exit 3).
        let mut again = Command::new(shell);
        again
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(skill_dir().join("claim.ps1"))
            .arg("sess-ps1");
        let stdout = run_script(again, &data);
        assert!(stdout.contains("CLAIM_EXIT=3"), "{shell}: {stdout}");
        assert!(stdout.contains("NO_PACKET"), "{shell}: {stdout}");
    }
}
