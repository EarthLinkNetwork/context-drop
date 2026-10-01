//! Context Drop core domain.
//!
//! This crate owns the packet model, the SQLite metadata authority, claim
//! routing, on-disk storage, and TTL cleanup. It contains **no** platform or
//! GUI code — clipboard adapters and the Tauri app depend on this crate, never
//! the other way around.
//!
//! The most important guarantees implemented here:
//! - Atomic, session-centric claim (exactly one session can claim a packet).
//! - Append is refused once a packet leaves DRAFT (append-vs-claim race).
//! - The claim result carries metadata only — never raw captured content.

pub mod claim;
pub mod cleanup;
pub mod clock;
pub mod database;
pub mod error;
pub mod hash;
pub mod id;
pub mod integration;
pub mod manifest;
pub mod packet;
pub mod project;
pub mod settings;
pub mod storage;

// Curated top-level re-exports for ergonomic use by the CLI and desktop app.
pub use claim::{
    append_snapshot, claim, clear_packet, consume, create_draft, current_draft_id, delete_item,
    finalize_capture, get_item, last_dispatch, list, mark_processing, packet_summary, recent_items,
    refresh_manifest, release, snapshot_content_hash, status, undo, undo_last_dispatch,
    AppendOutcome, CapturedItem, CapturedSnapshot, ClaimContext, ClaimResult, LastDispatch,
    PacketSummary, StatusReport, UndoResult,
};
pub use cleanup::{cleanup, is_eligible_for_cleanup, CleanupReport};
pub use database::{Db, SCHEMA_VERSION};
pub use error::{CoreError, Result};
pub use hash::sha256_hex;
pub use manifest::{ClaimMeta, Manifest, ManifestItem, MANIFEST_SCHEMA_VERSION};
pub use packet::{ItemKind, Packet, PacketItem, PacketState};
pub use project::{detect as detect_project, ProjectInfo};
pub use settings::{Limits, Settings};
pub use storage::{data_dir, Storage, APP_IDENTIFIER, DATA_DIR_ENV};

/// The recommended undo window: 5 minutes, per spec.
pub const DEFAULT_UNDO_WINDOW_MS: i64 = 5 * 60 * 1000;
