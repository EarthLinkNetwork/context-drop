//! End-to-end lifecycle, size-limit, cleanup, and manifest integration tests.

use std::fs;

use context_drop_core::{
    append_snapshot, claim, cleanup, clear_packet, consume, create_draft, finalize_capture,
    mark_processing, release, undo, AppendOutcome, CapturedItem, CapturedSnapshot, ClaimContext,
    CoreError, Db, Limits, Manifest, PacketState, DEFAULT_UNDO_WINDOW_MS, MANIFEST_SCHEMA_VERSION,
};
use context_drop_integration_tests::{draft_with_items, text_snapshot, Harness};

fn ctx(session: &str) -> ClaimContext {
    ClaimContext {
        session_id: Some(session.to_string()),
        cwd: "/repo/demo".into(),
        project_root: "/repo/demo".into(),
        project_name: "demo".into(),
        config_dir: Some("/home/u/.claude".into()),
        note: None,
        terminal: None,
    }
}

#[test]
fn full_happy_path_draft_to_consumed() {
    let h = Harness::new();
    let mut db = h.open_db();

    let draft = draft_with_items(&mut db, &h.storage, 2);
    assert_eq!(state(&db, &draft), PacketState::Draft);

    let claimed = claim(&mut db, &h.storage, &ctx("S1")).unwrap();
    assert_eq!(claimed.packet_id, draft);
    assert_eq!(state(&db, &draft), PacketState::Claimed);

    mark_processing(&mut db, &h.storage, &draft, "S1", Some(&claimed.claim_id)).unwrap();
    assert_eq!(state(&db, &draft), PacketState::Processing);

    consume(&mut db, &h.storage, &draft, "S1", None).unwrap();
    assert_eq!(state(&db, &draft), PacketState::Consumed);

    // consume is idempotent.
    consume(&mut db, &h.storage, &draft, "S1", None).unwrap();
    assert_eq!(state(&db, &draft), PacketState::Consumed);
}

#[test]
fn consume_walks_through_processing_from_claimed() {
    let h = Harness::new();
    let mut db = h.open_db();
    let draft = draft_with_items(&mut db, &h.storage, 1);
    claim(&mut db, &h.storage, &ctx("S1")).unwrap();
    // Consume directly from CLAIMED: the state machine passes through PROCESSING.
    consume(&mut db, &h.storage, &draft, "S1", None).unwrap();
    assert_eq!(state(&db, &draft), PacketState::Consumed);
}

#[test]
fn release_returns_claim_to_ready() {
    let h = Harness::new();
    let mut db = h.open_db();
    let draft = draft_with_items(&mut db, &h.storage, 1);
    claim(&mut db, &h.storage, &ctx("S1")).unwrap();
    release(&mut db, &h.storage, &draft).unwrap();
    assert_eq!(state(&db, &draft), PacketState::Ready);
    // It can be claimed again afterward.
    let again = claim(&mut db, &h.storage, &ctx("S2")).unwrap();
    assert_eq!(again.packet_id, draft);
}

#[test]
fn clear_packet_removes_draft_and_files() {
    let h = Harness::new();
    let mut db = h.open_db();
    let draft = draft_with_items(&mut db, &h.storage, 2);
    assert!(h.storage.items_dir(&draft).exists());
    clear_packet(&mut db, &h.storage, &draft).unwrap();
    assert!(context_drop_core::packet_summary(&db, &draft)
        .unwrap()
        .is_none());
    assert!(!h.storage.packet_dir(&draft).exists());
}

#[test]
fn item_too_large_is_rejected_and_packet_preserved() {
    let h = Harness::new();
    let mut db = h.open_db();
    let draft = create_draft(&mut db, &h.storage).unwrap();
    let limits = Limits {
        max_item_bytes: 10,
        max_packet_bytes: 1_000,
    };
    let big = CapturedSnapshot {
        snapshot_sha256: context_drop_core::sha256_hex(b"12345678901234567890"),
        items: vec![CapturedItem {
            kind: context_drop_core::ItemKind::Text,
            mime_type: "text/plain".into(),
            ext: "txt".into(),
            bytes: vec![b'x'; 20], // exceeds 10-byte item limit
        }],
    };
    let outcome = append_snapshot(&mut db, &h.storage, &draft, &big, &limits).unwrap();
    assert!(matches!(
        outcome,
        AppendOutcome::RejectedItemTooLarge {
            size: 20,
            limit: 10,
            ..
        }
    ));
    // Packet preserved and empty.
    assert_eq!(
        context_drop_core::packet_summary(&db, &draft)
            .unwrap()
            .unwrap()
            .item_count,
        0
    );
    // A small item still succeeds afterward.
    let ok = append_snapshot(&mut db, &h.storage, &draft, &text_snapshot("hi"), &limits).unwrap();
    assert!(matches!(ok, AppendOutcome::Added { .. }));
}

#[test]
fn packet_full_is_rejected_and_packet_preserved() {
    let h = Harness::new();
    let mut db = h.open_db();
    let draft = create_draft(&mut db, &h.storage).unwrap();
    let limits = Limits {
        max_item_bytes: 100,
        max_packet_bytes: 12, // room for ~1 small item
    };
    let a = append_snapshot(
        &mut db,
        &h.storage,
        &draft,
        &text_snapshot("hello"),
        &limits,
    )
    .unwrap();
    assert!(matches!(a, AppendOutcome::Added { .. }));
    // Next item would exceed the packet budget.
    let b = append_snapshot(
        &mut db,
        &h.storage,
        &draft,
        &text_snapshot("world-too-big"),
        &limits,
    )
    .unwrap();
    assert!(matches!(
        b,
        AppendOutcome::RejectedPacketFull { limit: 12, .. }
    ));
    assert_eq!(
        context_drop_core::packet_summary(&db, &draft)
            .unwrap()
            .unwrap()
            .item_count,
        1
    );
}

#[test]
fn ttl_cleanup_removes_stale_but_protects_draft() {
    let h = Harness::new();
    let mut db = h.open_db();

    // A CONSUMED packet.
    let consumed = draft_with_items(&mut db, &h.storage, 1);
    claim(&mut db, &h.storage, &ctx("S1")).unwrap();
    consume(&mut db, &h.storage, &consumed, "S1", None).unwrap();

    // A READY packet.
    let ready = draft_with_items(&mut db, &h.storage, 1);
    finalize_capture(&mut db, &h.storage, &ready).unwrap();

    // A protected DRAFT packet (capture in progress / recoverable).
    let draft = draft_with_items(&mut db, &h.storage, 1);

    // Pretend it is 48h later; TTL is 24h.
    let future = context_drop_core::clock::now_ms() + 48 * 3600 * 1000;
    let report = cleanup(&mut db, &h.storage, 24, future).unwrap();

    assert!(report.removed.contains(&consumed));
    assert!(report.removed.contains(&ready));
    assert!(report.expired.contains(&ready)); // READY marked EXPIRED before removal
    assert!(!report.removed.contains(&draft), "DRAFT must be protected");

    // DRAFT survives; its files remain.
    assert_eq!(state(&db, &draft), PacketState::Draft);
    assert!(h.storage.packet_dir(&draft).exists());
    // Removed packets' directories are gone.
    assert!(!h.storage.packet_dir(&consumed).exists());
}

#[test]
fn manifest_is_written_and_carries_no_raw_content() {
    let h = Harness::new();
    let mut db = h.open_db();
    let draft = create_draft(&mut db, &h.storage).unwrap();

    // Append an item whose content is a recognizable secret.
    let secret = "SECRET-abc123-do-not-leak";
    append_snapshot(
        &mut db,
        &h.storage,
        &draft,
        &text_snapshot(secret),
        &Limits::default(),
    )
    .unwrap();
    claim(&mut db, &h.storage, &ctx("S1")).unwrap();

    let manifest_path = h.storage.manifest_path(&draft);
    assert!(
        manifest_path.exists(),
        "manifest.json must exist after claim"
    );
    let raw = fs::read_to_string(&manifest_path).unwrap();

    // Parse and check structure.
    let manifest: Manifest = serde_json::from_str(&raw).unwrap();
    assert_eq!(manifest.schema_version, MANIFEST_SCHEMA_VERSION);
    assert_eq!(manifest.state, "CLAIMED");
    assert_eq!(manifest.items.len(), 1);
    let claim_meta = manifest.claim.expect("claim metadata present");
    assert_eq!(claim_meta.session_id, "S1");
    assert_eq!(claim_meta.project_name, "demo");

    // The secret content must NOT appear anywhere in the manifest.
    assert!(!raw.contains(secret), "manifest leaked raw item content");
    // But the item file on disk DOES contain the content (that's the point).
    let item_file = h
        .storage
        .packet_dir(&draft)
        .join(&manifest.items[0].relative_path);
    assert_eq!(fs::read_to_string(item_file).unwrap(), secret);
}

#[test]
fn clear_packet_on_claimed_is_refused_and_preserves_files() {
    // Regression: clearing must not delete a packet that was claimed between the
    // state check and the delete. A CLAIMED packet's material is safe.
    let h = Harness::new();
    let mut db = h.open_db();
    let draft = draft_with_items(&mut db, &h.storage, 2);
    claim(&mut db, &h.storage, &ctx("A")).unwrap(); // now CLAIMED
    assert!(h.storage.items_dir(&draft).exists());

    let result = clear_packet(&mut db, &h.storage, &draft);
    assert!(
        matches!(result, Err(CoreError::StateChanged { .. })),
        "clearing a CLAIMED packet must be refused, got {result:?}"
    );
    // Row and files preserved.
    assert!(context_drop_core::packet_summary(&db, &draft)
        .unwrap()
        .is_some());
    assert!(h.storage.packet_dir(&draft).exists());
    assert_eq!(state(&db, &draft), PacketState::Claimed);
}

#[test]
fn consume_refused_after_undo_and_reclaim_by_another_session() {
    // Regression: A claims -> A undoes (release) -> B claims -> A finishes late.
    // A's consume must NOT consume B's claim.
    let h = Harness::new();
    let mut db = h.open_db();
    let pid = draft_with_items(&mut db, &h.storage, 2);

    let a = claim(&mut db, &h.storage, &ctx("A")).unwrap();
    undo(&mut db, &h.storage, "A", DEFAULT_UNDO_WINDOW_MS).unwrap(); // back to READY
    let b = claim(&mut db, &h.storage, &ctx("B")).unwrap();
    assert_eq!(b.packet_id, pid, "B re-claims the same packet");

    // A tries to consume late with its own (now-stale) claim — refused.
    let a_consume = consume(&mut db, &h.storage, &pid, "A", Some(&a.claim_id));
    assert!(
        matches!(a_consume, Err(CoreError::ClaimNotOwned { .. })),
        "stale session A must not consume B's claim, got {a_consume:?}"
    );
    assert_eq!(state(&db, &pid), PacketState::Claimed);

    // B consumes its own claim normally.
    consume(&mut db, &h.storage, &pid, "B", Some(&b.claim_id)).unwrap();
    assert_eq!(state(&db, &pid), PacketState::Consumed);
}

#[test]
fn consume_refused_after_undo_and_reclaim_by_same_session() {
    // Stronger regression (codex): the SAME session A claims, undoes, then
    // re-claims. A's *first* claim finishing late must not consume the second
    // claim — the claim-id check (not just session) prevents it.
    let h = Harness::new();
    let mut db = h.open_db();
    let pid = draft_with_items(&mut db, &h.storage, 1);

    let first = claim(&mut db, &h.storage, &ctx("A")).unwrap();
    undo(&mut db, &h.storage, "A", DEFAULT_UNDO_WINDOW_MS).unwrap();
    let second = claim(&mut db, &h.storage, &ctx("A")).unwrap(); // same session!
    assert_eq!(second.packet_id, pid);
    assert_ne!(
        first.claim_id, second.claim_id,
        "each claim has a distinct id"
    );

    // A's stale first consume must be refused even though the session matches.
    let stale = consume(&mut db, &h.storage, &pid, "A", Some(&first.claim_id));
    assert!(
        matches!(stale, Err(CoreError::ClaimNotOwned { .. })),
        "the first (stale) claim must not consume the second, got {stale:?}"
    );
    assert_eq!(state(&db, &pid), PacketState::Claimed);

    // The second claim consumes correctly.
    consume(&mut db, &h.storage, &pid, "A", Some(&second.claim_id)).unwrap();
    assert_eq!(state(&db, &pid), PacketState::Consumed);
}

#[test]
fn cleanup_sweeps_orphan_packet_directories() {
    // Regression: files left with no DB row (e.g. a crash between row-delete and
    // file-remove) are self-healed by the next cleanup, and live packets are kept.
    let h = Harness::new();
    let mut db = h.open_db();

    // An orphan directory with no packets row.
    let orphan = "orphan-no-row";
    h.storage.ensure_packet_dirs(orphan).unwrap();
    assert!(h.storage.packet_dir(orphan).exists());

    // A live DRAFT that must be preserved.
    let draft = create_draft(&mut db, &h.storage).unwrap();

    let report = cleanup(&mut db, &h.storage, 24, context_drop_core::clock::now_ms()).unwrap();

    assert!(report.orphans_removed.contains(&orphan.to_string()));
    assert!(!h.storage.packet_dir(orphan).exists());
    assert!(h.storage.packet_dir(&draft).exists(), "live draft dir kept");
}

#[test]
fn delete_item_removes_one_and_adjusts_totals() {
    let h = Harness::new();
    let mut db = h.open_db();
    let draft = draft_with_items(&mut db, &h.storage, 3);
    let before = context_drop_core::packet_summary(&db, &draft)
        .unwrap()
        .unwrap();
    assert_eq!(before.item_count, 3);

    let items = context_drop_core::recent_items(&db, &draft, 10).unwrap();
    let victim = items[1].clone();
    let victim_path = h.storage.packet_dir(&draft).join(&victim.relative_path);
    assert!(victim_path.exists());

    context_drop_core::delete_item(&mut db, &h.storage, &draft, &victim.id).unwrap();

    let after = context_drop_core::packet_summary(&db, &draft)
        .unwrap()
        .unwrap();
    assert_eq!(after.item_count, 2);
    assert_eq!(after.total_bytes, before.total_bytes - victim.byte_size);
    assert!(
        !victim_path.exists(),
        "the item's file is removed from disk"
    );

    // Deleting an already-gone item is a no-op (no error, count unchanged).
    context_drop_core::delete_item(&mut db, &h.storage, &draft, &victim.id).unwrap();
    assert_eq!(
        context_drop_core::packet_summary(&db, &draft)
            .unwrap()
            .unwrap()
            .item_count,
        2
    );
}

#[test]
fn delete_item_refused_after_claim() {
    let h = Harness::new();
    let mut db = h.open_db();
    let draft = draft_with_items(&mut db, &h.storage, 2);
    let items = context_drop_core::recent_items(&db, &draft, 10).unwrap();
    claim(&mut db, &h.storage, &ctx("S1")).unwrap();
    // A claimed (dispatched) packet is immutable: per-item delete is refused.
    let err = context_drop_core::delete_item(&mut db, &h.storage, &draft, &items[0].id);
    assert!(matches!(err, Err(CoreError::StateChanged { .. })));
    assert_eq!(
        context_drop_core::packet_summary(&db, &draft)
            .unwrap()
            .unwrap()
            .item_count,
        2
    );
}

fn state(db: &Db, packet_id: &str) -> PacketState {
    context_drop_core::packet_summary(db, packet_id)
        .unwrap()
        .unwrap()
        .state
}
