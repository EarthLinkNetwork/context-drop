//! Error types for the Context Drop core domain.

use crate::packet::PacketState;

/// Result alias used throughout the core crate.
pub type Result<T> = std::result::Result<T, CoreError>;

/// All fallible core operations surface one of these variants.
///
/// Variants are deliberately concrete so callers (the CLI, the desktop app)
/// can map them to stable, machine-readable error codes without inspecting
/// message strings.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),

    /// No DRAFT-with-items and no READY packet exists to claim.
    #[error("no packet is ready to claim")]
    NoPacket,

    /// A claim was attempted without a Claude Code session id. Routing must be
    /// session-centric, so we refuse to fall back to project-only routing.
    #[error("no Claude Code session id available; refusing to route by project alone")]
    MissingSessionId,

    /// The packet is not in a state that permits the requested transition.
    #[error("invalid state transition from {from} to {to}")]
    InvalidTransition { from: PacketState, to: PacketState },

    /// A concurrent operation changed the packet's state out from under us.
    #[error("packet state changed: expected {expected}, found {actual}")]
    StateChanged {
        expected: PacketState,
        actual: PacketState,
    },

    /// The referenced packet does not exist.
    #[error("packet not found: {0}")]
    PacketNotFound(String),

    /// Nothing eligible could be undone for the current session.
    #[error("no recent claim eligible for undo in this session")]
    NothingToUndo,

    /// The packet's active claim belongs to a different session (it was released
    /// and re-claimed). A session must never consume another session's claim.
    #[error("packet {packet_id} is no longer claimed by this session")]
    ClaimNotOwned { packet_id: String },

    /// The application data directory could not be resolved.
    #[error("could not resolve application data directory")]
    NoDataDir,

    /// Internal invariant violated (should never happen; indicates a bug).
    #[error("internal error: {0}")]
    Internal(String),
}
