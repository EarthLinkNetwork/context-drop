//! The `manifest.json` projection of a packet.
//!
//! SQLite is the authority for packet metadata; the manifest is a
//! self-contained, human- and agent-readable projection written next to the
//! packet's items. Raw item contents are never inlined here — only references.

use serde::{Deserialize, Serialize};

use crate::clock::ms_to_rfc3339;
use crate::packet::{Packet, PacketItem};

/// Bumped whenever the manifest shape changes in a backward-incompatible way.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// The full manifest document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub id: String,
    pub state: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    pub items: Vec<ManifestItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim: Option<ClaimMeta>,
}

/// One item entry in the manifest. `relativePath` points to a file under the
/// packet's `items/` directory; contents are never inlined.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestItem {
    pub id: String,
    pub kind: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    #[serde(rename = "relativePath")]
    pub relative_path: String,
    #[serde(rename = "byteSize")]
    pub byte_size: i64,
    pub sha256: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

/// Routing metadata recorded when a packet is claimed by a session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClaimMeta {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub cwd: String,
    #[serde(rename = "projectRoot")]
    pub project_root: String,
    #[serde(rename = "projectName")]
    pub project_name: String,
    /// The `CLAUDE_CONFIG_DIR` in effect at claim time, when known. Recorded for
    /// diagnostics only — it is never used as a routing key.
    #[serde(rename = "configDir", skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<String>,
    #[serde(rename = "claimedAt")]
    pub claimed_at: String,
}

impl Manifest {
    /// Build a manifest projection from database rows.
    pub fn from_rows(packet: &Packet, items: &[PacketItem], claim: Option<ClaimMeta>) -> Manifest {
        Manifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            id: packet.id.clone(),
            state: packet.state.as_str().to_string(),
            created_at: ms_to_rfc3339(packet.created_at_ms),
            updated_at: ms_to_rfc3339(packet.updated_at_ms),
            items: items
                .iter()
                .map(|it| ManifestItem {
                    id: it.id.clone(),
                    kind: it.kind.as_str().to_string(),
                    mime_type: it.mime_type.clone(),
                    relative_path: it.relative_path.clone(),
                    byte_size: it.byte_size,
                    sha256: it.sha256.clone(),
                    created_at: ms_to_rfc3339(it.created_at_ms),
                })
                .collect(),
            claim,
        }
    }

    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::{ItemKind, PacketState};

    fn sample_packet() -> Packet {
        Packet {
            id: "pkt-1".into(),
            state: PacketState::Claimed,
            created_at_ms: 1_609_459_200_000,
            updated_at_ms: 1_609_459_260_000,
            last_snapshot_sha256: Some("abc".into()),
            total_bytes: 12,
        }
    }

    fn sample_item() -> PacketItem {
        PacketItem {
            id: "item-1".into(),
            packet_id: "pkt-1".into(),
            seq: 1,
            kind: ItemKind::Json,
            mime_type: "application/json".into(),
            relative_path: "items/0001.json".into(),
            byte_size: 12,
            sha256: "deadbeef".into(),
            created_at_ms: 1_609_459_205_000,
        }
    }

    #[test]
    fn serializes_with_schema_version_and_camel_case() {
        let claim = ClaimMeta {
            session_id: "sess-1".into(),
            cwd: "/tmp/x".into(),
            project_root: "/tmp/x".into(),
            project_name: "x".into(),
            config_dir: Some("/home/u/.claude".into()),
            claimed_at: ms_to_rfc3339(1_609_459_260_000),
        };
        let m = Manifest::from_rows(&sample_packet(), &[sample_item()], Some(claim));
        let json = m.to_json_pretty().unwrap();
        assert!(json.contains("\"schemaVersion\": 1"));
        assert!(json.contains("\"relativePath\": \"items/0001.json\""));
        assert!(json.contains("\"sessionId\": \"sess-1\""));
        assert!(json.contains("\"state\": \"CLAIMED\""));
        // Round-trips.
        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn claim_is_omitted_when_absent() {
        let m = Manifest::from_rows(&sample_packet(), &[sample_item()], None);
        let json = m.to_json_pretty().unwrap();
        assert!(!json.contains("\"claim\""));
    }

    #[test]
    fn raw_content_is_never_inlined() {
        // The manifest carries only references and hashes, never bytes/text.
        let m = Manifest::from_rows(&sample_packet(), &[sample_item()], None);
        let json = m.to_json_pretty().unwrap();
        assert!(json.contains("items/0001.json"));
        assert!(json.contains("deadbeef")); // hash, not content
    }
}
