//! SQLite access: connection setup, schema migrations, settings, and the
//! low-level row helpers shared by claim/cleanup.
//!
//! SQLite is the metadata authority. We open in WAL mode with a busy timeout so
//! the desktop app and multiple concurrent CLI invocations can coordinate
//! purely through the database + filesystem — no daemon, no network listener.

use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::clock::now_ms;
use crate::error::Result;
use crate::manifest::ClaimMeta;
use crate::packet::{ItemKind, Packet, PacketItem, PacketState};
use crate::settings::Settings;

/// The latest schema version this build knows how to produce.
pub const SCHEMA_VERSION: u32 = 2;

/// Busy timeout for lock contention (concurrent claims/appends).
const BUSY_TIMEOUT: Duration = Duration::from_millis(5_000);

/// A database handle. Wraps a single connection; open one per thread/process.
pub struct Db {
    pub(crate) conn: Connection,
}

/// Restrict the database (and its WAL/SHM sidecars) to owner-only (0600).
#[cfg(unix)]
fn secure_db_files(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    for suffix in ["", "-wal", "-shm"] {
        let p = if suffix.is_empty() {
            path.to_path_buf()
        } else {
            let mut s = path.as_os_str().to_owned();
            s.push(suffix);
            std::path::PathBuf::from(s)
        };
        if let Ok(md) = std::fs::metadata(&p) {
            let mut perms = md.permissions();
            perms.set_mode(0o600);
            let _ = std::fs::set_permissions(&p, perms);
        }
    }
}

impl Db {
    /// Open (creating if needed) the database at `path`, configure pragmas, and
    /// run migrations. Parent directories must already exist.
    pub fn open(path: impl AsRef<Path>) -> Result<Db> {
        let path = path.as_ref();
        let conn = Connection::open(path)?;
        Self::configure(&conn)?;
        let mut db = Db { conn };
        db.migrate()?;
        #[cfg(unix)]
        secure_db_files(path);
        Ok(db)
    }

    /// Open a private in-memory database (used by fast unit tests that do not
    /// exercise cross-connection concurrency).
    pub fn open_in_memory() -> Result<Db> {
        let conn = Connection::open_in_memory()?;
        Self::configure(&conn)?;
        let mut db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    fn configure(conn: &Connection) -> Result<()> {
        conn.busy_timeout(BUSY_TIMEOUT)?;
        // WAL enables concurrent readers with a writer; NORMAL sync is the
        // recommended durability/speed balance under WAL.
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA foreign_keys=ON;",
        )?;
        Ok(())
    }

    /// The current on-disk schema version (`PRAGMA user_version`).
    pub fn schema_version(&self) -> Result<u32> {
        let v: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        Ok(v as u32)
    }

    /// Apply any pending migrations. Idempotent.
    fn migrate(&mut self) -> Result<()> {
        let current = self.schema_version()?;
        if current < 1 {
            self.apply_v1()?;
        }
        if current < 2 {
            self.apply_v2()?;
        }
        Ok(())
    }

    /// v2: per-claim note (the user's pull instruction) and terminal label, for
    /// the desktop dispatch history. Nullable, so older rows stay valid.
    fn apply_v2(&mut self) -> Result<()> {
        // IMMEDIATE + re-check: the app and a CLI may open the DB at the same
        // time; only the first may add the columns (a second ALTER would fail).
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let v: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v >= 2 {
            return Ok(());
        }
        tx.execute_batch(
            "ALTER TABLE claims ADD COLUMN note TEXT;
             ALTER TABLE claims ADD COLUMN terminal TEXT;",
        )?;
        tx.execute(
            "INSERT INTO migrations(version, applied_at_ms) VALUES (2, ?1)",
            params![now_ms()],
        )?;
        tx.pragma_update(None, "user_version", 2)?;
        tx.commit()?;
        Ok(())
    }

    fn apply_v1(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS migrations (
                version       INTEGER PRIMARY KEY,
                applied_at_ms INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS packets (
                id                    TEXT PRIMARY KEY,
                state                 TEXT NOT NULL,
                created_at_ms         INTEGER NOT NULL,
                updated_at_ms         INTEGER NOT NULL,
                last_snapshot_sha256  TEXT,
                total_bytes           INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_packets_state_created
                ON packets(state, created_at_ms);

            CREATE TABLE IF NOT EXISTS packet_items (
                id            TEXT PRIMARY KEY,
                packet_id     TEXT NOT NULL REFERENCES packets(id) ON DELETE CASCADE,
                seq           INTEGER NOT NULL,
                kind          TEXT NOT NULL,
                mime_type     TEXT NOT NULL,
                relative_path TEXT NOT NULL,
                byte_size     INTEGER NOT NULL,
                sha256        TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                UNIQUE(packet_id, seq)
            );
            CREATE INDEX IF NOT EXISTS idx_items_packet
                ON packet_items(packet_id, seq);

            CREATE TABLE IF NOT EXISTS claims (
                id             TEXT PRIMARY KEY,
                packet_id      TEXT NOT NULL REFERENCES packets(id) ON DELETE CASCADE,
                session_id     TEXT NOT NULL,
                cwd            TEXT NOT NULL,
                project_root   TEXT NOT NULL,
                project_name   TEXT NOT NULL,
                config_dir     TEXT,
                claimed_at_ms  INTEGER NOT NULL,
                released_at_ms INTEGER,
                status         TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_claims_session
                ON claims(session_id, claimed_at_ms);
            CREATE INDEX IF NOT EXISTS idx_claims_packet
                ON claims(packet_id);

            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            "#,
        )?;
        tx.execute(
            "INSERT INTO migrations(version, applied_at_ms) VALUES (1, ?1)",
            params![now_ms()],
        )?;
        tx.pragma_update(None, "user_version", 1)?;
        tx.commit()?;
        Ok(())
    }

    // ---- Settings -------------------------------------------------------

    /// Load settings, filling any missing keys with defaults.
    pub fn load_settings(&self) -> Result<Settings> {
        let mut s = Settings::default();
        let mut stmt = self.conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (k, v) = row?;
            match k.as_str() {
                "globalShortcut" => s.global_shortcut = v,
                "packetTtlHours" => s.packet_ttl_hours = v.parse().unwrap_or(s.packet_ttl_hours),
                "maxItemBytes" => s.max_item_bytes = v.parse().unwrap_or(s.max_item_bytes),
                "maxPacketBytes" => s.max_packet_bytes = v.parse().unwrap_or(s.max_packet_bytes),
                "autoCleanup" => s.auto_cleanup = v == "true",
                "shortAliasInstalled" => s.short_alias_installed = v == "true",
                _ => {}
            }
        }
        Ok(s.sanitized())
    }

    /// Persist settings (upsert of each key). Values are sanitized first.
    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        let s = settings.clone().sanitized();
        let pairs = [
            ("globalShortcut", s.global_shortcut.clone()),
            ("packetTtlHours", s.packet_ttl_hours.to_string()),
            ("maxItemBytes", s.max_item_bytes.to_string()),
            ("maxPacketBytes", s.max_packet_bytes.to_string()),
            ("autoCleanup", s.auto_cleanup.to_string()),
            ("shortAliasInstalled", s.short_alias_installed.to_string()),
        ];
        for (k, v) in pairs {
            self.conn.execute(
                "INSERT INTO settings(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![k, v],
            )?;
        }
        Ok(())
    }
}

// ---- Shared row helpers (used by claim.rs / cleanup.rs) -----------------

/// Map a `packets` row.
pub(crate) fn map_packet(row: &rusqlite::Row) -> rusqlite::Result<Packet> {
    let state_str: String = row.get("state")?;
    Ok(Packet {
        id: row.get("id")?,
        state: PacketState::parse(&state_str).unwrap_or(PacketState::Failed),
        created_at_ms: row.get("created_at_ms")?,
        updated_at_ms: row.get("updated_at_ms")?,
        last_snapshot_sha256: row.get("last_snapshot_sha256")?,
        total_bytes: row.get("total_bytes")?,
    })
}

pub(crate) fn get_packet(conn: &Connection, id: &str) -> Result<Option<Packet>> {
    let p = conn
        .query_row(
            "SELECT id, state, created_at_ms, updated_at_ms, last_snapshot_sha256, total_bytes
             FROM packets WHERE id = ?1",
            params![id],
            map_packet,
        )
        .optional()?;
    Ok(p)
}

pub(crate) fn list_items(conn: &Connection, packet_id: &str) -> Result<Vec<PacketItem>> {
    let mut stmt = conn.prepare(
        "SELECT id, packet_id, seq, kind, mime_type, relative_path, byte_size, sha256, created_at_ms
         FROM packet_items WHERE packet_id = ?1 ORDER BY seq ASC",
    )?;
    let rows = stmt.query_map(params![packet_id], |row| {
        let kind: String = row.get("kind")?;
        Ok(PacketItem {
            id: row.get("id")?,
            packet_id: row.get("packet_id")?,
            seq: row.get("seq")?,
            kind: ItemKind::parse(&kind),
            mime_type: row.get("mime_type")?,
            relative_path: row.get("relative_path")?,
            byte_size: row.get("byte_size")?,
            sha256: row.get("sha256")?,
            created_at_ms: row.get("created_at_ms")?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub(crate) fn count_items(conn: &Connection, packet_id: &str) -> Result<i64> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM packet_items WHERE packet_id = ?1",
        params![packet_id],
        |r| r.get(0),
    )?;
    Ok(n)
}

pub(crate) fn next_seq(conn: &Connection, packet_id: &str) -> Result<i64> {
    let max: Option<i64> = conn.query_row(
        "SELECT MAX(seq) FROM packet_items WHERE packet_id = ?1",
        params![packet_id],
        |r| r.get(0),
    )?;
    Ok(max.unwrap_or(0) + 1)
}

/// The most recent active claim's routing metadata for a packet, if any.
pub(crate) fn latest_claim_meta(conn: &Connection, packet_id: &str) -> Result<Option<ClaimMeta>> {
    let meta = conn
        .query_row(
            "SELECT session_id, cwd, project_root, project_name, config_dir, claimed_at_ms
             FROM claims WHERE packet_id = ?1
             ORDER BY claimed_at_ms DESC LIMIT 1",
            params![packet_id],
            |row| {
                let claimed_at_ms: i64 = row.get("claimed_at_ms")?;
                Ok(ClaimMeta {
                    session_id: row.get("session_id")?,
                    cwd: row.get("cwd")?,
                    project_root: row.get("project_root")?,
                    project_name: row.get("project_name")?,
                    config_dir: row.get("config_dir")?,
                    claimed_at: crate::clock::ms_to_rfc3339(claimed_at_ms),
                })
            },
        )
        .optional()?;
    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_sets_wal_and_schema_version() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        let mode: String = db
            .conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[test]
    fn migrations_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let _db = Db::open(&path).unwrap();
        }
        // Re-open: migration must not re-run or error.
        let db = Db::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, SCHEMA_VERSION as i64);
    }

    #[test]
    fn v1_database_upgrades_to_v2_keeping_claims() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            // Build a v1-only database (as written by app <= 0.1.5) with a claim.
            let conn = Connection::open(&path).unwrap();
            Db::configure(&conn).unwrap();
            let mut db = Db { conn };
            db.apply_v1().unwrap();
            db.conn
                .execute_batch(
                    "INSERT INTO packets(id, state, created_at_ms, updated_at_ms, total_bytes)
                     VALUES ('p1', 'CLAIMED', 1, 1, 0);
                     INSERT INTO claims(id, packet_id, session_id, cwd, project_root, project_name,
                                        config_dir, claimed_at_ms, released_at_ms, status)
                     VALUES ('c1', 'p1', 's1', '/r', '/r', 'r', NULL, 5, NULL, 'active');",
                )
                .unwrap();
            assert_eq!(db.schema_version().unwrap(), 1);
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), 2);
        let (note, terminal): (Option<String>, Option<String>) = db
            .conn
            .query_row("SELECT note, terminal FROM claims WHERE id='c1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((note, terminal), (None, None));
    }

    #[test]
    fn settings_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.load_settings().unwrap(), Settings::default());
        let s = Settings {
            packet_ttl_hours: 48,
            global_shortcut: "CommandOrControl+Alt+K".into(),
            short_alias_installed: true,
            ..Settings::default()
        };
        db.save_settings(&s).unwrap();
        let loaded = db.load_settings().unwrap();
        assert_eq!(loaded.packet_ttl_hours, 48);
        assert_eq!(loaded.global_shortcut, "CommandOrControl+Alt+K");
        assert!(loaded.short_alias_installed);
    }
}
