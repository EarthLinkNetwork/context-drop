//! Genuinely simultaneous concurrency tests (spec §9, §33, §43 Test A/E).
//!
//! Each thread opens its own database connection to the same file — exactly how
//! independent CLI invocations behave. WAL + busy_timeout + `BEGIN IMMEDIATE`
//! must guarantee that a packet is never claimed twice, and that an append can
//! never win a race against a claim.

use std::collections::HashSet;
use std::sync::{Arc, Barrier};
use std::thread;

use context_drop_core::{
    append_snapshot, claim, AppendOutcome, ClaimContext, CoreError, Db, Limits, PacketState,
};
use context_drop_integration_tests::{draft_with_items, text_snapshot, Harness};

fn ctx(session: &str) -> ClaimContext {
    ClaimContext {
        session_id: Some(session.to_string()),
        cwd: "/repo/x".into(),
        project_root: "/repo/x".into(),
        project_name: "x".into(),
        config_dir: None,
        note: None,
        terminal: None,
    }
}

#[test]
fn one_packet_n_simultaneous_claims_exactly_one_succeeds() {
    let h = Harness::new();
    {
        let mut db = h.open_db();
        draft_with_items(&mut db, &h.storage, 3);
    }

    const THREADS: usize = 8;
    let barrier = Arc::new(Barrier::new(THREADS));
    let mut handles = Vec::new();
    for i in 0..THREADS {
        let db_path = h.db_path.clone();
        let storage = h.storage.clone();
        let barrier = barrier.clone();
        handles.push(thread::spawn(move || {
            let mut db = Db::open(&db_path).unwrap();
            barrier.wait();
            claim(&mut db, &storage, &ctx(&format!("S{i}")))
        }));
    }

    let mut ok = 0;
    let mut no_packet = 0;
    for handle in handles {
        match handle.join().unwrap() {
            Ok(_) => ok += 1,
            Err(CoreError::NoPacket) => no_packet += 1,
            Err(e) => panic!("unexpected error: {e:?}"),
        }
    }
    assert_eq!(ok, 1, "exactly one claim must succeed");
    assert_eq!(no_packet, THREADS - 1);
}

#[test]
fn twenty_packets_twenty_threads_all_distinct() {
    let h = Harness::new();
    {
        let mut db = h.open_db();
        for _ in 0..20 {
            let p = draft_with_items(&mut db, &h.storage, 1);
            context_drop_core::finalize_capture(&mut db, &h.storage, &p).unwrap();
        }
    }

    const THREADS: usize = 20;
    let barrier = Arc::new(Barrier::new(THREADS));
    let mut handles = Vec::new();
    for i in 0..THREADS {
        let db_path = h.db_path.clone();
        let storage = h.storage.clone();
        let barrier = barrier.clone();
        handles.push(thread::spawn(move || {
            let mut db = Db::open(&db_path).unwrap();
            barrier.wait();
            claim(&mut db, &storage, &ctx(&format!("S{i:02}")))
        }));
    }

    let mut claimed = HashSet::new();
    for handle in handles {
        let res = handle.join().unwrap().expect("all 20 should claim");
        assert!(claimed.insert(res.packet_id), "a packet was claimed twice");
    }
    assert_eq!(claimed.len(), 20);
}

// Note: the cleanup-vs-claim guarantee is verified single-threaded in
// tests/integration/lifecycle.rs (the guarded, state+updated_at conditional
// delete). A multi-threaded stress version was removed: it spawned a thread
// storm and its invariant was unsound (it forced an artificial "future now"
// that also defeats the real age check), so it did not reflect production.

#[test]
fn append_never_wins_against_claim() {
    // Fire a claim and an append at the same instant, many times. Invariants
    // that must ALWAYS hold regardless of who wins the write lock:
    //   * the claim succeeds (there is a DRAFT with items),
    //   * the packet ends up CLAIMED,
    //   * the stored item count is 1 (seed) plus 1 iff the append committed
    //     before the claim — an append can never land on a CLAIMED packet.
    // A modest iteration count: enough to exercise both orderings without
    // spawning a heavy thread storm.
    for iteration in 0..8 {
        let h = Harness::new();
        let packet_id = {
            let mut db = h.open_db();
            let id = draft_with_items(&mut db, &h.storage, 1); // seed: 1 item
            id
        };

        let barrier = Arc::new(Barrier::new(2));

        let claim_handle = {
            let db_path = h.db_path.clone();
            let storage = h.storage.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let mut db = Db::open(&db_path).unwrap();
                barrier.wait();
                claim(&mut db, &storage, &ctx(&format!("claimer-{iteration}")))
            })
        };

        let append_handle = {
            let db_path = h.db_path.clone();
            let storage = h.storage.clone();
            let barrier = barrier.clone();
            let pid = packet_id.clone();
            thread::spawn(move || {
                let mut db = Db::open(&db_path).unwrap();
                let snap = text_snapshot("late-append");
                barrier.wait();
                append_snapshot(&mut db, &storage, &pid, &snap, &Limits::default()).unwrap()
            })
        };

        let claim_res = claim_handle.join().unwrap();
        let append_res = append_handle.join().unwrap();

        assert!(claim_res.is_ok(), "iter {iteration}: claim must succeed");

        let mut db = h.open_db();
        let summary = context_drop_core::packet_summary(&db, &packet_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            summary.state,
            PacketState::Claimed,
            "iter {iteration}: packet must end CLAIMED"
        );

        let expected_items = match append_res {
            AppendOutcome::Added { .. } => 2, // committed before the claim
            AppendOutcome::StateChanged { actual } => {
                assert_eq!(actual, PacketState::Claimed);
                1
            }
            other => panic!("iter {iteration}: unexpected append outcome {other:?}"),
        };
        assert_eq!(
            summary.item_count, expected_items,
            "iter {iteration}: item count must match who won the race"
        );

        // A fresh append after the claim is always refused.
        let after = append_snapshot(
            &mut db,
            &h.storage,
            &packet_id,
            &text_snapshot("after-claim"),
            &Limits::default(),
        )
        .unwrap();
        assert!(matches!(
            after,
            AppendOutcome::StateChanged {
                actual: PacketState::Claimed
            }
        ));
    }
}
