//! TTL cleanup.
//!
//! Triggered on app startup and when a new capture starts — never by a
//! scheduler, cron, or background service. DRAFT and PROCESSING packets are
//! always protected from age-based deletion (they are actively in use or
//! recoverable). Stale READY/CLAIMED/FAILED packets are marked EXPIRED and then
//! removed together with terminal packets past the TTL.

use rusqlite::params;

use crate::database::Db;
use crate::error::Result;
use crate::packet::PacketState;
use crate::storage::Storage;

/// What a cleanup pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanupReport {
    /// Packet ids that were deleted (files + rows).
    pub removed: Vec<String>,
    /// Packet ids that were transitioned to EXPIRED before removal.
    pub expired: Vec<String>,
    /// Orphaned packet directories removed (files present with no DB row),
    /// e.g. from a crash between row deletion and file removal.
    pub orphans_removed: Vec<String>,
}

/// Whether a packet is eligible for age-based cleanup.
///
/// Pure function so it can be unit-tested exhaustively. DRAFT and PROCESSING
/// are never eligible regardless of age.
pub fn is_eligible_for_cleanup(
    state: PacketState,
    updated_at_ms: i64,
    now_ms: i64,
    ttl_ms: i64,
) -> bool {
    if matches!(state, PacketState::Draft | PacketState::Processing) {
        return false;
    }
    let age = now_ms.saturating_sub(updated_at_ms);
    age >= ttl_ms
}

/// Run a cleanup pass for the given TTL (in hours). Returns what was removed.
pub fn cleanup(
    db: &mut Db,
    storage: &Storage,
    ttl_hours: u32,
    now_ms_val: i64,
) -> Result<CleanupReport> {
    let ttl_ms = (ttl_hours as i64) * 3600 * 1000;
    let mut report = CleanupReport::default();

    // Gather candidates first (read-only), then act. Only non-active states are
    // ever considered, and DRAFT/PROCESSING are excluded by the query.
    let candidates: Vec<(String, PacketState, i64)> = {
        let mut stmt = db.conn.prepare(
            "SELECT id, state, updated_at_ms FROM packets
             WHERE state IN ('READY','CLAIMED','FAILED','CONSUMED','EXPIRED')",
        )?;
        let rows = stmt.query_map([], |r| {
            let state_str: String = r.get(1)?;
            Ok((r.get::<_, String>(0)?, state_str, r.get::<_, i64>(2)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, state_str, updated) = row?;
            if let Some(state) = PacketState::parse(&state_str) {
                out.push((id, state, updated));
            }
        }
        out
    };

    for (id, state, updated) in candidates {
        if !is_eligible_for_cleanup(state, updated, now_ms_val, ttl_ms) {
            continue;
        }
        // Delete atomically, guarded by the exact (state, updated_at_ms) we
        // observed. Any concurrent change — most importantly a claim, which
        // bumps state to CLAIMED and updated_at to "now" — makes the guard fail,
        // so a just-dispatched packet is never deleted out from under a session.
        let deleted = {
            let tx = db
                .conn
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let n = tx.execute(
                "DELETE FROM packets WHERE id = ?1 AND state = ?2 AND updated_at_ms = ?3",
                params![id, state.as_str(), updated],
            )?;
            tx.commit()?;
            n == 1
        };
        if deleted {
            if matches!(
                state,
                PacketState::Ready | PacketState::Claimed | PacketState::Failed
            ) {
                // These stale-but-non-terminal states are conceptually EXPIRED
                // before removal.
                report.expired.push(id.clone());
            }
            storage.remove_packet_dir(&id)?;
            report.removed.push(id);
        }
    }

    // Self-heal: remove orphaned packet directories that have no DB row (e.g. a
    // crash or failed removal left files behind).
    report.orphans_removed = sweep_orphan_dirs(db, storage)?;

    Ok(report)
}

/// Remove packet directories under `packets/` whose id has no `packets` row.
///
/// Race-safe by construction: `create_draft` commits the packet row BEFORE it
/// creates the directory, so a directory that exists always has (or had) a row.
/// We therefore re-check the DB **per directory, freshly**, right before
/// deleting — a packet created concurrently already has its row and is skipped;
/// only a directory whose row was truly deleted is removed.
fn sweep_orphan_dirs(db: &Db, storage: &Storage) -> Result<Vec<String>> {
    let packets_dir = storage.packets_dir();
    if !packets_dir.exists() {
        return Ok(Vec::new());
    }
    let mut has_row = db
        .conn
        .prepare("SELECT 1 FROM packets WHERE id = ?1 LIMIT 1")?;
    let mut removed = Vec::new();
    for entry in std::fs::read_dir(&packets_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if has_row.exists(params![name])? {
            continue; // a live (or concurrently-created) packet — never touch it
        }
        // Best-effort: ignore removal errors (will retry next cleanup).
        let _ = std::fs::remove_dir_all(entry.path());
        removed.push(name);
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR_MS: i64 = 3600 * 1000;

    #[test]
    fn draft_and_processing_never_expire_by_age() {
        // Even at extreme age.
        let ancient = 0;
        let now = 1_000 * 24 * HOUR_MS;
        assert!(!is_eligible_for_cleanup(
            PacketState::Draft,
            ancient,
            now,
            24 * HOUR_MS
        ));
        assert!(!is_eligible_for_cleanup(
            PacketState::Processing,
            ancient,
            now,
            24 * HOUR_MS
        ));
    }

    #[test]
    fn terminal_and_stale_states_expire_past_ttl() {
        let ttl = 24 * HOUR_MS;
        let now = 100 * HOUR_MS;
        // 25h old > 24h ttl.
        let old = now - 25 * HOUR_MS;
        for s in [
            PacketState::Consumed,
            PacketState::Failed,
            PacketState::Expired,
            PacketState::Ready,
            PacketState::Claimed,
        ] {
            assert!(
                is_eligible_for_cleanup(s, old, now, ttl),
                "{s} should expire"
            );
        }
    }

    #[test]
    fn fresh_packets_are_kept() {
        let ttl = 24 * HOUR_MS;
        let now = 100 * HOUR_MS;
        let fresh = now - HOUR_MS; // 1h old
        for s in [
            PacketState::Consumed,
            PacketState::Ready,
            PacketState::Claimed,
        ] {
            assert!(!is_eligible_for_cleanup(s, fresh, now, ttl));
        }
    }
}
