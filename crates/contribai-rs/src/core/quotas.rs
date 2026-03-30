//! API usage tracking and quota enforcement.
//!
//! Port from Python `core/quotas.py`.

use chrono::Local;
use tracing::warn;

/// Usage record for a single day.
#[derive(Debug, Clone)]
struct UsageRecord {
    date: String,
    github_calls: u64,
    llm_calls: u64,
    llm_tokens: u64,
}

impl Default for UsageRecord {
    fn default() -> Self {
        Self {
            date: Local::now().format("%Y-%m-%d").to_string(),
            github_calls: 0,
            llm_calls: 0,
            llm_tokens: 0,
        }
    }
}

/// Track and enforce API usage quotas.
pub struct UsageTracker {
    github_limit: u64,
    llm_limit: u64,
    llm_token_limit: u64,
    usage: UsageRecord,
}

impl UsageTracker {
    pub fn new(github_daily_limit: u64, llm_daily_limit: u64, llm_daily_tokens: u64) -> Self {
        Self {
            github_limit: github_daily_limit,
            llm_limit: llm_daily_limit,
            llm_token_limit: llm_daily_tokens,
            usage: UsageRecord::default(),
        }
    }

    fn today() -> String {
        Local::now().format("%Y-%m-%d").to_string()
    }

    fn ensure_today(&mut self) {
        let today = Self::today();
        if self.usage.date != today {
            self.usage = UsageRecord {
                date: today,
                ..Default::default()
            };
        }
    }

    // ── Recording ────────────────────────────────────

    pub fn record_github_call(&mut self, count: u64) {
        self.ensure_today();
        self.usage.github_calls += count;
        if self.usage.github_calls >= self.github_limit {
            warn!(
                calls = self.usage.github_calls,
                limit = self.github_limit,
                "GitHub API quota exhausted"
            );
        }
    }

    pub fn record_llm_call(&mut self, tokens_used: u64) {
        self.ensure_today();
        self.usage.llm_calls += 1;
        self.usage.llm_tokens += tokens_used;
    }

    // ── Checking ─────────────────────────────────────

    pub fn check_github_quota(&mut self) -> bool {
        self.ensure_today();
        self.usage.github_calls < self.github_limit
    }

    pub fn check_llm_quota(&mut self) -> bool {
        self.ensure_today();
        self.usage.llm_calls < self.llm_limit && self.usage.llm_tokens < self.llm_token_limit
    }

    pub fn github_remaining(&mut self) -> u64 {
        self.ensure_today();
        self.github_limit.saturating_sub(self.usage.github_calls)
    }

    pub fn llm_remaining(&mut self) -> u64 {
        self.ensure_today();
        self.llm_limit.saturating_sub(self.usage.llm_calls)
    }

    pub fn llm_tokens_remaining(&mut self) -> u64 {
        self.ensure_today();
        self.llm_token_limit.saturating_sub(self.usage.llm_tokens)
    }

    pub fn get_usage(&mut self) -> UsageStats {
        self.ensure_today();
        UsageStats {
            date: self.usage.date.clone(),
            github_calls: self.usage.github_calls,
            github_limit: self.github_limit,
            github_remaining: self.github_remaining(),
            llm_calls: self.usage.llm_calls,
            llm_limit: self.llm_limit,
            llm_remaining: self.llm_remaining(),
            llm_tokens_used: self.usage.llm_tokens,
            llm_tokens_limit: self.llm_token_limit,
            llm_tokens_remaining: self.llm_tokens_remaining(),
        }
    }
}

impl Default for UsageTracker {
    fn default() -> Self {
        Self::new(5000, 1000, 1_000_000)
    }
}

#[derive(Debug, Clone)]
pub struct UsageStats {
    pub date: String,
    pub github_calls: u64,
    pub github_limit: u64,
    pub github_remaining: u64,
    pub llm_calls: u64,
    pub llm_limit: u64,
    pub llm_remaining: u64,
    pub llm_tokens_used: u64,
    pub llm_tokens_limit: u64,
    pub llm_tokens_remaining: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_tracker() {
        let mut t = UsageTracker::default();
        assert!(t.check_github_quota());
        assert!(t.check_llm_quota());
        assert_eq!(t.github_remaining(), 5000);
    }

    #[test]
    fn test_record_github() {
        let mut t = UsageTracker::new(100, 100, 100_000);
        t.record_github_call(50);
        assert_eq!(t.github_remaining(), 50);
        assert!(t.check_github_quota());
        t.record_github_call(50);
        assert!(!t.check_github_quota());
    }

    #[test]
    fn test_record_llm() {
        let mut t = UsageTracker::new(100, 5, 100_000);
        t.record_llm_call(1000);
        t.record_llm_call(2000);
        assert_eq!(t.llm_remaining(), 3);
        assert!(t.check_llm_quota());
    }

    #[test]
    fn test_llm_token_limit() {
        let mut t = UsageTracker::new(100, 100, 5000);
        t.record_llm_call(5000);
        assert!(!t.check_llm_quota());
    }

    #[test]
    fn test_get_usage() {
        let mut t = UsageTracker::new(100, 100, 100_000);
        t.record_github_call(10);
        t.record_llm_call(500);
        let stats = t.get_usage();
        assert_eq!(stats.github_calls, 10);
        assert_eq!(stats.llm_calls, 1);
        assert_eq!(stats.llm_tokens_used, 500);
    }
}
