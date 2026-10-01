//! Packet domain types and the packet state machine.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The lifecycle state of a packet.
///
/// ```text
/// DRAFT ─┬─▶ READY ─▶ CLAIMED ─▶ PROCESSING ─▶ CONSUMED
///        └────────────▲
/// ```
/// with failure/recovery edges (release/undo back to READY, FAILED, EXPIRED).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PacketState {
    /// Capture is in progress; clipboard items are being appended.
    Draft,
    /// Capture was explicitly stopped but the packet has not been handed to a session yet.
    Ready,
    /// Bound to a specific Claude Code session.
    Claimed,
    /// A subagent has started consuming the packet.
    Processing,
    /// Successfully consumed.
    Consumed,
    /// A failure occurred during processing.
    Failed,
    /// Expired by TTL cleanup.
    Expired,
}

impl PacketState {
    /// The stable uppercase token stored in SQLite and manifests.
    pub fn as_str(self) -> &'static str {
        match self {
            PacketState::Draft => "DRAFT",
            PacketState::Ready => "READY",
            PacketState::Claimed => "CLAIMED",
            PacketState::Processing => "PROCESSING",
            PacketState::Consumed => "CONSUMED",
            PacketState::Failed => "FAILED",
            PacketState::Expired => "EXPIRED",
        }
    }

    /// Parse a stored token back into a state.
    pub fn parse(s: &str) -> Option<PacketState> {
        Some(match s {
            "DRAFT" => PacketState::Draft,
            "READY" => PacketState::Ready,
            "CLAIMED" => PacketState::Claimed,
            "PROCESSING" => PacketState::Processing,
            "CONSUMED" => PacketState::Consumed,
            "FAILED" => PacketState::Failed,
            "EXPIRED" => PacketState::Expired,
            _ => return None,
        })
    }

    /// Whether a transition `self -> to` is permitted.
    ///
    /// Main path transitions come straight from the spec. Failure/recovery
    /// edges are kept to the minimum needed to implement release/undo, subagent
    /// failure, and TTL expiry — nothing speculative.
    pub fn can_transition_to(self, to: PacketState) -> bool {
        use PacketState::*;
        matches!(
            (self, to),
            // Main path.
            (Draft, Ready)
                | (Draft, Claimed)
                | (Ready, Claimed)
                | (Claimed, Processing)
                | (Processing, Consumed)
                // Release / undo: a claim can be handed back to READY.
                | (Claimed, Ready)
                | (Processing, Ready)
                // Failure.
                | (Claimed, Failed)
                | (Processing, Failed)
                // Retry a failed packet.
                | (Failed, Ready)
                // TTL expiry (cleanup marks stale non-active packets EXPIRED).
                | (Ready, Expired)
                | (Claimed, Expired)
                | (Failed, Expired)
                | (Consumed, Expired)
        )
    }

    /// Terminal states never transition again on their own.
    pub fn is_terminal(self) -> bool {
        matches!(self, PacketState::Consumed | PacketState::Expired)
    }
}

impl fmt::Display for PacketState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The classified kind of a captured clipboard item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Text,
    Json,
    Html,
    Url,
    Image,
    File,
    Unknown,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Text => "text",
            ItemKind::Json => "json",
            ItemKind::Html => "html",
            ItemKind::Url => "url",
            ItemKind::Image => "image",
            ItemKind::File => "file",
            ItemKind::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> ItemKind {
        match s {
            "text" => ItemKind::Text,
            "json" => ItemKind::Json,
            "html" => ItemKind::Html,
            "url" => ItemKind::Url,
            "image" => ItemKind::Image,
            "file" => ItemKind::File,
            _ => ItemKind::Unknown,
        }
    }
}

/// A packet row as stored in SQLite (the metadata authority).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub id: String,
    pub state: PacketState,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    /// Hash of the most recent captured snapshot, used for consecutive dedupe.
    pub last_snapshot_sha256: Option<String>,
    /// Sum of item byte sizes (kept in the row so packet-size limits are O(1)).
    pub total_bytes: i64,
}

/// A packet item row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketItem {
    pub id: String,
    pub packet_id: String,
    pub seq: i64,
    pub kind: ItemKind,
    pub mime_type: String,
    pub relative_path: String,
    pub byte_size: i64,
    pub sha256: String,
    pub created_at_ms: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrips_through_string() {
        for s in [
            PacketState::Draft,
            PacketState::Ready,
            PacketState::Claimed,
            PacketState::Processing,
            PacketState::Consumed,
            PacketState::Failed,
            PacketState::Expired,
        ] {
            assert_eq!(PacketState::parse(s.as_str()), Some(s));
        }
        assert_eq!(PacketState::parse("NONSENSE"), None);
    }

    #[test]
    fn main_path_transitions_are_allowed() {
        use PacketState::*;
        assert!(Draft.can_transition_to(Ready));
        assert!(Draft.can_transition_to(Claimed));
        assert!(Ready.can_transition_to(Claimed));
        assert!(Claimed.can_transition_to(Processing));
        assert!(Processing.can_transition_to(Consumed));
    }

    #[test]
    fn illegal_transitions_are_rejected() {
        use PacketState::*;
        assert!(!Draft.can_transition_to(Processing));
        assert!(!Draft.can_transition_to(Consumed));
        assert!(!Ready.can_transition_to(Processing));
        assert!(!Consumed.can_transition_to(Ready));
        assert!(!Consumed.can_transition_to(Processing));
        assert!(!Draft.can_transition_to(Draft));
    }

    #[test]
    fn recovery_transitions_are_allowed() {
        use PacketState::*;
        assert!(Claimed.can_transition_to(Ready)); // undo / release
        assert!(Processing.can_transition_to(Ready)); // release
        assert!(Processing.can_transition_to(Failed));
        assert!(Ready.can_transition_to(Expired)); // ttl
    }

    #[test]
    fn terminal_states() {
        assert!(PacketState::Consumed.is_terminal());
        assert!(PacketState::Expired.is_terminal());
        assert!(!PacketState::Draft.is_terminal());
    }
}
