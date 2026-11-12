// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Signer state (SQLite): reservations and their slots, used nonces, the audit log.
//! The audit log holds hosts, routes, ids and outcomes only: there is no user data to hold (D6, T6).

use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Mutex;

pub struct Store {
    db: Mutex<Connection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reservation {
    pub id: String,
    pub host: String,
    pub template_id: String,
    pub bundle_version: u32,
    pub params_json: String,
    pub sequence_params_json: Option<String>,
    pub sequence_json: Option<String>,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotUse {
    pub body_sha256: String,
    pub manifest_uuid: String,
}

/// Row counts per request-log table (what a prune removed, or what is there).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pruned {
    pub audit: u64,
    pub reservations: u64,
    pub slots: u64,
}

/// The oldest moment the request log keeps, `days` before `now`.
pub fn request_log_cutoff(now: u64, days: u64) -> u64 {
    now.saturating_sub(days.saturating_mul(86_400))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NonceOutcome {
    Fresh,
    Replayed,
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Store> {
        let c = Connection::open(path)?;
        Self::init(c)
    }

    pub fn in_memory() -> anyhow::Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(c: Connection) -> anyhow::Result<Store> {
        c.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
             CREATE TABLE IF NOT EXISTS reservations (
               id TEXT PRIMARY KEY, host TEXT NOT NULL, template_id TEXT NOT NULL, bundle_version INTEGER NOT NULL,
               params_json TEXT NOT NULL, sequence_params_json TEXT, sequence_json TEXT,
               created_at INTEGER NOT NULL, expires_at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS slots (
               reservation_id TEXT NOT NULL, slot TEXT NOT NULL, body_sha256 TEXT NOT NULL, manifest_uuid TEXT NOT NULL,
               signed_at INTEGER NOT NULL, PRIMARY KEY (reservation_id, slot));
             CREATE TABLE IF NOT EXISTS nonces (host TEXT NOT NULL, nonce TEXT NOT NULL, seen_at INTEGER NOT NULL,
               PRIMARY KEY (host, nonce));
             CREATE TABLE IF NOT EXISTS audit (
               ts INTEGER NOT NULL, host TEXT, route TEXT NOT NULL, reservation_id TEXT, slot TEXT,
               manifest_uuid TEXT, outcome TEXT NOT NULL, detail TEXT);
             CREATE INDEX IF NOT EXISTS audit_ts ON audit (ts);
             CREATE INDEX IF NOT EXISTS nonces_seen ON nonces (seen_at);
             CREATE INDEX IF NOT EXISTS nonces_host ON nonces (host, seen_at);
             CREATE INDEX IF NOT EXISTS reservations_host ON reservations (host, created_at);",
        )?;
        Ok(Store { db: Mutex::new(c) })
    }

    /// Consume a nonce for an authentic, allow-listed host (called last in auth). Forgets nonces older
    /// than `memory_secs`.
    pub fn use_nonce(
        &self,
        host: &str,
        nonce: &str,
        now: u64,
        memory_secs: u64,
    ) -> anyhow::Result<NonceOutcome> {
        let c = self.db.lock().unwrap_or_else(|e| e.into_inner());
        c.execute(
            "DELETE FROM nonces WHERE seen_at < ?1",
            params![now.saturating_sub(memory_secs) as i64],
        )?;
        let n = c.execute(
            "INSERT OR IGNORE INTO nonces (host, nonce, seen_at) VALUES (?1, ?2, ?3)",
            params![host, nonce, now as i64],
        )?;
        Ok(if n == 1 {
            NonceOutcome::Fresh
        } else {
            NonceOutcome::Replayed
        })
    }

    pub fn insert_reservation(&self, r: &Reservation) -> anyhow::Result<()> {
        let c = self.db.lock().unwrap_or_else(|e| e.into_inner());
        c.execute(
            "INSERT INTO reservations (id, host, template_id, bundle_version, params_json, sequence_params_json, sequence_json, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![r.id, r.host, r.template_id, r.bundle_version, r.params_json, r.sequence_params_json, r.sequence_json, r.created_at as i64, r.expires_at as i64],
        )?;
        Ok(())
    }

    pub fn reservation(&self, id: &str) -> anyhow::Result<Option<Reservation>> {
        let c = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(c.query_row(
            "SELECT id, host, template_id, bundle_version, params_json, sequence_params_json, sequence_json, created_at, expires_at FROM reservations WHERE id = ?1",
            params![id],
            |r| {
                Ok(Reservation {
                    id: r.get(0)?,
                    host: r.get(1)?,
                    template_id: r.get(2)?,
                    bundle_version: r.get(3)?,
                    params_json: r.get(4)?,
                    sequence_params_json: r.get(5)?,
                    sequence_json: r.get(6)?,
                    created_at: r.get::<_, i64>(7)? as u64,
                    expires_at: r.get::<_, i64>(8)? as u64,
                })
            },
        )
        .optional()?)
    }

    pub fn slot(&self, reservation_id: &str, slot: &str) -> anyhow::Result<Option<SlotUse>> {
        let c = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(c.query_row(
            "SELECT body_sha256, manifest_uuid FROM slots WHERE reservation_id = ?1 AND slot = ?2",
            params![reservation_id, slot],
            |r| {
                Ok(SlotUse {
                    body_sha256: r.get(0)?,
                    manifest_uuid: r.get(1)?,
                })
            },
        )
        .optional()?)
    }

    /// Mark a slot signed. Returns false if it was already consumed (the caller lost a race).
    pub fn consume_slot(
        &self,
        reservation_id: &str,
        slot: &str,
        body_sha256: &str,
        manifest_uuid: &str,
        now: u64,
    ) -> anyhow::Result<bool> {
        let c = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let n = c.execute(
            "INSERT OR IGNORE INTO slots (reservation_id, slot, body_sha256, manifest_uuid, signed_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![reservation_id, slot, body_sha256, manifest_uuid, now as i64],
        )?;
        Ok(n == 1)
    }

    /// Authenticated requests by a host in the last `window` seconds (each consumed one nonce).
    pub fn recent_nonces(&self, host: &str, now: u64, window: u64) -> anyhow::Result<u64> {
        let c = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let n: i64 = c.query_row(
            "SELECT COUNT(*) FROM nonces WHERE host = ?1 AND seen_at >= ?2",
            params![host, now.saturating_sub(window) as i64],
            |r| r.get(0),
        )?;
        Ok(n as u64)
    }

    /// Nonce rows for a host (tests: an unlisted caller must leave none).
    pub fn nonce_count(&self, host: &str) -> anyhow::Result<u64> {
        let c = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let n: i64 = c.query_row(
            "SELECT COUNT(*) FROM nonces WHERE host = ?1",
            params![host],
            |r| r.get(0),
        )?;
        Ok(n as u64)
    }

    /// Reservations issued by a host in the last `window` seconds (rate limit).
    pub fn recent_reservations(&self, host: &str, now: u64, window: u64) -> anyhow::Result<u64> {
        let c = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let n: i64 = c.query_row(
            "SELECT COUNT(*) FROM reservations WHERE host = ?1 AND created_at >= ?2",
            params![host, now.saturating_sub(window) as i64],
            |r| r.get(0),
        )?;
        Ok(n as u64)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn audit(
        &self,
        ts: u64,
        host: Option<&str>,
        route: &str,
        reservation_id: Option<&str>,
        slot: Option<&str>,
        manifest_uuid: Option<&str>,
        outcome: &str,
        detail: Option<&str>,
    ) {
        let c = self.db.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = c.execute(
            "INSERT INTO audit (ts, host, route, reservation_id, slot, manifest_uuid, outcome, detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![ts as i64, host, route, reservation_id, slot, manifest_uuid, outcome, detail],
        ) {
            tracing::error!("audit write failed: {e}");
        }
    }

    /// Retention: forget the LOG OF REQUESTS older than `cutoff` (Unix seconds). That is three tables, the only
    /// places that tie a host to a signature: `audit` (one row per request), `reservations` that expired before
    /// the cutoff, and the `slots` rows of those reservations (which reservation produced which manifest). Rows
    /// are overwritten, not just unlinked. NOT touched: the repository of records (manifests, fingerprints and
    /// frame indexes, which detection and `/verify` need for as long as the outputs exist) and `nonces` (which
    /// forgets by itself within minutes).
    pub fn prune_request_log(&self, cutoff: u64) -> anyhow::Result<Pruned> {
        let mut c = self.db.lock().unwrap_or_else(|e| e.into_inner());
        c.execute_batch("PRAGMA secure_delete=ON;")?;
        let tx = c.transaction()?;
        let cutoff = cutoff.min(i64::MAX as u64) as i64;
        let audit = tx.execute("DELETE FROM audit WHERE ts < ?1", params![cutoff])?;
        // a slot goes only with its reservation, and only when it is itself older than the cutoff
        let slots = tx.execute(
            "DELETE FROM slots WHERE signed_at < ?1 AND reservation_id IN
               (SELECT id FROM reservations WHERE expires_at < ?1)",
            params![cutoff],
        )?;
        let reservations = tx.execute(
            "DELETE FROM reservations WHERE expires_at < ?1
               AND id NOT IN (SELECT reservation_id FROM slots)",
            params![cutoff],
        )?;
        tx.commit()?;
        Ok(Pruned {
            audit: audit as u64,
            reservations: reservations as u64,
            slots: slots as u64,
        })
    }

    /// Rows in one of the request-log tables (tests and the prune log line).
    pub fn request_log_rows(&self) -> anyhow::Result<Pruned> {
        let c = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let n = |t: &str| -> anyhow::Result<u64> {
            Ok(c.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| {
                r.get::<_, i64>(0)
            })? as u64)
        };
        Ok(Pruned {
            audit: n("audit")?,
            reservations: n("reservations")?,
            slots: n("slots")?,
        })
    }

    /// mp4 signatures per host in [from, to) (daily reconciliation, D11).
    pub fn signatures_per_host(&self, from: u64, to: u64) -> anyhow::Result<Vec<(String, u64)>> {
        let c = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut st = c.prepare("SELECT host, COUNT(*) FROM audit WHERE route = '/v1/sign' AND outcome = 'signed' AND ts >= ?1 AND ts < ?2 GROUP BY host ORDER BY host")?;
        let rows = st.query_map(params![from as i64, to as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}
