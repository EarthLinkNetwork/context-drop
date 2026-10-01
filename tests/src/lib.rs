//! Shared helpers for the Context Drop integration tests.

use std::path::PathBuf;

use context_drop_core::{
    append_snapshot, create_draft, CapturedItem, CapturedSnapshot, Db, ItemKind, Limits, Storage,
};

/// A test harness: an isolated data directory with an open database.
pub struct Harness {
    pub dir: tempfile::TempDir,
    pub db_path: PathBuf,
    pub storage: Storage,
}

impl Harness {
    /// Create an isolated harness rooted in a fresh temp directory.
    pub fn new() -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::at(dir.path());
        storage.ensure_layout().unwrap();
        let db_path = storage.db_path();
        // Initialize schema once.
        let _ = Db::open(&db_path).unwrap();
        Harness {
            db_path,
            storage,
            dir,
        }
    }

    /// Open a fresh database connection to this harness's file. Each connection
    /// is independent, which is exactly how concurrent CLI invocations behave.
    pub fn open_db(&self) -> Db {
        Db::open(&self.db_path).unwrap()
    }

    pub fn limits(&self) -> Limits {
        Limits::default()
    }
}

impl Default for Harness {
    fn default() -> Self {
        Harness::new()
    }
}

/// Build a text snapshot whose content and snapshot hash derive from `text`.
pub fn text_snapshot(text: &str) -> CapturedSnapshot {
    let bytes = text.as_bytes().to_vec();
    CapturedSnapshot {
        snapshot_sha256: context_drop_core::sha256_hex(&bytes),
        items: vec![CapturedItem {
            kind: ItemKind::Text,
            mime_type: "text/plain".into(),
            ext: "txt".into(),
            bytes,
        }],
    }
}

/// Create a DRAFT packet with `n` distinct text items and return its id.
pub fn draft_with_items(db: &mut Db, storage: &Storage, n: usize) -> String {
    let id = create_draft(db, storage).unwrap();
    for i in 0..n {
        let snap = text_snapshot(&format!("item-{i}"));
        append_snapshot(db, storage, &id, &snap, &Limits::default()).unwrap();
    }
    id
}
