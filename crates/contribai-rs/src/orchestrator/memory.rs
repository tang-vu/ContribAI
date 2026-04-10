//! Persistent memory system using SQLite.
//!
//! Port from Python `orchestrator/memory.py`.
//! Tracks analyzed repos, submitted PRs, outcome learning,
//! and working memory with TTL.

use chrono::{Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tracing::info;

use crate::core::error::{ContribError, Result};

/// A single message in a PR conversation thread.
pub struct ConversationMessage {
    pub repo: String,
    pub pr_number: i64,
    /// "maintainer", "contribai", or "bot"
    pub role: String,
    pub author: String,
    pub body: String,
    pub comment_id: i64,
    pub is_inline: bool,
    pub file_path: Option<String>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS analyzed_repos (
    full_name   TEXT PRIMARY KEY,
    language    TEXT,
    stars       INTEGER,
    analyzed_at TEXT,
    findings    INTEGER DEFAULT 0,
    metadata    TEXT DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS submitted_prs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    repo        TEXT NOT NULL,
    pr_number   INTEGER NOT NULL,
    pr_url      TEXT NOT NULL,
    title       TEXT NOT NULL,
    type        TEXT NOT NULL,
    status      TEXT DEFAULT 'open',
    branch      TEXT,
    fork        TEXT,
    created_at  TEXT,
    updated_at  TEXT,
    UNIQUE(repo, pr_number)
);

CREATE TABLE IF NOT EXISTS findings_cache (
    id          TEXT PRIMARY KEY,
    repo        TEXT NOT NULL,
    type        TEXT NOT NULL,
    severity    TEXT NOT NULL,
    title       TEXT NOT NULL,
    file_path   TEXT,
    status      TEXT DEFAULT 'new',
    created_at  TEXT
);

CREATE TABLE IF NOT EXISTS run_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at  TEXT,
    finished_at TEXT,
    repos_analyzed INTEGER DEFAULT 0,
    prs_created  INTEGER DEFAULT 0,
    findings     INTEGER DEFAULT 0,
    errors       INTEGER DEFAULT 0,
    metadata     TEXT DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS pr_outcomes (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    repo        TEXT NOT NULL,
    pr_number   INTEGER NOT NULL,
    pr_url      TEXT NOT NULL,
    pr_type     TEXT NOT NULL,
    outcome     TEXT NOT NULL,
    feedback    TEXT DEFAULT '',
    time_to_close_hours REAL DEFAULT 0,
    recorded_at TEXT,
    UNIQUE(repo, pr_number)
);

CREATE TABLE IF NOT EXISTS repo_preferences (
    repo        TEXT PRIMARY KEY,
    preferred_types TEXT DEFAULT '[]',
    rejected_types  TEXT DEFAULT '[]',
    merge_rate  REAL DEFAULT 0.0,
    avg_review_hours REAL DEFAULT 0.0,
    notes       TEXT DEFAULT '',
    updated_at  TEXT
);

CREATE TABLE IF NOT EXISTS working_memory (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    repo        TEXT NOT NULL,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    language    TEXT DEFAULT '',
    created_at  TEXT,
    expires_at  TEXT,
    UNIQUE(repo, key)
);

CREATE TABLE IF NOT EXISTS dream_meta (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  TEXT
);

CREATE TABLE IF NOT EXISTS pr_conversations (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    repo        TEXT NOT NULL,
    pr_number   INTEGER NOT NULL,
    role        TEXT NOT NULL,
    author      TEXT NOT NULL,
    body        TEXT NOT NULL,
    comment_id  INTEGER DEFAULT 0,
    is_inline   INTEGER DEFAULT 0,
    file_path   TEXT,
    created_at  TEXT,
    UNIQUE(repo, pr_number, comment_id)
);

CREATE TABLE IF NOT EXISTS sessions (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    mode        TEXT NOT NULL DEFAULT 'build',
    status      TEXT NOT NULL DEFAULT 'running',
    created_at  TEXT NOT NULL
);

-- Indexes for hot query paths
CREATE INDEX IF NOT EXISTS idx_submitted_prs_created_at ON submitted_prs(created_at);
CREATE INDEX IF NOT EXISTS idx_submitted_prs_status ON submitted_prs(status);
CREATE INDEX IF NOT EXISTS idx_pr_conversations_repo_pr ON pr_conversations(repo, pr_number);
CREATE INDEX IF NOT EXISTS idx_working_memory_repo_key ON working_memory(repo, key);
CREATE INDEX IF NOT EXISTS idx_working_memory_expires ON working_memory(expires_at);
CREATE INDEX IF NOT EXISTS idx_pr_outcomes_repo ON pr_outcomes(repo);
CREATE INDEX IF NOT EXISTS idx_findings_cache_repo ON findings_cache(repo);
"#;

/// Persistent memory backed by SQLite.
pub struct Memory {
    db: Mutex<Connection>,
    #[allow(dead_code)]
    db_path: PathBuf,
}

impl Memory {
    /// Safely lock the DB mutex, recovering from poisoned state.
    fn lock_db(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.db
            .lock()
            .map_err(|e| ContribError::Config(format!("DB lock poisoned: {}", e)))
    }

    /// Open (or create) a SQLite database.
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ContribError::Config(format!("Cannot create db dir: {}", e)))?;
        }

        let conn = Connection::open(db_path)
            .map_err(|e| ContribError::Config(format!("SQLite open error: {}", e)))?;

        // Enable WAL for concurrency
        conn.execute_batch("PRAGMA journal_mode=WAL;").ok();

        // Create schema
        conn.execute_batch(SCHEMA)
            .map_err(|e| ContribError::Config(format!("Schema init error: {}", e)))?;

        info!(path = ?db_path, "Memory initialized");
        Ok(Self {
            db: Mutex::new(conn),
            db_path: db_path.to_path_buf(),
        })
    }

    /// Open an in-memory database (for tests).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| ContribError::Config(format!("SQLite error: {}", e)))?;
        conn.execute_batch(SCHEMA)
            .map_err(|e| ContribError::Config(format!("Schema error: {}", e)))?;
        Ok(Self {
            db: Mutex::new(conn),
            db_path: PathBuf::from(":memory:"),
        })
    }

    // ── Repos ──────────────────────────────────────────────────────────────

    /// Check if a repo has been analyzed before (no time limit).
    pub fn has_analyzed(&self, full_name: &str) -> Result<bool> {
        let db = self.lock_db()?;
        let exists: bool = db
            .query_row(
                "SELECT 1 FROM analyzed_repos WHERE full_name = ?1",
                params![full_name],
                |_| Ok(true),
            )
            .optional()
            .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?
            .unwrap_or(false);
        Ok(exists)
    }

    /// Check if a repo was analyzed within the last `days` days.
    /// Returns false if analyzed longer ago (allows re-analysis).
    pub fn has_analyzed_since(&self, full_name: &str, days: i64) -> Result<bool> {
        let db = self.lock_db()?;
        let exists: bool = db
            .query_row(
                "SELECT 1 FROM analyzed_repos WHERE full_name = ?1
                 AND analyzed_at > datetime('now', ?2)",
                params![full_name, format!("-{} days", days)],
                |_| Ok(true),
            )
            .optional()
            .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?
            .unwrap_or(false);
        Ok(exists)
    }

    /// Record that a repo was analyzed.
    pub fn record_analysis(
        &self,
        full_name: &str,
        language: &str,
        stars: i64,
        findings_count: i64,
    ) -> Result<()> {
        let db = self.lock_db()?;
        db.execute(
            "INSERT OR REPLACE INTO analyzed_repos
             (full_name, language, stars, analyzed_at, findings)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                full_name,
                language,
                stars,
                Utc::now().to_rfc3339(),
                findings_count
            ],
        )
        .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;
        Ok(())
    }

    // ── PRs ────────────────────────────────────────────────────────────────

    /// Record a submitted PR.
    #[allow(clippy::too_many_arguments)]
    pub fn record_pr(
        &self,
        repo: &str,
        pr_number: i64,
        pr_url: &str,
        title: &str,
        pr_type: &str,
        branch: &str,
        fork: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let db = self.lock_db()?;
        db.execute(
            "INSERT OR REPLACE INTO submitted_prs
             (repo, pr_number, pr_url, title, type, branch, fork, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![repo, pr_number, pr_url, title, pr_type, branch, fork, now, now],
        )
        .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;
        Ok(())
    }

    /// Update PR status.
    pub fn update_pr_status(&self, repo: &str, pr_number: i64, status: &str) -> Result<()> {
        let db = self.lock_db()?;
        db.execute(
            "UPDATE submitted_prs SET status = ?1, updated_at = ?2
             WHERE repo = ?3 AND pr_number = ?4",
            params![status, Utc::now().to_rfc3339(), repo, pr_number],
        )
        .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;
        Ok(())
    }

    /// Get PRs, optionally filtered by status.
    pub fn get_prs(
        &self,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Vec<HashMap<String, String>>> {
        let db = self.lock_db()?;
        let mut rows = Vec::new();

        if let Some(s) = status {
            let mut stmt = db
                .prepare(
                    "SELECT repo, pr_number, pr_url, title, type, status, branch, fork, created_at
                     FROM submitted_prs WHERE status = ?1
                     ORDER BY created_at DESC LIMIT ?2",
                )
                .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

            let mapped = stmt
                .query_map(params![s, limit as i64], |row| Ok(pr_row_to_map(row)))
                .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

            for m in mapped.flatten() {
                rows.push(m);
            }
        } else {
            let mut stmt = db
                .prepare(
                    "SELECT repo, pr_number, pr_url, title, type, status, branch, fork, created_at
                     FROM submitted_prs ORDER BY created_at DESC LIMIT ?1",
                )
                .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

            let mapped = stmt
                .query_map(params![limit as i64], |row| Ok(pr_row_to_map(row)))
                .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

            for m in mapped.flatten() {
                rows.push(m);
            }
        }

        Ok(rows)
    }

    /// Get number of PRs created today.
    pub fn get_today_pr_count(&self) -> Result<usize> {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let db = self.lock_db()?;
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM submitted_prs WHERE created_at LIKE ?1",
                params![format!("{}%", today)],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(count as usize)
    }

    // ── Run Log ────────────────────────────────────────────────────────────

    /// Record the start of a pipeline run.
    pub fn start_run(&self) -> Result<i64> {
        let db = self.lock_db()?;
        db.execute(
            "INSERT INTO run_log (started_at) VALUES (?1)",
            params![Utc::now().to_rfc3339()],
        )
        .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;
        Ok(db.last_insert_rowid())
    }

    /// Record completion of a pipeline run.
    pub fn finish_run(
        &self,
        run_id: i64,
        repos_analyzed: i64,
        prs_created: i64,
        findings: i64,
        errors: i64,
    ) -> Result<()> {
        let db = self.lock_db()?;
        db.execute(
            "UPDATE run_log SET finished_at = ?1, repos_analyzed = ?2,
             prs_created = ?3, findings = ?4, errors = ?5 WHERE id = ?6",
            params![
                Utc::now().to_rfc3339(),
                repos_analyzed,
                prs_created,
                findings,
                errors,
                run_id
            ],
        )
        .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;
        Ok(())
    }

    /// Get overall statistics.
    pub fn get_stats(&self) -> Result<HashMap<String, i64>> {
        let db = self.lock_db()?;
        let mut stats = HashMap::new();

        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM analyzed_repos", [], |r| r.get(0))
            .unwrap_or(0);
        stats.insert("total_repos_analyzed".into(), count);

        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM submitted_prs", [], |r| r.get(0))
            .unwrap_or(0);
        stats.insert("total_prs_submitted".into(), count);

        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM submitted_prs WHERE status = 'merged'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        stats.insert("prs_merged".into(), count);

        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM run_log", [], |r| r.get(0))
            .unwrap_or(0);
        stats.insert("total_runs".into(), count);

        Ok(stats)
    }

    // ── Sessions ──────────────────────────────────────────────────────────

    /// Create a new session.
    pub fn create_session(&self, id: &str, name: &str, mode: &str) -> Result<()> {
        let db = self.lock_db()?;
        db.execute(
            "INSERT OR REPLACE INTO sessions (id, name, mode, status, created_at)
             VALUES (?1, ?2, ?3, 'running', ?4)",
            params![id, name, mode, chrono::Utc::now().to_rfc3339()],
        )
        .map_err(|e| ContribError::Database(format!("Session create: {}", e)))?;
        Ok(())
    }

    /// Get all sessions.
    pub fn get_sessions(&self) -> Result<Vec<serde_json::Value>> {
        let db = self.lock_db()?;
        let mut stmt = db
            .prepare(
                "SELECT id, name, mode, status, created_at FROM sessions ORDER BY created_at DESC",
            )
            .map_err(|e| ContribError::Database(format!("Session query: {}", e)))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "mode": r.get::<_, String>(2)?,
                    "status": r.get::<_, String>(3)?,
                    "created_at": r.get::<_, String>(4)?,
                }))
            })
            .map_err(|e| ContribError::Database(format!("Session query map: {}", e)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── Outcome Learning ──────────────────────────────────────────────────

    /// Record PR outcome (merged, closed, rejected).
    #[allow(clippy::too_many_arguments)]
    pub fn record_outcome(
        &self,
        repo: &str,
        pr_number: i64,
        pr_url: &str,
        pr_type: &str,
        outcome: &str,
        feedback: &str,
        time_to_close_hours: f64,
    ) -> Result<()> {
        {
            let db = self.lock_db()?;
            db.execute(
                "INSERT OR REPLACE INTO pr_outcomes
                 (repo, pr_number, pr_url, pr_type, outcome, feedback,
                  time_to_close_hours, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    repo,
                    pr_number,
                    pr_url,
                    pr_type,
                    outcome,
                    feedback,
                    time_to_close_hours,
                    Utc::now().to_rfc3339()
                ],
            )
            .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;
        }

        // Auto-update preferences
        self.update_repo_preferences(repo)?;
        Ok(())
    }

    /// Recompute repo preferences from outcome history.
    fn update_repo_preferences(&self, repo: &str) -> Result<()> {
        let db = self.lock_db()?;

        let mut stmt = db
            .prepare(
                "SELECT pr_type, outcome, time_to_close_hours FROM pr_outcomes WHERE repo = ?1",
            )
            .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

        let rows: Vec<(String, String, f64)> = stmt
            .query_map(params![repo], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2).unwrap_or(0.0),
                ))
            })
            .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?
            .filter_map(|r| r.ok())
            .collect();

        if rows.is_empty() {
            return Ok(());
        }

        let mut merged_types: Vec<String> = Vec::new();
        let mut rejected_types: Vec<String> = Vec::new();
        let mut total_hours = 0.0f64;
        let mut merged_count = 0usize;

        for (pr_type, outcome, hours) in &rows {
            if outcome == "merged" {
                if !merged_types.contains(pr_type) {
                    merged_types.push(pr_type.clone());
                }
                merged_count += 1;
                total_hours += hours;
            } else if (outcome == "closed" || outcome == "rejected")
                && !rejected_types.contains(pr_type)
            {
                rejected_types.push(pr_type.clone());
            }
        }

        let merge_rate = if !rows.is_empty() {
            merged_count as f64 / rows.len() as f64
        } else {
            0.0
        };
        let avg_hours = if merged_count > 0 {
            total_hours / merged_count as f64
        } else {
            0.0
        };

        db.execute(
            "INSERT OR REPLACE INTO repo_preferences
             (repo, preferred_types, rejected_types, merge_rate,
              avg_review_hours, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                repo,
                serde_json::to_string(&merged_types).unwrap_or_default(),
                serde_json::to_string(&rejected_types).unwrap_or_default(),
                (merge_rate * 1000.0).round() / 1000.0,
                (avg_hours * 10.0).round() / 10.0,
                Utc::now().to_rfc3339()
            ],
        )
        .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

        Ok(())
    }

    /// Get learned preferences for a specific repo.
    pub fn get_repo_preferences(&self, repo: &str) -> Result<Option<RepoPreferences>> {
        let db = self.lock_db()?;
        db.query_row(
            "SELECT preferred_types, rejected_types, merge_rate, avg_review_hours, notes
             FROM repo_preferences WHERE repo = ?1",
            params![repo],
            |row| {
                let pref: String = row.get(0)?;
                let rej: String = row.get(1)?;
                Ok(RepoPreferences {
                    preferred_types: serde_json::from_str(&pref).unwrap_or_default(),
                    rejected_types: serde_json::from_str(&rej).unwrap_or_default(),
                    merge_rate: row.get(2)?,
                    avg_review_hours: row.get(3)?,
                    notes: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|e| ContribError::Config(format!("DB error: {}", e)))
    }

    // ── Working Memory ────────────────────────────────────────────────────

    /// Store hot context for a repo with TTL.
    pub fn store_context(
        &self,
        repo: &str,
        key: &str,
        value: &str,
        language: &str,
        ttl_hours: f64,
    ) -> Result<()> {
        let now = Utc::now();
        let expires = now + Duration::seconds((ttl_hours * 3600.0) as i64);
        let db = self.lock_db()?;
        db.execute(
            "INSERT OR REPLACE INTO working_memory
             (repo, key, value, language, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                repo,
                key,
                value,
                language,
                now.to_rfc3339(),
                expires.to_rfc3339()
            ],
        )
        .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;
        Ok(())
    }

    /// Retrieve hot context, returns None if expired.
    pub fn get_context(&self, repo: &str, key: &str) -> Result<Option<String>> {
        let now = Utc::now().to_rfc3339();
        let db = self.lock_db()?;
        db.query_row(
            "SELECT value FROM working_memory
             WHERE repo = ?1 AND key = ?2 AND expires_at > ?3",
            params![repo, key, now],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| ContribError::Config(format!("DB error: {}", e)))
    }

    /// Find context from repos with the same language.
    pub fn get_similar_context(
        &self,
        language: &str,
        key: &str,
        limit: usize,
    ) -> Result<Vec<(String, String)>> {
        let now = Utc::now().to_rfc3339();
        let db = self.lock_db()?;
        let mut stmt = db
            .prepare(
                "SELECT repo, value FROM working_memory
                 WHERE language = ?1 AND key = ?2 AND expires_at > ?3
                 ORDER BY created_at DESC LIMIT ?4",
            )
            .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

        let rows = stmt
            .query_map(params![language, key, now, limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Delete expired working memory entries.
    pub fn archive_expired(&self) -> Result<usize> {
        let now = Utc::now().to_rfc3339();
        let db = self.lock_db()?;
        let deleted = db
            .execute(
                "DELETE FROM working_memory WHERE expires_at <= ?1",
                params![now],
            )
            .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;
        Ok(deleted)
    }

    // ── Dream Memory Consolidation ────────────────────────────────────────

    /// Increment session counter for dream gating.
    pub fn increment_session_count(&self) -> Result<i64> {
        let db = self.lock_db()?;
        let current: i64 = db
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM dream_meta WHERE key = 'session_count'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let new_count = current + 1;
        db.execute(
            "INSERT OR REPLACE INTO dream_meta (key, value, updated_at)
             VALUES ('session_count', ?1, ?2)",
            params![new_count.to_string(), Utc::now().to_rfc3339()],
        )
        .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

        Ok(new_count)
    }

    /// Check if dream consolidation should run (3-gate trigger).
    /// Gate 1: 24h since last dream
    /// Gate 2: At least 5 sessions since last dream
    /// Gate 3: No concurrent lock
    pub fn should_dream(&self) -> Result<bool> {
        let db = self.lock_db()?;

        // Gate 1: Time — 24h since last dream
        let last_dream: Option<String> = db
            .query_row(
                "SELECT value FROM dream_meta WHERE key = 'last_dream_at'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

        if let Some(ts) = last_dream {
            if let Ok(last) = chrono::DateTime::parse_from_rfc3339(&ts) {
                let hours_since = (Utc::now() - last.with_timezone(&Utc)).num_hours();
                if hours_since < 24 {
                    return Ok(false);
                }
            }
        }

        // Gate 2: Sessions — at least 5 sessions
        let sessions: i64 = db
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM dream_meta WHERE key = 'session_count'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        if sessions < 5 {
            return Ok(false);
        }

        // Gate 3: Lock — no concurrent dream
        let locked: Option<String> = db
            .query_row(
                "SELECT value FROM dream_meta WHERE key = 'dream_lock'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

        if locked.as_deref() == Some("1") {
            return Ok(false);
        }

        Ok(true)
    }

    /// Run dream consolidation — aggregate PR outcomes into durable repo profiles.
    pub fn run_dream(&self) -> Result<DreamResult> {
        let db = self.lock_db()?;

        // Acquire lock
        db.execute(
            "INSERT OR REPLACE INTO dream_meta (key, value, updated_at)
             VALUES ('dream_lock', '1', ?1)",
            params![Utc::now().to_rfc3339()],
        )
        .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

        let mut result = DreamResult::default();

        // Phase 1: Gather — get all repos with PR history
        let repos: Vec<String> = {
            let mut stmt = db
                .prepare("SELECT DISTINCT repo FROM pr_outcomes")
                .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;
            let mapped = stmt
                .query_map([], |row| row.get(0))
                .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;
            mapped.filter_map(|r| r.ok()).collect()
        };

        // Phase 2: Consolidate — for each repo, compute profile
        for repo in &repos {
            let mut stmt = db
                .prepare(
                    "SELECT pr_type, outcome, time_to_close_hours, feedback
                     FROM pr_outcomes WHERE repo = ?1",
                )
                .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

            let rows: Vec<(String, String, f64, String)> = {
                let mapped = stmt
                    .query_map(params![repo], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, f64>(2).unwrap_or(0.0),
                            row.get::<_, String>(3).unwrap_or_default(),
                        ))
                    })
                    .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;
                mapped.filter_map(|r| r.ok()).collect()
            };

            if rows.is_empty() {
                continue;
            }

            let mut type_stats: HashMap<String, (i32, i32)> = HashMap::new(); // (merged, total)
            let mut total_hours = 0.0f64;
            let mut merged_count = 0i32;
            let mut feedbacks: Vec<String> = Vec::new();

            for (pr_type, outcome, hours, feedback) in &rows {
                let entry = type_stats.entry(pr_type.clone()).or_insert((0, 0));
                entry.1 += 1;
                if outcome == "merged" {
                    entry.0 += 1;
                    merged_count += 1;
                    total_hours += hours;
                }
                if !feedback.is_empty() {
                    feedbacks.push(feedback.clone());
                }
            }

            let preferred: Vec<String> = type_stats
                .iter()
                .filter(|(_, (m, t))| *t > 0 && (*m as f64 / *t as f64) >= 0.5)
                .map(|(k, _)| k.clone())
                .collect();

            let avoid: Vec<String> = type_stats
                .iter()
                .filter(|(_, (m, t))| *t >= 2 && *m == 0)
                .map(|(k, _)| k.clone())
                .collect();

            let merge_rate = if !rows.is_empty() {
                merged_count as f64 / rows.len() as f64
            } else {
                0.0
            };

            let avg_hours = if merged_count > 0 {
                total_hours / merged_count as f64
            } else {
                0.0
            };

            // Summarize maintainer style from feedback
            let notes = if feedbacks.is_empty() {
                String::new()
            } else {
                format!(
                    "Last {} feedbacks recorded. Patterns: {}",
                    feedbacks.len(),
                    feedbacks
                        .iter()
                        .take(3)
                        .map(|f| f.chars().take(60).collect::<String>())
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            };

            db.execute(
                "INSERT OR REPLACE INTO repo_preferences
                 (repo, preferred_types, rejected_types, merge_rate,
                  avg_review_hours, notes, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    repo,
                    serde_json::to_string(&preferred).unwrap_or_default(),
                    serde_json::to_string(&avoid).unwrap_or_default(),
                    (merge_rate * 1000.0).round() / 1000.0,
                    (avg_hours * 10.0).round() / 10.0,
                    notes,
                    Utc::now().to_rfc3339()
                ],
            )
            .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

            result.repos_profiled += 1;
        }

        // Prune expired working memory
        let now = Utc::now().to_rfc3339();
        let pruned = db
            .execute(
                "DELETE FROM working_memory WHERE expires_at <= ?1",
                params![now],
            )
            .unwrap_or(0);
        result.entries_pruned = pruned;

        // Update dream meta
        db.execute(
            "INSERT OR REPLACE INTO dream_meta (key, value, updated_at)
             VALUES ('last_dream_at', ?1, ?1)",
            params![Utc::now().to_rfc3339()],
        )
        .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

        // Reset session counter
        db.execute(
            "INSERT OR REPLACE INTO dream_meta (key, value, updated_at)
             VALUES ('session_count', '0', ?1)",
            params![Utc::now().to_rfc3339()],
        )
        .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

        // Release lock
        db.execute(
            "INSERT OR REPLACE INTO dream_meta (key, value, updated_at)
             VALUES ('dream_lock', '0', ?1)",
            params![Utc::now().to_rfc3339()],
        )
        .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

        result.success = true;
        Ok(result)
    }

    /// Get dream stats for display.
    pub fn get_dream_stats(&self) -> Result<HashMap<String, String>> {
        let db = self.lock_db()?;
        let mut stats = HashMap::new();

        let last: String = db
            .query_row(
                "SELECT value FROM dream_meta WHERE key = 'last_dream_at'",
                [],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| "never".into());
        stats.insert("last_dream".into(), last);

        let sessions: String = db
            .query_row(
                "SELECT value FROM dream_meta WHERE key = 'session_count'",
                [],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| "0".into());
        stats.insert("sessions_since_dream".into(), sessions);

        let profiles: i64 = db
            .query_row("SELECT COUNT(*) FROM repo_preferences", [], |r| r.get(0))
            .unwrap_or(0);
        stats.insert("repo_profiles".into(), profiles.to_string());

        Ok(stats)
    }

    /// Get repo leaderboard sorted by merge rate.
    pub fn get_leaderboard(&self, limit: usize) -> Result<Vec<HashMap<String, String>>> {
        let db = self.lock_db()?;
        let mut stmt = db
            .prepare(
                "SELECT repo, merge_rate, preferred_types, rejected_types
                 FROM repo_preferences
                 ORDER BY merge_rate DESC LIMIT ?1",
            )
            .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

        let rows = stmt
            .query_map(params![limit as i64], |row| {
                let mut m = HashMap::new();
                m.insert("repo".into(), row.get::<_, String>(0)?);
                m.insert(
                    "merge_rate".into(),
                    format!("{:.0}%", row.get::<_, f64>(1)? * 100.0),
                );
                m.insert("preferred".into(), row.get::<_, String>(2)?);
                m.insert("avoided".into(), row.get::<_, String>(3)?);
                Ok(m)
            })
            .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get consolidated repo profile from dream data.
    ///
    /// Returns `None` if no profile exists yet (dream hasn't run for this repo).
    pub fn get_repo_profile(&self, repo: &str) -> Result<Option<RepoPreferences>> {
        let db = self.lock_db()?;
        let result = db.query_row(
            "SELECT preferred_types, rejected_types, merge_rate, avg_review_hours, notes
             FROM repo_preferences WHERE repo = ?1",
            params![repo],
            |row| {
                let preferred_str: String = row.get(0)?;
                let rejected_str: String = row.get(1)?;
                let merge_rate: f64 = row.get(2)?;
                let avg_review_hours: f64 = row.get(3)?;
                let notes: String = row.get(4)?;

                let preferred: Vec<String> =
                    serde_json::from_str(&preferred_str).unwrap_or_default();
                let rejected: Vec<String> = serde_json::from_str(&rejected_str).unwrap_or_default();

                Ok(RepoPreferences {
                    preferred_types: preferred,
                    rejected_types: rejected,
                    merge_rate,
                    avg_review_hours,
                    notes,
                })
            },
        );

        match result {
            Ok(profile) => Ok(Some(profile)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(ContribError::Config(format!("DB error: {}", e))),
        }
    }

    // ── PR Conversation Memory ────────────────────────────────────────────────

    /// Record a single message in a PR conversation thread.
    ///
    /// Duplicate comment_ids are silently ignored (UNIQUE constraint).
    pub fn record_conversation(&self, msg: &ConversationMessage) -> Result<()> {
        let repo = &msg.repo;
        let pr_number = msg.pr_number;
        let role = &msg.role;
        let author = &msg.author;
        let body = &msg.body;
        let comment_id = msg.comment_id;
        let is_inline = msg.is_inline;
        let file_path = msg.file_path.as_deref();
        let db = self.lock_db()?;
        let now = Utc::now().to_rfc3339();
        db.execute(
            "INSERT OR IGNORE INTO pr_conversations
             (repo, pr_number, role, author, body, comment_id, is_inline, file_path, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                repo,
                pr_number,
                role,
                author,
                body,
                comment_id,
                is_inline as i32,
                file_path,
                now
            ],
        )
        .map_err(|e| ContribError::Config(format!("Failed to record conversation: {}", e)))?;
        Ok(())
    }

    /// Get full conversation context for a PR, formatted for LLM prompts.
    ///
    /// Returns a chronologically-ordered thread like:
    /// ```text
    /// [maintainer @alice] Please use `unwrap_or_default()` instead.
    /// [contribai @contribai-bot] ✅ Fixed — pushed update.
    /// [maintainer @alice] Looks good, thanks!
    /// ```
    pub fn get_conversation_context(&self, repo: &str, pr_number: i64) -> Result<String> {
        let db = self.lock_db()?;
        let mut stmt = db
            .prepare(
                "SELECT role, author, body, file_path, is_inline
                 FROM pr_conversations
                 WHERE repo = ?1 AND pr_number = ?2
                 ORDER BY id ASC",
            )
            .map_err(|e| ContribError::Config(format!("DB prepare: {}", e)))?;

        let rows = stmt
            .query_map(params![repo, pr_number], |row| {
                let role: String = row.get(0)?;
                let author: String = row.get(1)?;
                let body: String = row.get(2)?;
                let file_path: Option<String> = row.get(3)?;
                let is_inline: bool = row.get::<_, i32>(4)? != 0;
                Ok((role, author, body, file_path, is_inline))
            })
            .map_err(|e| ContribError::Config(format!("DB query: {}", e)))?;

        let mut lines = Vec::new();
        for row in rows {
            let (role, author, body, file_path, is_inline) =
                row.map_err(|e| ContribError::Config(format!("DB row: {}", e)))?;

            let location = if is_inline {
                file_path
                    .map(|f| format!(" (on {})", f))
                    .unwrap_or_default()
            } else {
                String::new()
            };

            lines.push(format!("[{} @{}{}] {}", role, author, location, body));
        }

        Ok(lines.join("\n"))
    }

    /// Count conversation messages for a PR.
    pub fn get_conversation_count(&self, repo: &str, pr_number: i64) -> Result<usize> {
        let db = self.lock_db()?;
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM pr_conversations WHERE repo = ?1 AND pr_number = ?2",
                params![repo, pr_number],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(count as usize)
    }
}

/// Result of a dream consolidation pass.
#[derive(Debug, Default)]
pub struct DreamResult {
    pub success: bool,
    pub repos_profiled: usize,
    pub entries_pruned: usize,
}

/// Learned repo preferences.
#[derive(Debug, Clone)]
pub struct RepoPreferences {
    pub preferred_types: Vec<String>,
    pub rejected_types: Vec<String>,
    pub merge_rate: f64,
    pub avg_review_hours: f64,
    pub notes: String,
}

/// Helper: convert a PR row to HashMap.
fn pr_row_to_map(row: &rusqlite::Row) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("repo".into(), row.get::<_, String>(0).unwrap_or_default());
    m.insert(
        "pr_number".into(),
        row.get::<_, i64>(1).unwrap_or(0).to_string(),
    );
    m.insert("pr_url".into(), row.get::<_, String>(2).unwrap_or_default());
    m.insert("title".into(), row.get::<_, String>(3).unwrap_or_default());
    m.insert("type".into(), row.get::<_, String>(4).unwrap_or_default());
    m.insert("status".into(), row.get::<_, String>(5).unwrap_or_default());
    m.insert("branch".into(), row.get::<_, String>(6).unwrap_or_default());
    m.insert("fork".into(), row.get::<_, String>(7).unwrap_or_default());
    m.insert(
        "created_at".into(),
        row.get::<_, String>(8).unwrap_or_default(),
    );
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_memory() -> Memory {
        Memory::open_in_memory().unwrap()
    }

    #[test]
    fn test_analyzed_repos() {
        let mem = test_memory();
        assert!(!mem.has_analyzed("test/repo").unwrap());

        mem.record_analysis("test/repo", "python", 100, 5).unwrap();
        assert!(mem.has_analyzed("test/repo").unwrap());
    }

    #[test]
    fn test_analyzed_since() {
        let mem = test_memory();
        assert!(!mem.has_analyzed_since("test/repo", 7).unwrap());

        mem.record_analysis("test/repo", "python", 100, 5).unwrap();
        // Just recorded — should be within 7 days
        assert!(mem.has_analyzed_since("test/repo", 7).unwrap());
        // 0-day window should still match (within same day)
        assert!(mem.has_analyzed_since("test/repo", 0).unwrap());
    }

    #[test]
    fn test_pr_recording() {
        let mem = test_memory();
        mem.record_pr(
            "test/repo",
            42,
            "https://github.com/test/repo/pull/42",
            "fix: issue",
            "code_quality",
            "fix/issue",
            "fork/repo",
        )
        .unwrap();

        let prs = mem.get_prs(None, 10).unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0]["pr_number"], "42");
    }

    #[test]
    fn test_pr_status_update() {
        let mem = test_memory();
        mem.record_pr("test/repo", 1, "url", "title", "fix", "branch", "fork")
            .unwrap();

        mem.update_pr_status("test/repo", 1, "merged").unwrap();
        let prs = mem.get_prs(Some("merged"), 10).unwrap();
        assert_eq!(prs.len(), 1);
    }

    #[test]
    fn test_today_pr_count() {
        let mem = test_memory();
        mem.record_pr("a/b", 1, "url1", "t1", "fix", "", "")
            .unwrap();
        mem.record_pr("a/b", 2, "url2", "t2", "fix", "", "")
            .unwrap();

        let count = mem.get_today_pr_count().unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_run_log() {
        let mem = test_memory();
        let run_id = mem.start_run().unwrap();
        assert!(run_id > 0);

        mem.finish_run(run_id, 5, 2, 10, 1).unwrap();
        let stats = mem.get_stats().unwrap();
        assert_eq!(stats["total_runs"], 1);
    }

    #[test]
    fn test_outcome_learning() {
        let mem = test_memory();

        mem.record_outcome("test/repo", 1, "url1", "security_fix", "merged", "", 24.0)
            .unwrap();
        mem.record_outcome(
            "test/repo",
            2,
            "url2",
            "code_quality",
            "closed",
            "not needed",
            48.0,
        )
        .unwrap();
        mem.record_outcome("test/repo", 3, "url3", "security_fix", "merged", "", 12.0)
            .unwrap();

        let prefs = mem.get_repo_preferences("test/repo").unwrap().unwrap();
        assert!(prefs.preferred_types.contains(&"security_fix".to_string()));
        assert!(prefs.rejected_types.contains(&"code_quality".to_string()));
        assert!((prefs.merge_rate - 0.667).abs() < 0.01);
        assert!(prefs.avg_review_hours > 0.0);
    }

    #[test]
    fn test_working_memory() {
        let mem = test_memory();

        mem.store_context("test/repo", "style", "4 spaces indent", "python", 24.0)
            .unwrap();
        let val = mem.get_context("test/repo", "style").unwrap();
        assert_eq!(val, Some("4 spaces indent".to_string()));

        let missing = mem.get_context("test/repo", "nonexistent").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_similar_context() {
        let mem = test_memory();
        mem.store_context("repo/a", "style", "PEP 8", "python", 24.0)
            .unwrap();
        mem.store_context("repo/b", "style", "Black format", "python", 24.0)
            .unwrap();
        mem.store_context("repo/c", "style", "gofmt", "go", 24.0)
            .unwrap();

        let similar = mem.get_similar_context("python", "style", 10).unwrap();
        assert_eq!(similar.len(), 2);
    }

    #[test]
    fn test_stats() {
        let mem = test_memory();
        mem.record_analysis("a/b", "python", 100, 5).unwrap();
        mem.record_pr("a/b", 1, "url", "t", "fix", "", "").unwrap();
        mem.update_pr_status("a/b", 1, "merged").unwrap();

        let stats = mem.get_stats().unwrap();
        assert_eq!(stats["total_repos_analyzed"], 1);
        assert_eq!(stats["total_prs_submitted"], 1);
        assert_eq!(stats["prs_merged"], 1);
    }

    // ── Dream consolidation tests ─────────────────────────────────────────

    #[test]
    fn test_session_counter() {
        let mem = test_memory();
        assert_eq!(mem.increment_session_count().unwrap(), 1);
        assert_eq!(mem.increment_session_count().unwrap(), 2);
        assert_eq!(mem.increment_session_count().unwrap(), 3);
    }

    #[test]
    fn test_should_dream_gates() {
        let mem = test_memory();

        // No sessions yet → false
        assert!(!mem.should_dream().unwrap());

        // Add 5 sessions → should pass (no prior dream = time gate passes)
        for _ in 0..5 {
            mem.increment_session_count().unwrap();
        }
        assert!(mem.should_dream().unwrap());
    }

    #[test]
    fn test_dream_consolidation() {
        let mem = test_memory();

        // Add outcomes
        mem.record_outcome("repo/a", 1, "url1", "security_fix", "merged", "", 24.0)
            .unwrap();
        mem.record_outcome("repo/a", 2, "url2", "docs", "merged", "good docs", 12.0)
            .unwrap();
        mem.record_outcome("repo/a", 3, "url3", "refactor", "closed", "not needed", 0.0)
            .unwrap();

        mem.record_outcome("repo/b", 10, "url10", "docs", "merged", "", 6.0)
            .unwrap();

        // Fill sessions
        for _ in 0..5 {
            mem.increment_session_count().unwrap();
        }

        // Run dream
        let result = mem.run_dream().unwrap();
        assert!(result.success);
        assert_eq!(result.repos_profiled, 2);

        // Verify profiles
        let prefs_a = mem.get_repo_preferences("repo/a").unwrap().unwrap();
        assert!(prefs_a
            .preferred_types
            .contains(&"security_fix".to_string()));
        assert!(prefs_a.preferred_types.contains(&"docs".to_string()));
        assert!(prefs_a.merge_rate > 0.6);

        let prefs_b = mem.get_repo_preferences("repo/b").unwrap().unwrap();
        assert_eq!(prefs_b.merge_rate, 1.0);

        // After dream, session counter should be reset
        assert!(!mem.should_dream().unwrap());
    }

    #[test]
    fn test_dream_stats() {
        let mem = test_memory();
        let stats = mem.get_dream_stats().unwrap();
        assert_eq!(stats["last_dream"], "never");
        assert_eq!(stats["sessions_since_dream"], "0");
    }

    #[test]
    fn test_leaderboard() {
        let mem = test_memory();

        mem.record_outcome("repo/a", 1, "u", "fix", "merged", "", 10.0)
            .unwrap();
        mem.record_outcome("repo/b", 1, "u", "fix", "closed", "", 10.0)
            .unwrap();
        mem.record_outcome("repo/b", 2, "u", "fix", "merged", "", 10.0)
            .unwrap();

        let board = mem.get_leaderboard(10).unwrap();
        assert!(!board.is_empty());
        // repo/a has 100% merge rate, should be first
        assert_eq!(board[0]["repo"], "repo/a");
    }
}
