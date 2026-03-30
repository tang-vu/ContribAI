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
        self.db.lock().map_err(|e| {
            ContribError::Config(format!("DB lock poisoned: {}", e))
        })
    }

    /// Open (or create) a SQLite database.
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ContribError::Config(format!("Cannot create db dir: {}", e))
            })?;
        }

        let conn = Connection::open(db_path).map_err(|e| {
            ContribError::Config(format!("SQLite open error: {}", e))
        })?;

        // Enable WAL for concurrency
        conn.execute_batch("PRAGMA journal_mode=WAL;").ok();

        // Create schema
        conn.execute_batch(SCHEMA).map_err(|e| {
            ContribError::Config(format!("Schema init error: {}", e))
        })?;

        info!(path = ?db_path, "Memory initialized");
        Ok(Self {
            db: Mutex::new(conn),
            db_path: db_path.to_path_buf(),
        })
    }

    /// Open an in-memory database (for tests).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(|e| {
            ContribError::Config(format!("SQLite error: {}", e))
        })?;
        conn.execute_batch(SCHEMA).map_err(|e| {
            ContribError::Config(format!("Schema error: {}", e))
        })?;
        Ok(Self {
            db: Mutex::new(conn),
            db_path: PathBuf::from(":memory:"),
        })
    }

    // ── Repos ──────────────────────────────────────────────────────────────

    /// Check if a repo has been analyzed before.
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
            params![full_name, language, stars, Utc::now().to_rfc3339(), findings_count],
        )
        .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;
        Ok(())
    }

    // ── PRs ────────────────────────────────────────────────────────────────

    /// Record a submitted PR.
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
    pub fn get_prs(&self, status: Option<&str>, limit: usize) -> Result<Vec<HashMap<String, String>>> {
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
                .query_map(params![s, limit as i64], |row| {
                    Ok(pr_row_to_map(row))
                })
                .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

            for r in mapped {
                if let Ok(m) = r {
                    rows.push(m);
                }
            }
        } else {
            let mut stmt = db
                .prepare(
                    "SELECT repo, pr_number, pr_url, title, type, status, branch, fork, created_at
                     FROM submitted_prs ORDER BY created_at DESC LIMIT ?1",
                )
                .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

            let mapped = stmt
                .query_map(params![limit as i64], |row| {
                    Ok(pr_row_to_map(row))
                })
                .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;

            for r in mapped {
                if let Ok(m) = r {
                    rows.push(m);
                }
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
            params![Utc::now().to_rfc3339(), repos_analyzed, prs_created, findings, errors, run_id],
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

    // ── Outcome Learning ──────────────────────────────────────────────────

    /// Record PR outcome (merged, closed, rejected).
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
                    repo, pr_number, pr_url, pr_type, outcome, feedback,
                    time_to_close_hours, Utc::now().to_rfc3339()
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
            } else if outcome == "closed" || outcome == "rejected" {
                if !rejected_types.contains(pr_type) {
                    rejected_types.push(pr_type.clone());
                }
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
            params![repo, key, value, language, now.to_rfc3339(), expires.to_rfc3339()],
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
            .execute("DELETE FROM working_memory WHERE expires_at <= ?1", params![now])
            .map_err(|e| ContribError::Config(format!("DB error: {}", e)))?;
        Ok(deleted)
    }
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
    m.insert("pr_number".into(), row.get::<_, i64>(1).unwrap_or(0).to_string());
    m.insert("pr_url".into(), row.get::<_, String>(2).unwrap_or_default());
    m.insert("title".into(), row.get::<_, String>(3).unwrap_or_default());
    m.insert("type".into(), row.get::<_, String>(4).unwrap_or_default());
    m.insert("status".into(), row.get::<_, String>(5).unwrap_or_default());
    m.insert("branch".into(), row.get::<_, String>(6).unwrap_or_default());
    m.insert("fork".into(), row.get::<_, String>(7).unwrap_or_default());
    m.insert("created_at".into(), row.get::<_, String>(8).unwrap_or_default());
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
    fn test_pr_recording() {
        let mem = test_memory();
        mem.record_pr(
            "test/repo", 42, "https://github.com/test/repo/pull/42",
            "fix: issue", "code_quality", "fix/issue", "fork/repo",
        ).unwrap();

        let prs = mem.get_prs(None, 10).unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0]["pr_number"], "42");
    }

    #[test]
    fn test_pr_status_update() {
        let mem = test_memory();
        mem.record_pr(
            "test/repo", 1, "url", "title", "fix", "branch", "fork",
        ).unwrap();

        mem.update_pr_status("test/repo", 1, "merged").unwrap();
        let prs = mem.get_prs(Some("merged"), 10).unwrap();
        assert_eq!(prs.len(), 1);
    }

    #[test]
    fn test_today_pr_count() {
        let mem = test_memory();
        mem.record_pr("a/b", 1, "url1", "t1", "fix", "", "").unwrap();
        mem.record_pr("a/b", 2, "url2", "t2", "fix", "", "").unwrap();

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

        mem.record_outcome("test/repo", 1, "url1", "security_fix", "merged", "", 24.0).unwrap();
        mem.record_outcome("test/repo", 2, "url2", "code_quality", "closed", "not needed", 48.0).unwrap();
        mem.record_outcome("test/repo", 3, "url3", "security_fix", "merged", "", 12.0).unwrap();

        let prefs = mem.get_repo_preferences("test/repo").unwrap().unwrap();
        assert!(prefs.preferred_types.contains(&"security_fix".to_string()));
        assert!(prefs.rejected_types.contains(&"code_quality".to_string()));
        assert!((prefs.merge_rate - 0.667).abs() < 0.01);
        assert!(prefs.avg_review_hours > 0.0);
    }

    #[test]
    fn test_working_memory() {
        let mem = test_memory();

        mem.store_context("test/repo", "style", "4 spaces indent", "python", 24.0).unwrap();
        let val = mem.get_context("test/repo", "style").unwrap();
        assert_eq!(val, Some("4 spaces indent".to_string()));

        let missing = mem.get_context("test/repo", "nonexistent").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_similar_context() {
        let mem = test_memory();
        mem.store_context("repo/a", "style", "PEP 8", "python", 24.0).unwrap();
        mem.store_context("repo/b", "style", "Black format", "python", 24.0).unwrap();
        mem.store_context("repo/c", "style", "gofmt", "go", 24.0).unwrap();

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
}
