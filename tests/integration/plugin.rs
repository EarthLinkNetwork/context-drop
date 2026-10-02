//! Claude Code integration validation (spec §43): plugin metadata validation,
//! skill discovery, agent discovery, and the file-level "raw context stays out
//! of the main conversation" guarantee.
//!
//! Behavioral guarantees (ANALYZE does not edit, FIX may edit, compact return,
//! raw packet not injected into the main prompt) are enforced by the SKILL/agent
//! instructions themselves; here we assert those instructions are present.

use std::fs;
use std::path::PathBuf;

fn plugin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../integrations/claude-code")
}

fn read(rel: &str) -> String {
    fs::read_to_string(plugin_dir().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

fn has_frontmatter(body: &str) -> bool {
    body.starts_with("---") && body.matches("---").count() >= 2
}

#[test]
fn plugin_metadata_is_valid() {
    let raw = read(".claude-plugin/plugin.json");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("plugin.json is valid JSON");
    assert_eq!(v["name"], serde_json::json!("context-drop"));
    assert!(v["version"].is_string(), "plugin.json has a version");
    assert!(
        v["description"].as_str().unwrap_or("").len() > 10,
        "plugin.json has a meaningful description"
    );
}

#[test]
fn skills_are_discoverable() {
    for (rel, name) in [
        ("skills/pull/SKILL.md", "pull"),
        ("skills/undo/SKILL.md", "undo"),
        ("skills/status/SKILL.md", "status"),
    ] {
        let body = read(rel);
        assert!(has_frontmatter(&body), "{rel} has YAML frontmatter");
        assert!(
            body.contains(&format!("name: {name}")),
            "{rel} declares name: {name}"
        );
        assert!(
            body.contains("description:"),
            "{rel} declares a description"
        );
    }
}

#[test]
fn short_alias_skill_is_discoverable() {
    let body = read("alias/cd/SKILL.md");
    assert!(has_frontmatter(&body));
    assert!(body.contains("name: cd"));
}

#[test]
fn agent_is_discoverable_with_tools() {
    let body = read("agents/context-investigator.md");
    assert!(has_frontmatter(&body));
    assert!(body.contains("name: context-investigator"));
    assert!(body.contains("tools:"), "agent declares tools");
    // Task modes and compact-result contract are present.
    for token in ["ANALYZE", "FIX", "REVIEW", "Confidence", "CONFIRMED"] {
        assert!(body.contains(token), "agent instructions mention {token}");
    }
}

#[test]
fn pull_skill_forbids_reading_raw_content_in_main_agent() {
    let body = read("skills/pull/SKILL.md");
    // The core guarantee: the main agent must not read raw packet material.
    assert!(
        body.contains("MUST NOT read"),
        "pull skill states the main agent must not read raw content"
    );
    // The claim runs deterministically at skill expansion via the bundled
    // claim.sh, which calls the CLI's `claim --json`.
    assert!(
        body.contains("!`sh \"${CLAUDE_SKILL_DIR}/claim.sh\""),
        "pull skill claims at expansion via the bundled claim.sh"
    );
    assert!(
        read("skills/pull/claim.sh").contains("claim --json"),
        "claim.sh claims via the CLI"
    );
    assert!(
        body.contains("context-investigator"),
        "pull skill delegates to the isolated subagent"
    );
    // Metadata-only into the main context.
    assert!(body.to_lowercase().contains("metadata"));
}
