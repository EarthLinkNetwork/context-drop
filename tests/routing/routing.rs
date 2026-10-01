//! Routing tests (spec §43, Tests A–F) and claim-priority / dedupe behavior.
//!
//! These are single-threaded logical tests; the genuinely simultaneous races
//! live in `concurrency.rs`.

use context_drop_core::{
    append_snapshot, claim, create_draft, finalize_capture, undo, AppendOutcome, ClaimContext,
    CoreError, Limits, PacketState, DEFAULT_UNDO_WINDOW_MS,
};
use context_drop_integration_tests::{draft_with_items, text_snapshot, Harness};

fn ctx(session: &str, root: &str, config_dir: Option<&str>) -> ClaimContext {
    ClaimContext {
        session_id: Some(session.to_string()),
        cwd: root.to_string(),
        project_root: root.to_string(),
        project_name: root.rsplit('/').next().unwrap_or(root).to_string(),
        config_dir: config_dir.map(|s| s.to_string()),
    }
}

#[test]
fn test_a_one_packet_two_sequential_claims_exactly_one_succeeds() {
    let h = Harness::new();
    let mut db = h.open_db();
    draft_with_items(&mut db, &h.storage, 3);

    let first = claim(&mut db, &h.storage, &ctx("A", "/repo/alpha", None));
    let second = claim(&mut db, &h.storage, &ctx("B", "/repo/alpha", None));

    assert!(first.is_ok(), "first claim should succeed");
    assert!(
        matches!(second, Err(CoreError::NoPacket)),
        "second claim must find no packet, got {second:?}"
    );
    assert_eq!(first.unwrap().item_count, 3);
}

#[test]
fn test_b_two_packets_two_sessions_get_distinct_packets() {
    let h = Harness::new();
    let mut db = h.open_db();
    // Two READY packets.
    let p1 = draft_with_items(&mut db, &h.storage, 2);
    finalize_capture(&mut db, &h.storage, &p1).unwrap();
    let p2 = draft_with_items(&mut db, &h.storage, 1);
    finalize_capture(&mut db, &h.storage, &p2).unwrap();

    let a = claim(&mut db, &h.storage, &ctx("A", "/repo/a", None)).unwrap();
    let b = claim(&mut db, &h.storage, &ctx("B", "/repo/b", None)).unwrap();

    assert_ne!(
        a.packet_id, b.packet_id,
        "sessions must get distinct packets"
    );
    let mut got = [a.packet_id, b.packet_id];
    got.sort();
    let mut want = [p1, p2];
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn test_c_draft_with_items_wins_over_old_ready() {
    let h = Harness::new();
    let mut db = h.open_db();
    // Old READY packet first.
    let ready = draft_with_items(&mut db, &h.storage, 1);
    finalize_capture(&mut db, &h.storage, &ready).unwrap();
    // Newer DRAFT with items.
    let draft = draft_with_items(&mut db, &h.storage, 2);

    let claimed = claim(&mut db, &h.storage, &ctx("A", "/repo/a", None)).unwrap();
    assert_eq!(
        claimed.packet_id, draft,
        "DRAFT-with-items must win over READY"
    );

    // The DRAFT is now CLAIMED (capture ended atomically).
    let summary = context_drop_core::packet_summary(&db, &draft)
        .unwrap()
        .unwrap();
    assert_eq!(summary.state, PacketState::Claimed);
}

#[test]
fn test_d_same_repo_different_sessions_are_distinguished() {
    let h = Harness::new();
    let mut db = h.open_db();
    // Two packets from the SAME repository path.
    let p1 = draft_with_items(&mut db, &h.storage, 1);
    finalize_capture(&mut db, &h.storage, &p1).unwrap();
    let p2 = draft_with_items(&mut db, &h.storage, 1);
    finalize_capture(&mut db, &h.storage, &p2).unwrap();

    let a = claim(&mut db, &h.storage, &ctx("session-A", "/same/repo", None)).unwrap();
    let b = claim(&mut db, &h.storage, &ctx("session-B", "/same/repo", None)).unwrap();

    // Same project root, but distinct sessions bound to distinct packets.
    assert_eq!(a.project_root, "/same/repo");
    assert_eq!(b.project_root, "/same/repo");
    assert_eq!(a.session_id, "session-A");
    assert_eq!(b.session_id, "session-B");
    assert_ne!(a.packet_id, b.packet_id);
}

#[test]
fn test_e_twenty_sessions_route_deterministically() {
    let h = Harness::new();
    let mut db = h.open_db();
    // 20 READY packets.
    let mut packet_ids = Vec::new();
    for _ in 0..20 {
        let p = draft_with_items(&mut db, &h.storage, 1);
        finalize_capture(&mut db, &h.storage, &p).unwrap();
        packet_ids.push(p);
    }

    // 20 unique sessions each claim exactly one.
    let mut claimed = std::collections::HashSet::new();
    for i in 0..20 {
        let session = format!("session-{i:02}");
        let res = claim(&mut db, &h.storage, &ctx(&session, "/repo/x", None)).unwrap();
        assert!(claimed.insert(res.packet_id), "no packet claimed twice");
    }
    assert_eq!(claimed.len(), 20);
    // 21st claim: nothing left.
    let none = claim(&mut db, &h.storage, &ctx("session-99", "/repo/x", None));
    assert!(matches!(none, Err(CoreError::NoPacket)));
}

#[test]
fn test_f_different_config_dirs_share_one_global_store() {
    // The Context Drop store is global per OS user; CLAUDE_CONFIG_DIR is only
    // recorded, never used for routing or store selection. Different config
    // dirs therefore all claim from the same packet store.
    let h = Harness::new();
    let mut db = h.open_db();
    let p1 = draft_with_items(&mut db, &h.storage, 1);
    finalize_capture(&mut db, &h.storage, &p1).unwrap();
    let p2 = draft_with_items(&mut db, &h.storage, 1);
    finalize_capture(&mut db, &h.storage, &p2).unwrap();

    let a = claim(
        &mut db,
        &h.storage,
        &ctx("A", "/repo/a", Some("/home/u/.claude")),
    )
    .unwrap();
    let b = claim(
        &mut db,
        &h.storage,
        &ctx("B", "/repo/b", Some("/home/u/.claude-work")),
    )
    .unwrap();
    assert_ne!(a.packet_id, b.packet_id);
}

#[test]
fn missing_session_id_is_rejected_not_routed_by_project() {
    let h = Harness::new();
    let mut db = h.open_db();
    draft_with_items(&mut db, &h.storage, 1);
    let no_session = ClaimContext {
        session_id: None,
        cwd: "/repo/a".into(),
        project_root: "/repo/a".into(),
        project_name: "a".into(),
        config_dir: None,
    };
    let res = claim(&mut db, &h.storage, &no_session);
    assert!(matches!(res, Err(CoreError::MissingSessionId)));
    // Empty string session id is also rejected.
    let empty = ClaimContext {
        session_id: Some("   ".into()),
        ..no_session
    };
    assert!(matches!(
        claim(&mut db, &h.storage, &empty),
        Err(CoreError::MissingSessionId)
    ));
}

#[test]
fn consecutive_duplicates_are_deduped_but_repeats_are_kept() {
    // Sequence A A A B A must be stored as A B A.
    let h = Harness::new();
    let mut db = h.open_db();
    let id = create_draft(&mut db, &h.storage).unwrap();
    let a = text_snapshot("A");
    let b = text_snapshot("B");
    let limits = Limits::default();

    let seq = [&a, &a, &a, &b, &a];
    let mut added = 0;
    for snap in seq {
        match append_snapshot(&mut db, &h.storage, &id, snap, &limits).unwrap() {
            AppendOutcome::Added { .. } => added += 1,
            AppendOutcome::Duplicate => {}
            other => panic!("unexpected: {other:?}"),
        }
    }
    assert_eq!(added, 3, "A, B, A should be stored (3 items)");
    let summary = context_drop_core::packet_summary(&db, &id)
        .unwrap()
        .unwrap();
    assert_eq!(summary.item_count, 3);
}

#[test]
fn undo_returns_claim_to_ready_within_window() {
    let h = Harness::new();
    let mut db = h.open_db();
    draft_with_items(&mut db, &h.storage, 2);
    let claimed = claim(&mut db, &h.storage, &ctx("S1", "/repo/a", None)).unwrap();
    // Packet is CLAIMED.
    assert_eq!(
        context_drop_core::packet_summary(&db, &claimed.packet_id)
            .unwrap()
            .unwrap()
            .state,
        PacketState::Claimed
    );
    let undone = undo(&mut db, &h.storage, "S1", DEFAULT_UNDO_WINDOW_MS).unwrap();
    assert_eq!(undone.packet_id, claimed.packet_id);
    assert_eq!(
        context_drop_core::packet_summary(&db, &claimed.packet_id)
            .unwrap()
            .unwrap()
            .state,
        PacketState::Ready
    );

    // A different session cannot undo this claim.
    draft_with_items(&mut db, &h.storage, 1);
    let c2 = claim(&mut db, &h.storage, &ctx("S2", "/repo/b", None)).unwrap();
    assert!(matches!(
        undo(&mut db, &h.storage, "OTHER", DEFAULT_UNDO_WINDOW_MS),
        Err(CoreError::NothingToUndo)
    ));
    // But its own session can.
    assert_eq!(
        undo(&mut db, &h.storage, "S2", DEFAULT_UNDO_WINDOW_MS)
            .unwrap()
            .packet_id,
        c2.packet_id
    );
}

#[test]
fn undo_outside_window_is_not_eligible() {
    let h = Harness::new();
    let mut db = h.open_db();
    draft_with_items(&mut db, &h.storage, 1);
    claim(&mut db, &h.storage, &ctx("S1", "/repo/a", None)).unwrap();
    // A zero-length window makes any past claim ineligible.
    assert!(matches!(
        undo(&mut db, &h.storage, "S1", 0),
        Err(CoreError::NothingToUndo)
    ));
}
