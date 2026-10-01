//! Packet lifecycle operations: draft creation, appending captured snapshots
//! (with consecutive dedupe and size limits), stop/finalize, atomic claim,
//! processing/consume/release, and session-scoped undo.
//!
//! Every state change goes through a `BEGIN IMMEDIATE` transaction so that
//! concurrent CLI claims and desktop appends serialize correctly. Atomic claim
//! uses a conditional `UPDATE ... WHERE state = <expected>` so exactly one
//! caller can move a given packet out of DRAFT/READY.

use rusqlite::{params, OptionalExtension, TransactionBehavior};

use crate::clock::now_ms;
use crate::database::{count_items, get_packet, latest_claim_meta, list_items, next_seq, Db};
use crate::error::{CoreError, Result};
use crate::id::{new_claim_id, new_item_id, new_packet_id};
use crate::manifest::Manifest;
use crate::packet::{ItemKind, PacketItem, PacketState};
use crate::settings::Limits;
use crate::storage::Storage;

/// One captured clipboard item, ready to be persisted.
#[derive(Debug, Clone)]
pub struct CapturedItem {
    pub kind: ItemKind,
    pub mime_type: String,
    /// Preferred file extension (sanitized on write).
    pub ext: String,
    pub bytes: Vec<u8>,
}

/// A single clipboard change event. Multiple items (e.g. several copied files)
/// share one snapshot hash used for consecutive-duplicate detection.
#[derive(Debug, Clone)]
pub struct CapturedSnapshot {
    /// Hash over the whole snapshot; consecutive identical snapshots are skipped.
    pub snapshot_sha256: String,
    pub items: Vec<CapturedItem>,
}

/// A content-derived snapshot hash over captured items (kind + byte length +
/// bytes). Callers that already hold the real bytes — e.g. the desktop, after
/// reading in-limit copied files — should set `snapshot_sha256` to this so that
/// consecutive-dedupe compares actual CONTENT, not a filename or timestamp.
pub fn snapshot_content_hash(items: &[CapturedItem]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for it in items {
        h.update(it.kind.as_str().as_bytes());
        h.update(b":");
        h.update((it.bytes.len() as u64).to_le_bytes());
        h.update(&it.bytes);
        h.update([0u8]); // separator
    }
    let out = h.finalize();
    let mut s = String::with_capacity(out.len() * 2);
    for b in out {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Outcome of appending a snapshot to a DRAFT packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppendOutcome {
    Added {
        item_ids: Vec<String>,
        item_count: usize,
    },
    /// Snapshot identical to the immediately preceding one; skipped.
    Duplicate,
    /// An individual item exceeded the per-item limit; packet preserved.
    RejectedItemTooLarge { index: usize, size: i64, limit: i64 },
    /// Adding the snapshot would exceed the total packet limit; packet preserved.
    RejectedPacketFull { attempted_total: i64, limit: i64 },
    /// The packet is no longer DRAFT (e.g. it was just claimed): append refused.
    StateChanged { actual: PacketState },
}

/// Routing identity gathered by the claiming CLI.
#[derive(Debug, Clone)]
pub struct ClaimContext {
    pub session_id: Option<String>,
    pub cwd: String,
    pub project_root: String,
    pub project_name: String,
    pub config_dir: Option<String>,
}

/// Metadata-only result of a successful claim (never any raw content).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimResult {
    pub packet_id: String,
    /// The unique id of the claim record just created. Pass this back to
    /// `consume` so a late finish can never consume a *different* claim that
    /// re-used the same packet (even from the same session).
    pub claim_id: String,
    pub manifest_path: String,
    pub item_count: usize,
    pub session_id: String,
    pub cwd: String,
    pub project_root: String,
    pub project_name: String,
}

/// A metadata summary of one packet (no content).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketSummary {
    pub id: String,
    pub state: PacketState,
    pub item_count: i64,
    pub total_bytes: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Information about the most recent dispatch (claim).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastDispatch {
    pub packet_id: String,
    pub project_name: String,
    pub session_id: String,
    pub item_count: i64,
    pub claimed_at_ms: i64,
    pub packet_state: PacketState,
}

/// A high-level status snapshot for the CLI/desktop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusReport {
    pub current_draft: Option<PacketSummary>,
    pub ready_count: i64,
    pub last_dispatch: Option<LastDispatch>,
    pub total_packets: i64,
}

/// Result of an undo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoResult {
    pub packet_id: String,
    pub project_name: String,
    pub item_count: i64,
}

// ---- Draft / capture ----------------------------------------------------

/// Create a fresh DRAFT packet and return its id.
pub fn create_draft(db: &mut Db, storage: &Storage) -> Result<String> {
    let id = new_packet_id();
    let now = now_ms();
    db.conn.execute(
        "INSERT INTO packets(id, state, created_at_ms, updated_at_ms, total_bytes)
         VALUES (?1, 'DRAFT', ?2, ?2, 0)",
        params![id, now],
    )?;
    storage.ensure_packet_dirs(&id)?;
    refresh_manifest(db, storage, &id)?;
    Ok(id)
}

/// The newest DRAFT packet id, if capture is in progress.
pub fn current_draft_id(db: &Db) -> Result<Option<String>> {
    let id = db
        .conn
        .query_row(
            "SELECT id FROM packets WHERE state='DRAFT' ORDER BY created_at_ms DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(id)
}

/// Append a captured snapshot to a DRAFT packet.
///
/// Verifies the packet is still DRAFT inside the transaction (so an append can
/// never win a race against a claim), applies consecutive dedupe, and enforces
/// per-item and total-packet size limits. Rejections preserve the packet.
pub fn append_snapshot(
    db: &mut Db,
    storage: &Storage,
    packet_id: &str,
    snapshot: &CapturedSnapshot,
    limits: &Limits,
) -> Result<AppendOutcome> {
    let now = now_ms();
    let outcome = {
        let tx = db
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let packet = get_packet(&tx, packet_id)?
            .ok_or_else(|| CoreError::PacketNotFound(packet_id.into()))?;

        // Append is only ever valid against a DRAFT packet.
        if packet.state != PacketState::Draft {
            return Ok(AppendOutcome::StateChanged {
                actual: packet.state,
            });
        }

        // Consecutive duplicate: same snapshot hash as the last one stored.
        if packet.last_snapshot_sha256.as_deref() == Some(snapshot.snapshot_sha256.as_str()) {
            return Ok(AppendOutcome::Duplicate);
        }

        // Per-item size limit.
        for (index, item) in snapshot.items.iter().enumerate() {
            let size = item.bytes.len() as i64;
            if size > limits.max_item_bytes {
                return Ok(AppendOutcome::RejectedItemTooLarge {
                    index,
                    size,
                    limit: limits.max_item_bytes,
                });
            }
        }

        // Total packet size limit.
        let add_total: i64 = snapshot.items.iter().map(|i| i.bytes.len() as i64).sum();
        let attempted_total = packet.total_bytes + add_total;
        if attempted_total > limits.max_packet_bytes {
            return Ok(AppendOutcome::RejectedPacketFull {
                attempted_total,
                limit: limits.max_packet_bytes,
            });
        }

        // Persist each item: atomic file write, then row insert.
        let start_seq = next_seq(&tx, packet_id)?;
        let mut item_ids = Vec::with_capacity(snapshot.items.len());
        for (offset, item) in snapshot.items.iter().enumerate() {
            let seq = start_seq + offset as i64;
            let stored = storage.write_item_atomic(packet_id, seq, &item.ext, &item.bytes)?;
            let item_id = new_item_id();
            tx.execute(
                "INSERT INTO packet_items
                    (id, packet_id, seq, kind, mime_type, relative_path, byte_size, sha256, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    item_id,
                    packet_id,
                    seq,
                    item.kind.as_str(),
                    item.mime_type,
                    stored.relative_path,
                    stored.byte_size,
                    stored.sha256,
                    now,
                ],
            )?;
            item_ids.push(item_id);
        }
        tx.execute(
            "UPDATE packets
             SET last_snapshot_sha256 = ?1, total_bytes = total_bytes + ?2, updated_at_ms = ?3
             WHERE id = ?4",
            params![snapshot.snapshot_sha256, add_total, now, packet_id],
        )?;
        tx.commit()?;
        AppendOutcome::Added {
            item_ids,
            item_count: 0, // filled in below after commit
        }
    };

    match outcome {
        AppendOutcome::Added { item_ids, .. } => {
            let item_count = count_items(&db.conn, packet_id)? as usize;
            refresh_manifest(db, storage, packet_id)?;
            Ok(AppendOutcome::Added {
                item_ids,
                item_count,
            })
        }
        other => Ok(other),
    }
}

/// Stop capture: DRAFT -> READY. Idempotent if already READY.
pub fn finalize_capture(db: &mut Db, storage: &Storage, packet_id: &str) -> Result<()> {
    let now = now_ms();
    {
        let tx = db
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let packet = get_packet(&tx, packet_id)?
            .ok_or_else(|| CoreError::PacketNotFound(packet_id.into()))?;
        match packet.state {
            PacketState::Ready => {
                tx.commit()?;
                return Ok(());
            }
            PacketState::Draft => {
                transition_locked(&tx, packet_id, PacketState::Draft, PacketState::Ready, now)?;
                tx.commit()?;
            }
            other => {
                return Err(CoreError::StateChanged {
                    expected: PacketState::Draft,
                    actual: other,
                });
            }
        }
    }
    refresh_manifest(db, storage, packet_id)
}

/// Discard a DRAFT (or READY) packet entirely: delete files and rows.
///
/// The state check and the delete are performed atomically inside a
/// `BEGIN IMMEDIATE` transaction with a state-guarded `DELETE`, so a packet that
/// is claimed concurrently (DRAFT/READY -> CLAIMED) between check and delete is
/// **not** removed — its just-dispatched material is safe.
pub fn clear_packet(db: &mut Db, storage: &Storage, packet_id: &str) -> Result<()> {
    let deleted = {
        let tx = db
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let packet = get_packet(&tx, packet_id)?
            .ok_or_else(|| CoreError::PacketNotFound(packet_id.into()))?;
        if !matches!(packet.state, PacketState::Draft | PacketState::Ready) {
            return Err(CoreError::StateChanged {
                expected: PacketState::Draft,
                actual: packet.state,
            });
        }
        // State-guarded delete: only removes the row if it is still DRAFT/READY.
        let n = tx.execute(
            "DELETE FROM packets WHERE id = ?1 AND state IN ('DRAFT','READY')",
            params![packet_id],
        )?;
        if n != 1 {
            // Raced with a claim; leave the (now-CLAIMED) packet intact.
            let actual = get_packet(&tx, packet_id)?
                .map(|p| p.state)
                .unwrap_or(PacketState::Failed);
            tx.commit()?;
            return Err(CoreError::StateChanged {
                expected: PacketState::Draft,
                actual,
            });
        }
        tx.commit()?;
        true
    };
    // Only remove files once the row deletion committed.
    if deleted {
        storage.remove_packet_dir(packet_id)?;
    }
    Ok(())
}

/// Fetch a single packet item by id (must belong to `packet_id`).
pub fn get_item(db: &Db, packet_id: &str, item_id: &str) -> Result<Option<PacketItem>> {
    Ok(list_items(&db.conn, packet_id)?
        .into_iter()
        .find(|i| i.id == item_id))
}

/// Remove ONE item from a DRAFT packet: delete its row + file and adjust the
/// packet's `total_bytes`. Only valid while the packet is DRAFT — a packet that
/// has been claimed/sent is immutable. Atomic: the row delete is state-guarded
/// inside a `BEGIN IMMEDIATE` transaction; the file is removed only after commit.
/// Deleting a missing item is a no-op (Ok). A concurrent claim makes this refuse
/// with `StateChanged` rather than edit a dispatched packet.
pub fn delete_item(db: &mut Db, storage: &Storage, packet_id: &str, item_id: &str) -> Result<()> {
    let now = now_ms();
    let relative_path = {
        let tx = db
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let packet = get_packet(&tx, packet_id)?
            .ok_or_else(|| CoreError::PacketNotFound(packet_id.into()))?;
        if packet.state != PacketState::Draft {
            return Err(CoreError::StateChanged {
                expected: PacketState::Draft,
                actual: packet.state,
            });
        }
        let row: Option<(String, i64)> = tx
            .query_row(
                "SELECT relative_path, byte_size FROM packet_items WHERE id = ?1 AND packet_id = ?2",
                params![item_id, packet_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((relative_path, byte_size)) = row else {
            // Already gone (or never existed): nothing to delete.
            tx.commit()?;
            return Ok(());
        };
        // State-guarded delete: removes the item only while the packet is DRAFT.
        let n = tx.execute(
            "DELETE FROM packet_items WHERE id = ?1 AND packet_id = ?2
               AND EXISTS (SELECT 1 FROM packets p WHERE p.id = ?2 AND p.state = 'DRAFT')",
            params![item_id, packet_id],
        )?;
        if n != 1 {
            // Raced with a claim between the state check and the delete.
            let actual = get_packet(&tx, packet_id)?
                .map(|p| p.state)
                .unwrap_or(PacketState::Failed);
            tx.commit()?;
            return Err(CoreError::StateChanged {
                expected: PacketState::Draft,
                actual,
            });
        }
        // Adjust the O(1) size tally and clear the consecutive-dedupe marker, so
        // re-copying the same content after a delete is allowed again.
        tx.execute(
            "UPDATE packets
             SET total_bytes = MAX(0, total_bytes - ?1), last_snapshot_sha256 = NULL,
                 updated_at_ms = ?2
             WHERE id = ?3",
            params![byte_size, now, packet_id],
        )?;
        tx.commit()?;
        relative_path
    };
    // Remove the file only after the row deletion committed (best-effort: a
    // missing file must not fail the logical delete).
    let _ = std::fs::remove_file(storage.packet_dir(packet_id).join(&relative_path));
    refresh_manifest(db, storage, packet_id)
}

// ---- Claim / routing ----------------------------------------------------

/// Atomically claim a packet for the given session.
///
/// Priority: (1) newest DRAFT with items > 0, (2) newest READY, (3) NoPacket.
/// Claiming a DRAFT atomically ends capture by moving it to CLAIMED — after
/// which any further append is refused (see `append_snapshot`).
pub fn claim(db: &mut Db, storage: &Storage, ctx: &ClaimContext) -> Result<ClaimResult> {
    // Routing must be session-centric; refuse to route by project alone.
    let session_id = ctx
        .session_id
        .clone()
        .filter(|s| !s.trim().is_empty())
        .ok_or(CoreError::MissingSessionId)?;

    let now = now_ms();
    let (packet_id, claim_id) = {
        let tx = db
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        match pick_and_claim(&tx, now)? {
            Some(pid) => {
                let claim_id = new_claim_id();
                tx.execute(
                    "INSERT INTO claims
                        (id, packet_id, session_id, cwd, project_root, project_name,
                         config_dir, claimed_at_ms, released_at_ms, status)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, 'active')",
                    params![
                        claim_id,
                        pid,
                        session_id,
                        ctx.cwd,
                        ctx.project_root,
                        ctx.project_name,
                        ctx.config_dir,
                        now,
                    ],
                )?;
                tx.commit()?;
                (pid, claim_id)
            }
            None => {
                tx.commit()?;
                return Err(CoreError::NoPacket);
            }
        }
    };

    refresh_manifest(db, storage, &packet_id)?;
    let item_count = count_items(&db.conn, &packet_id)? as usize;
    Ok(ClaimResult {
        claim_id,
        manifest_path: storage
            .manifest_path(&packet_id)
            .to_string_lossy()
            .into_owned(),
        packet_id,
        item_count,
        session_id,
        cwd: ctx.cwd.clone(),
        project_root: ctx.project_root.clone(),
        project_name: ctx.project_name.clone(),
    })
}

/// Inside a held write transaction, pick the highest-priority candidate and
/// claim it with a conditional update. Returns the claimed packet id, or None.
fn pick_and_claim(tx: &rusqlite::Transaction, now: i64) -> Result<Option<String>> {
    // Bounded loop: guards the (cross-process) race where the candidate changes
    // state between selection and the conditional update.
    for _ in 0..256 {
        // Priority 1: newest DRAFT that has at least one item.
        let draft: Option<String> = tx
            .query_row(
                "SELECT p.id FROM packets p
                 WHERE p.state = 'DRAFT'
                   AND EXISTS (SELECT 1 FROM packet_items i WHERE i.packet_id = p.id)
                 ORDER BY p.created_at_ms DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = draft {
            let n = tx.execute(
                "UPDATE packets SET state='CLAIMED', updated_at_ms=?1 WHERE id=?2 AND state='DRAFT'",
                params![now, id],
            )?;
            if n == 1 {
                return Ok(Some(id));
            }
            continue;
        }

        // Priority 2: newest READY.
        let ready: Option<String> = tx
            .query_row(
                "SELECT id FROM packets WHERE state='READY' ORDER BY created_at_ms DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = ready {
            let n = tx.execute(
                "UPDATE packets SET state='CLAIMED', updated_at_ms=?1 WHERE id=?2 AND state='READY'",
                params![now, id],
            )?;
            if n == 1 {
                return Ok(Some(id));
            }
            continue;
        }

        return Ok(None);
    }
    Ok(None)
}

/// Mark a claimed packet as PROCESSING (a subagent has started), on behalf of
/// `session_id`. This is what makes the packet safe from TTL cleanup while it is
/// actively being investigated. Ownership is verified like `consume`.
pub fn mark_processing(
    db: &mut Db,
    storage: &Storage,
    packet_id: &str,
    session_id: &str,
    claim_id: Option<&str>,
) -> Result<()> {
    let now = now_ms();
    {
        let tx = db
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let packet = get_packet(&tx, packet_id)?
            .ok_or_else(|| CoreError::PacketNotFound(packet_id.into()))?;
        // Ownership is verified for BOTH the CLAIMED transition and the
        // already-PROCESSING idempotent case, so a stale session can never
        // (re)start processing on a packet that was re-claimed by another.
        if matches!(packet.state, PacketState::Claimed | PacketState::Processing)
            && !active_claim_is_owned(&tx, packet_id, session_id, claim_id)?
        {
            tx.commit()?;
            return Err(CoreError::ClaimNotOwned {
                packet_id: packet_id.into(),
            });
        }
        match packet.state {
            PacketState::Processing => {
                tx.commit()?;
                return Ok(());
            }
            PacketState::Claimed => {
                transition_locked(
                    &tx,
                    packet_id,
                    PacketState::Claimed,
                    PacketState::Processing,
                    now,
                )?;
                tx.commit()?;
            }
            other => {
                return Err(CoreError::StateChanged {
                    expected: PacketState::Claimed,
                    actual: other,
                });
            }
        }
    }
    refresh_manifest(db, storage, packet_id)
}

/// Consume a packet (normal completion) on behalf of `session_id`. Accepts
/// CLAIMED or PROCESSING and walks the state machine to CONSUMED. Idempotent if
/// already CONSUMED.
///
/// The packet's *active* claim must belong to `session_id`. This prevents the
/// undo/re-claim race: if session A claims a packet, A undoes it (release), and
/// session B re-claims it, then A finishing late must NOT consume B's claim.
pub fn consume(
    db: &mut Db,
    storage: &Storage,
    packet_id: &str,
    session_id: &str,
    claim_id: Option<&str>,
) -> Result<()> {
    let now = now_ms();
    {
        let tx = db
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let packet = get_packet(&tx, packet_id)?
            .ok_or_else(|| CoreError::PacketNotFound(packet_id.into()))?;
        // Idempotent: already consumed.
        if packet.state == PacketState::Consumed {
            tx.commit()?;
            return Ok(());
        }
        // Ownership: the active claim must be this session's (and, when a
        // claim_id is supplied, that exact claim) — so a late finish can never
        // consume a different claim that re-used the same packet.
        if !active_claim_is_owned(&tx, packet_id, session_id, claim_id)? {
            tx.commit()?;
            return Err(CoreError::ClaimNotOwned {
                packet_id: packet_id.into(),
            });
        }
        match packet.state {
            PacketState::Consumed => {
                tx.commit()?;
                return Ok(());
            }
            PacketState::Claimed => {
                transition_locked(
                    &tx,
                    packet_id,
                    PacketState::Claimed,
                    PacketState::Processing,
                    now,
                )?;
                transition_locked(
                    &tx,
                    packet_id,
                    PacketState::Processing,
                    PacketState::Consumed,
                    now,
                )?;
            }
            PacketState::Processing => {
                transition_locked(
                    &tx,
                    packet_id,
                    PacketState::Processing,
                    PacketState::Consumed,
                    now,
                )?;
            }
            other => {
                return Err(CoreError::StateChanged {
                    expected: PacketState::Processing,
                    actual: other,
                });
            }
        }
        tx.execute(
            "UPDATE claims SET status='consumed', released_at_ms=?1
             WHERE packet_id=?2 AND status='active'",
            params![now, packet_id],
        )?;
        tx.commit()?;
    }
    refresh_manifest(db, storage, packet_id)
}

/// Release a packet back to READY (routing undone; no code rollback implied).
pub fn release(db: &mut Db, storage: &Storage, packet_id: &str) -> Result<()> {
    let now = now_ms();
    {
        let tx = db
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let packet = get_packet(&tx, packet_id)?
            .ok_or_else(|| CoreError::PacketNotFound(packet_id.into()))?;
        match packet.state {
            PacketState::Claimed => {
                transition_locked(
                    &tx,
                    packet_id,
                    PacketState::Claimed,
                    PacketState::Ready,
                    now,
                )?;
            }
            PacketState::Processing => {
                transition_locked(
                    &tx,
                    packet_id,
                    PacketState::Processing,
                    PacketState::Ready,
                    now,
                )?;
            }
            PacketState::Ready => {
                tx.commit()?;
                return Ok(());
            }
            other => {
                return Err(CoreError::StateChanged {
                    expected: PacketState::Claimed,
                    actual: other,
                });
            }
        }
        tx.execute(
            "UPDATE claims SET status='released', released_at_ms=?1
             WHERE packet_id=?2 AND status='active'",
            params![now, packet_id],
        )?;
        tx.commit()?;
    }
    refresh_manifest(db, storage, packet_id)
}

/// Undo the most recent eligible claim for a session, returning the packet to
/// READY. "Eligible" = same session, claimed within `window_ms`, packet still
/// CLAIMED or PROCESSING (never CONSUMED). Undo affects routing only.
pub fn undo(
    db: &mut Db,
    storage: &Storage,
    session_id: &str,
    window_ms: i64,
) -> Result<UndoResult> {
    let now = now_ms();
    let cutoff = now - window_ms;
    let packet_id = {
        let tx = db
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Most recent active claim for this session within the window whose
        // packet is safe to return to READY.
        let candidate: Option<String> = tx
            .query_row(
                "SELECT c.packet_id
                 FROM claims c JOIN packets p ON p.id = c.packet_id
                 WHERE c.session_id = ?1
                   AND c.status = 'active'
                   AND c.claimed_at_ms >= ?2
                   AND p.state IN ('CLAIMED','PROCESSING')
                 ORDER BY c.claimed_at_ms DESC LIMIT 1",
                params![session_id, cutoff],
                |r| r.get(0),
            )
            .optional()?;
        let Some(pid) = candidate else {
            tx.commit()?;
            return Err(CoreError::NothingToUndo);
        };
        let packet =
            get_packet(&tx, &pid)?.ok_or_else(|| CoreError::PacketNotFound(pid.clone()))?;
        let expected = packet.state;
        transition_locked(&tx, &pid, expected, PacketState::Ready, now)?;
        tx.execute(
            "UPDATE claims SET status='released', released_at_ms=?1
             WHERE packet_id=?2 AND status='active'",
            params![now, pid],
        )?;
        tx.commit()?;
        pid
    };

    refresh_manifest(db, storage, &packet_id)?;
    let item_count = count_items(&db.conn, &packet_id)?;
    let project_name: String = db
        .conn
        .query_row(
            "SELECT project_name FROM claims WHERE packet_id=?1 ORDER BY claimed_at_ms DESC LIMIT 1",
            params![packet_id],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_default();
    Ok(UndoResult {
        packet_id,
        project_name,
        item_count,
    })
}

/// Undo the most recent dispatch regardless of session (used by the desktop
/// "Undo" button). Selection, eligibility, and release happen in ONE
/// transaction, guarded by the exact claim id, so it can never release a
/// different claim that re-used the same packet in the meantime.
pub fn undo_last_dispatch(db: &mut Db, storage: &Storage, window_ms: i64) -> Result<UndoResult> {
    let now = now_ms();
    let cutoff = now - window_ms;
    let (packet_id, project_name) = {
        let tx = db
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Bind to the ACTUAL latest dispatch (regardless of state), then check
        // whether THAT one is eligible. Never fall back to an older dispatch —
        // clicking "Undo" must only ever undo the most recent dispatch.
        let latest: Option<(String, String, String, i64, String)> = tx
            .query_row(
                "SELECT c.id, c.packet_id, c.project_name, c.claimed_at_ms, p.state
                 FROM claims c JOIN packets p ON p.id = c.packet_id
                 ORDER BY c.claimed_at_ms DESC LIMIT 1",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((claim_id, pid, project_name, claimed_at_ms, state_str)) = latest else {
            tx.commit()?;
            return Err(CoreError::NothingToUndo);
        };
        // Eligible only if it is still the active claim, within the window, and
        // the packet is CLAIMED/PROCESSING.
        let eligible = claimed_at_ms >= cutoff
            && matches!(
                PacketState::parse(&state_str),
                Some(PacketState::Claimed) | Some(PacketState::Processing)
            )
            && active_claim_is_owned_id(&tx, &pid, &claim_id)?;
        if !eligible {
            tx.commit()?;
            return Err(CoreError::NothingToUndo);
        };
        let packet =
            get_packet(&tx, &pid)?.ok_or_else(|| CoreError::PacketNotFound(pid.clone()))?;
        transition_locked(&tx, &pid, packet.state, PacketState::Ready, now)?;
        // Release exactly the claim we selected (guarded by its id).
        let n = tx.execute(
            "UPDATE claims SET status='released', released_at_ms=?1
             WHERE id=?2 AND status='active'",
            params![now, claim_id],
        )?;
        if n != 1 {
            return Err(CoreError::NothingToUndo);
        }
        tx.commit()?;
        (pid, project_name)
    };
    refresh_manifest(db, storage, &packet_id)?;
    let item_count = count_items(&db.conn, &packet_id)?;
    Ok(UndoResult {
        packet_id,
        project_name,
        item_count,
    })
}

// ---- Read models --------------------------------------------------------

/// A high-level status snapshot (counts and pointers only, no content).
pub fn status(db: &Db) -> Result<StatusReport> {
    let current_draft = match current_draft_id(db)? {
        Some(id) => packet_summary(db, &id)?,
        None => None,
    };
    let ready_count: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM packets WHERE state='READY'",
        [],
        |r| r.get(0),
    )?;
    let total_packets: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM packets", [], |r| r.get(0))?;
    let last_dispatch = last_dispatch(db)?;
    Ok(StatusReport {
        current_draft,
        ready_count,
        last_dispatch,
        total_packets,
    })
}

/// The most recent dispatch (claim), if any.
pub fn last_dispatch(db: &Db) -> Result<Option<LastDispatch>> {
    let row = db
        .conn
        .query_row(
            "SELECT c.packet_id, c.project_name, c.session_id, c.claimed_at_ms, p.state
             FROM claims c JOIN packets p ON p.id = c.packet_id
             ORDER BY c.claimed_at_ms DESC LIMIT 1",
            [],
            |r| {
                let state_str: String = r.get(4)?;
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    state_str,
                ))
            },
        )
        .optional()?;
    match row {
        None => Ok(None),
        Some((packet_id, project_name, session_id, claimed_at_ms, state_str)) => {
            let item_count = count_items(&db.conn, &packet_id)?;
            Ok(Some(LastDispatch {
                packet_id,
                project_name,
                session_id,
                item_count,
                claimed_at_ms,
                packet_state: PacketState::parse(&state_str).unwrap_or(PacketState::Failed),
            }))
        }
    }
}

/// The most recent items of a packet (metadata only), newest last, capped at
/// `limit`. Used by the desktop to show a short "recent items" list.
pub fn recent_items(db: &Db, packet_id: &str, limit: usize) -> Result<Vec<PacketItem>> {
    let mut items = list_items(&db.conn, packet_id)?;
    if items.len() > limit {
        items = items.split_off(items.len() - limit);
    }
    Ok(items)
}

/// A metadata summary for a single packet.
pub fn packet_summary(db: &Db, packet_id: &str) -> Result<Option<PacketSummary>> {
    let Some(p) = get_packet(&db.conn, packet_id)? else {
        return Ok(None);
    };
    Ok(Some(PacketSummary {
        item_count: count_items(&db.conn, packet_id)?,
        id: p.id,
        state: p.state,
        total_bytes: p.total_bytes,
        created_at_ms: p.created_at_ms,
        updated_at_ms: p.updated_at_ms,
    }))
}

/// List all packets (newest first) as metadata summaries.
pub fn list(db: &Db) -> Result<Vec<PacketSummary>> {
    let mut stmt = db.conn.prepare(
        "SELECT id, state, created_at_ms, updated_at_ms, total_bytes
         FROM packets ORDER BY created_at_ms DESC",
    )?;
    let ids: Vec<(String, String, i64, i64, i64)> = stmt
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut out = Vec::with_capacity(ids.len());
    for (id, state, created, updated, bytes) in ids {
        let item_count = count_items(&db.conn, &id)?;
        out.push(PacketSummary {
            id,
            state: PacketState::parse(&state).unwrap_or(PacketState::Failed),
            item_count,
            total_bytes: bytes,
            created_at_ms: created,
            updated_at_ms: updated,
        });
    }
    Ok(out)
}

// ---- Internals ----------------------------------------------------------

/// Regenerate `manifest.json` from the database (the authority). Claim metadata
/// is included only once the packet is CLAIMED or beyond.
pub fn refresh_manifest(db: &mut Db, storage: &Storage, packet_id: &str) -> Result<()> {
    let Some(packet) = get_packet(&db.conn, packet_id)? else {
        return Ok(());
    };
    let items = list_items(&db.conn, packet_id)?;
    let claim = match packet.state {
        PacketState::Claimed | PacketState::Processing | PacketState::Consumed => {
            latest_claim_meta(&db.conn, packet_id)?
        }
        _ => None,
    };
    let manifest = Manifest::from_rows(&packet, &items, claim);

    // Monotonic guard: never let an OLDER projection overwrite a NEWER one. If a
    // manifest with a strictly newer `updatedAt` already exists (a concurrent
    // appender/claimer published newer state), skip this write. `updated_at_ms`
    // is monotonic per packet mutation, so this prevents stale clobbering across
    // processes without holding a cross-process lock.
    if let Ok(existing) = std::fs::read_to_string(storage.manifest_path(packet_id)) {
        if let Ok(m) = serde_json::from_str::<Manifest>(&existing) {
            if let Some(existing_ms) = crate::clock::rfc3339_to_ms(&m.updated_at) {
                if existing_ms > packet.updated_at_ms {
                    return Ok(());
                }
            }
        }
    }

    let json = manifest.to_json_pretty()?;
    storage.write_manifest_atomic(packet_id, &json)?;
    Ok(())
}

/// Whether the packet's *active* claim is owned by `session_id` (and, when a
/// `claim_id` is supplied, is that exact claim). Prevents a stale finish from
/// acting on a claim that re-used the same packet.
fn active_claim_is_owned(
    tx: &rusqlite::Transaction,
    packet_id: &str,
    session_id: &str,
    claim_id: Option<&str>,
) -> Result<bool> {
    let active: Option<(String, String)> = tx
        .query_row(
            "SELECT id, session_id FROM claims
             WHERE packet_id = ?1 AND status = 'active'
             ORDER BY claimed_at_ms DESC LIMIT 1",
            params![packet_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(match (&active, claim_id) {
        (Some((active_id, active_session)), Some(cid)) => {
            active_id == cid && active_session == session_id
        }
        (Some((_, active_session)), None) => active_session == session_id,
        (None, _) => false,
    })
}

/// Whether the claim with `claim_id` for `packet_id` is still the active claim.
fn active_claim_is_owned_id(
    tx: &rusqlite::Transaction,
    packet_id: &str,
    claim_id: &str,
) -> Result<bool> {
    let ok = tx.query_row(
        "SELECT 1 FROM claims WHERE id = ?1 AND packet_id = ?2 AND status = 'active' LIMIT 1",
        params![claim_id, packet_id],
        |_| Ok(()),
    );
    Ok(matches!(ok, Ok(())))
}

/// Conditional state transition within a held transaction. Validates the edge,
/// then updates only if the row is still in `expected` state.
fn transition_locked(
    tx: &rusqlite::Transaction,
    packet_id: &str,
    expected: PacketState,
    to: PacketState,
    now: i64,
) -> Result<()> {
    if !expected.can_transition_to(to) {
        return Err(CoreError::InvalidTransition { from: expected, to });
    }
    let n = tx.execute(
        "UPDATE packets SET state=?1, updated_at_ms=?2 WHERE id=?3 AND state=?4",
        params![to.as_str(), now, packet_id, expected.as_str()],
    )?;
    if n != 1 {
        let actual = get_packet(tx, packet_id)?
            .map(|p| p.state)
            .ok_or_else(|| CoreError::PacketNotFound(packet_id.into()))?;
        return Err(CoreError::StateChanged { expected, actual });
    }
    Ok(())
}
