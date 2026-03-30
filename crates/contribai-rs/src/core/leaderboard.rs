//! Contribution leaderboard and success rate tracking.
//!
//! Port from Python `core/leaderboard.py`.

use rusqlite::Connection;
use tracing::debug;

/// A single leaderboard entry.
#[derive(Debug, Clone)]
pub struct LeaderboardEntry {
    pub repo: String,
    pub total_prs: i64,
    pub merged: i64,
    pub closed: i64,
    pub open: i64,
}

impl LeaderboardEntry {
    pub fn merge_rate(&self) -> f64 {
        let decided = self.merged + self.closed;
        if decided == 0 { 0.0 } else { self.merged as f64 / decided as f64 * 100.0 }
    }

    pub fn status(&self) -> &str {
        let rate = self.merge_rate();
        if rate >= 70.0 { "excellent" }
        else if rate >= 40.0 { "good" }
        else if rate > 0.0 { "needs_improvement" }
        else { "pending" }
    }
}

/// Stats per contribution type.
#[derive(Debug, Clone)]
pub struct TypeStats {
    pub contribution_type: String,
    pub total: i64,
    pub merged: i64,
    pub closed: i64,
}

impl TypeStats {
    pub fn merge_rate(&self) -> f64 {
        let decided = self.merged + self.closed;
        if decided == 0 { 0.0 } else { self.merged as f64 / decided as f64 * 100.0 }
    }
}

/// Contribution leaderboard — reads from submitted_prs table.
pub struct Leaderboard<'a> {
    db: &'a Connection,
}

impl<'a> Leaderboard<'a> {
    pub fn new(db: &'a Connection) -> Self {
        Self { db }
    }

    /// Get overall contribution statistics.
    pub fn get_overall_stats(&self) -> OverallStats {
        let mut stats = OverallStats::default();
        let mut stmt = match self.db.prepare(
            "SELECT status, COUNT(*) FROM submitted_prs GROUP BY status",
        ) {
            Ok(s) => s,
            Err(e) => {
                debug!(error = %e, "Could not query overall stats");
                return stats;
            }
        };
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .ok();
        if let Some(rows) = rows {
            for row in rows.flatten() {
                let (status, count) = row;
                stats.total += count;
                match status.as_str() {
                    "merged" => stats.merged = count,
                    "closed" => stats.closed = count,
                    "open" => stats.open = count,
                    _ => {}
                }
            }
        }
        let decided = stats.merged + stats.closed;
        stats.merge_rate = if decided > 0 {
            stats.merged as f64 / decided as f64 * 100.0
        } else {
            0.0
        };
        stats
    }

    /// Get repo rankings by merge count.
    pub fn get_repo_rankings(&self, limit: u32) -> Vec<LeaderboardEntry> {
        let mut stmt = match self.db.prepare(
            "SELECT repo, COUNT(*) as total,
             SUM(CASE WHEN status='merged' THEN 1 ELSE 0 END),
             SUM(CASE WHEN status='closed' THEN 1 ELSE 0 END),
             SUM(CASE WHEN status='open' THEN 1 ELSE 0 END)
             FROM submitted_prs GROUP BY repo
             ORDER BY SUM(CASE WHEN status='merged' THEN 1 ELSE 0 END) DESC, total DESC
             LIMIT ?",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map([limit], |row| {
            Ok(LeaderboardEntry {
                repo: row.get(0)?,
                total_prs: row.get(1)?,
                merged: row.get(2)?,
                closed: row.get(3)?,
                open: row.get(4)?,
            })
        })
        .ok()
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// Get success stats by contribution type.
    pub fn get_type_stats(&self) -> Vec<TypeStats> {
        let mut stmt = match self.db.prepare(
            "SELECT type, COUNT(*), 
             SUM(CASE WHEN status='merged' THEN 1 ELSE 0 END),
             SUM(CASE WHEN status='closed' THEN 1 ELSE 0 END)
             FROM submitted_prs GROUP BY type ORDER BY 
             SUM(CASE WHEN status='merged' THEN 1 ELSE 0 END) DESC",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map([], |row| {
            Ok(TypeStats {
                contribution_type: row.get(0)?,
                total: row.get(1)?,
                merged: row.get(2)?,
                closed: row.get(3)?,
            })
        })
        .ok()
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }
    /// Get recently merged PRs.
    pub fn get_recent_merges(&self, limit: u32) -> Vec<RecentMerge> {
        let mut stmt = match self.db.prepare(
            "SELECT repo, pr_number, pr_url, title, type, updated_at
             FROM submitted_prs
             WHERE status = 'merged'
             ORDER BY updated_at DESC
             LIMIT ?",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map([limit], |row| {
            Ok(RecentMerge {
                repo: row.get(0)?,
                pr_number: row.get(1)?,
                pr_url: row.get(2)?,
                title: row.get(3)?,
                contribution_type: row.get(4)?,
                merged_at: row.get(5)?,
            })
        })
        .ok()
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }
}

/// A recently merged PR entry.
#[derive(Debug, Clone)]
pub struct RecentMerge {
    pub repo: String,
    pub pr_number: i64,
    pub pr_url: String,
    pub title: String,
    pub contribution_type: String,
    pub merged_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct OverallStats {
    pub total: i64,
    pub merged: i64,
    pub closed: i64,
    pub open: i64,
    pub merge_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE submitted_prs (
                repo TEXT, pr_number INTEGER, pr_url TEXT, title TEXT,
                type TEXT, status TEXT, updated_at TEXT
            );
            INSERT INTO submitted_prs VALUES ('a/b', 1, '', 'fix', 'security_fix', 'merged', '');
            INSERT INTO submitted_prs VALUES ('a/b', 2, '', 'doc', 'docs_improve', 'merged', '');
            INSERT INTO submitted_prs VALUES ('c/d', 3, '', 'test', 'code_quality', 'closed', '');
            INSERT INTO submitted_prs VALUES ('c/d', 4, '', 'feat', 'feature_add', 'open', '');",
        )
        .unwrap();
        db
    }

    #[test]
    fn test_overall_stats() {
        let db = setup_db();
        let lb = Leaderboard::new(&db);
        let stats = lb.get_overall_stats();
        assert_eq!(stats.total, 4);
        assert_eq!(stats.merged, 2);
        assert_eq!(stats.closed, 1);
    }

    #[test]
    fn test_repo_rankings() {
        let db = setup_db();
        let lb = Leaderboard::new(&db);
        let rankings = lb.get_repo_rankings(10);
        assert_eq!(rankings.len(), 2);
        assert_eq!(rankings[0].repo, "a/b"); // 2 merged > 0
    }

    #[test]
    fn test_type_stats() {
        let db = setup_db();
        let lb = Leaderboard::new(&db);
        let stats = lb.get_type_stats();
        assert!(!stats.is_empty());
    }

    #[test]
    fn test_recent_merges() {
        let db = setup_db();
        let lb = Leaderboard::new(&db);
        let merges = lb.get_recent_merges(5);
        assert_eq!(merges.len(), 2);
        assert!(merges.iter().all(|m| m.pr_url.is_empty() || m.pr_url.len() >= 0));
    }

    #[test]
    fn test_merge_rate() {
        let entry = LeaderboardEntry {
            repo: "test".into(),
            total_prs: 10,
            merged: 7,
            closed: 3,
            open: 0,
        };
        assert!((entry.merge_rate() - 70.0).abs() < 0.01);
        assert_eq!(entry.status(), "excellent");
    }
}
