//! SQLite-backed, atomic run/event history. Raw prompts and answers are omitted.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use super::{HarnessEvent, HarnessJournal, HarnessRun};
use crate::core::error::{ContribError, Result};

/// A journal may share the configured ContribAI SQLite database. Its tables are
/// separate from contribution admission evidence and cannot authorize writes.
pub struct HarnessStore(Mutex<Connection>);

impl HarnessStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        Self::from_connection(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self> {
        connection.busy_timeout(std::time::Duration::from_secs(2))?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS harness_runs (id TEXT PRIMARY KEY, started_at TEXT NOT NULL, receipt TEXT NOT NULL); CREATE TABLE IF NOT EXISTS harness_events (run_id TEXT NOT NULL, sequence INTEGER NOT NULL, event TEXT NOT NULL, PRIMARY KEY (run_id, sequence));")?;
        Ok(Self(Mutex::new(connection)))
    }

    /// Newest runs first; a stale `running` receipt means completion is unknown.
    pub fn list(&self, limit: usize) -> Result<Vec<HarnessRun>> {
        let connection = self
            .0
            .lock()
            .map_err(|_| ContribError::Database("harness journal unavailable".into()))?;
        let mut query = connection.prepare(
            "SELECT receipt FROM harness_runs ORDER BY started_at DESC, id DESC LIMIT ?1",
        )?;
        let rows = query.query_map(params![limit.min(100) as i64], |row| {
            row.get::<_, String>(0)
        })?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    /// Return the exact stored metadata and ordered events without a model call.
    pub fn inspect(&self, id: &str) -> Result<Option<(HarnessRun, Vec<HarnessEvent>)>> {
        let mut connection = self
            .0
            .lock()
            .map_err(|_| ContribError::Database("harness journal unavailable".into()))?;
        let transaction = connection.transaction()?;
        let receipt: Option<String> = transaction
            .query_row(
                "SELECT receipt FROM harness_runs WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(receipt) = receipt else {
            return Ok(None);
        };
        let mut query = transaction
            .prepare("SELECT event FROM harness_events WHERE run_id = ?1 ORDER BY sequence")?;
        let rows = query.query_map(params![id], |row| row.get::<_, String>(0))?;
        let events = rows
            .map(|row| Ok(serde_json::from_str(&row?)?))
            .collect::<Result<Vec<_>>>()?;
        Ok(Some((serde_json::from_str(&receipt)?, events)))
    }
}

impl HarnessJournal for HarnessStore {
    fn record(&self, run: &HarnessRun, event: &HarnessEvent) -> Result<()> {
        let mut connection = self
            .0
            .lock()
            .map_err(|_| ContribError::Database("harness journal unavailable".into()))?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO harness_events (run_id, sequence, event) VALUES (?1, ?2, ?3)",
            params![run.id, event.sequence, serde_json::to_string(event)?],
        )?;
        transaction.execute("INSERT INTO harness_runs (id, started_at, receipt) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET receipt = excluded.receipt", params![run.id, run.started_at.to_rfc3339(), serde_json::to_string(run)?])?;
        transaction.commit()?;
        Ok(())
    }
}
