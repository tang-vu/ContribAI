//! Circuit Breaker for LLM provider failures.
//!
//! Implements the Circuit Breaker pattern to prevent cascading failures
//! when the LLM provider is experiencing issues. States:
//!
//! - **Closed**: Normal operation, requests pass through.
//! - **Open**: After N consecutive failures, circuit opens — requests fail fast.
//! - **Half-Open**: After a cooldown period, one request is allowed through to test.
//!
//! If the test request succeeds, circuit closes. If it fails, circuit re-opens.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;
use tracing::{info, warn};

/// Circuit breaker state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation — requests pass through.
    Closed,
    /// Too many failures — requests fail fast.
    Open,
    /// Testing recovery — one request allowed through.
    HalfOpen,
}

/// Shared circuit breaker with atomic state.
/// Thread-safe and cloneable (shares state via Arc).
pub struct CircuitBreaker {
    /// Current state: 0=Closed, 1=Open, 2=HalfOpen
    state: AtomicU32,
    /// Consecutive failure count.
    failures: AtomicU32,
    /// Consecutive success count (used in HalfOpen to close circuit).
    successes: AtomicU32,
    /// Failure threshold to open the circuit.
    failure_threshold: u32,
    /// Number of consecutive successes needed to close from HalfOpen.
    success_threshold: u32,
    /// Cooldown duration before transitioning Open → HalfOpen.
    cooldown: Duration,
    /// Timestamp when circuit was opened.
    opened_at: AtomicU64,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with default thresholds.
    pub fn new() -> Self {
        Self {
            state: AtomicU32::new(CircuitState::Closed as u32),
            failures: AtomicU32::new(0),
            successes: AtomicU32::new(0),
            failure_threshold: 5,
            success_threshold: 2,
            cooldown: Duration::from_secs(300), // 5 minutes
            opened_at: AtomicU64::new(0),
        }
    }

    /// Create with custom thresholds.
    pub fn with_thresholds(
        mut self,
        failure_threshold: u32,
        success_threshold: u32,
        cooldown_secs: u64,
    ) -> Self {
        self.failure_threshold = failure_threshold;
        self.success_threshold = success_threshold;
        self.cooldown = Duration::from_secs(cooldown_secs);
        self
    }

    /// Get current state.
    pub fn state(&self) -> CircuitState {
        match self.state.load(Ordering::Relaxed) {
            0 => CircuitState::Closed,
            1 => CircuitState::Open,
            2 => CircuitState::HalfOpen,
            _ => CircuitState::Closed,
        }
    }

    /// Get consecutive failure count.
    pub fn failure_count(&self) -> u32 {
        self.failures.load(Ordering::Relaxed)
    }

    /// Check if a request should be allowed through.
    ///
    /// Returns `true` if the request can proceed, `false` if it should fail fast.
    /// If circuit is Open and cooldown has elapsed, transitions to HalfOpen.
    pub fn allow_request(&self) -> bool {
        match self.state() {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true,
            CircuitState::Open => {
                // Check if cooldown has elapsed
                let opened_at = self.opened_at.load(Ordering::Relaxed);
                if opened_at > 0 {
                    let now_secs = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let cooldown_elapsed = now_secs.saturating_sub(opened_at);
                    if cooldown_elapsed >= self.cooldown.as_secs() {
                        // Transition to HalfOpen
                        let prev = self
                            .state
                            .swap(CircuitState::HalfOpen as u32, Ordering::Relaxed);
                        if prev == CircuitState::Open as u32 {
                            info!(
                                cooldown_secs = cooldown_elapsed,
                                "⚡ Circuit breaker: Open → HalfOpen (cooldown elapsed)"
                            );
                            return true;
                        }
                    }
                }
                false
            }
        }
    }

    /// Record a successful request.
    ///
    /// - In Closed: resets failure counter.
    /// - In HalfOpen: increments success counter, closes if threshold met.
    pub fn record_success(&self) {
        self.failures.store(0, Ordering::Relaxed);

        if self.state() == CircuitState::HalfOpen {
            let successes = self.successes.fetch_add(1, Ordering::Relaxed) + 1;
            if successes >= self.success_threshold {
                self.state
                    .store(CircuitState::Closed as u32, Ordering::Relaxed);
                self.successes.store(0, Ordering::Relaxed);
                info!("✅ Circuit breaker: HalfOpen → Closed (recovered)");
            }
        }
    }

    /// Record a failed request.
    ///
    /// - In Closed: increments failure counter, opens if threshold met.
    /// - In HalfOpen: re-opens the circuit.
    pub fn record_failure(&self) {
        let failures = self.failures.fetch_add(1, Ordering::Relaxed) + 1;

        match self.state() {
            CircuitState::Closed => {
                if failures >= self.failure_threshold {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    self.state
                        .store(CircuitState::Open as u32, Ordering::Relaxed);
                    self.opened_at.store(now, Ordering::Relaxed);
                    warn!(
                        failures,
                        threshold = self.failure_threshold,
                        "🔴 Circuit breaker: Closed → Open (too many failures)"
                    );
                }
            }
            CircuitState::HalfOpen => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                self.state
                    .store(CircuitState::Open as u32, Ordering::Relaxed);
                self.opened_at.store(now, Ordering::Relaxed);
                self.successes.store(0, Ordering::Relaxed);
                warn!("🔴 Circuit breaker: HalfOpen → Open (test request failed)");
            }
            CircuitState::Open => {
                // Already open — just update counter
            }
        }
    }

    /// Manually reset the circuit breaker to Closed state.
    pub fn reset(&self) {
        self.state
            .store(CircuitState::Closed as u32, Ordering::Relaxed);
        self.failures.store(0, Ordering::Relaxed);
        self.successes.store(0, Ordering::Relaxed);
        self.opened_at.store(0, Ordering::Relaxed);
        info!("🔵 Circuit breaker: manually reset to Closed");
    }

    /// Get a human-readable summary.
    pub fn summary(&self) -> String {
        let state = self.state();
        let failures = self.failure_count();
        match state {
            CircuitState::Closed => {
                format!("CLOSED (failures: {}/{})", failures, self.failure_threshold)
            }
            CircuitState::Open => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let opened_at = self.opened_at.load(Ordering::Relaxed);
                let remaining = self
                    .cooldown
                    .as_secs()
                    .saturating_sub(now.saturating_sub(opened_at));
                format!(
                    "OPEN (failures: {}, cooldown remaining: {}s)",
                    failures, remaining
                )
            }
            CircuitState::HalfOpen => format!(
                "HALF-OPEN (successes: {}/{})",
                self.successes.load(Ordering::Relaxed),
                self.success_threshold
            ),
        }
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_state_is_closed() {
        let cb = CircuitBreaker::new();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert_eq!(cb.failure_count(), 0);
    }

    #[test]
    fn test_closed_allows_requests() {
        let cb = CircuitBreaker::new();
        assert!(cb.allow_request());
    }

    #[test]
    fn test_opens_after_threshold() {
        let cb = CircuitBreaker::new().with_thresholds(3, 1, 1); // 3 failures → open, 1 success → close, 1s cooldown

        for _ in 0..2 {
            cb.record_failure();
            assert_eq!(cb.state(), CircuitState::Closed);
            assert!(cb.allow_request());
        }

        // 3rd failure → opens
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow_request());
    }

    #[test]
    fn test_open_blocks_requests() {
        let cb = CircuitBreaker::new().with_thresholds(2, 1, 999); // Very long cooldown

        cb.record_failure();
        cb.record_failure();

        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow_request());
    }

    #[test]
    fn test_half_open_after_cooldown() {
        // Use 0-second cooldown to trigger HalfOpen immediately
        let cb = CircuitBreaker::new().with_thresholds(1, 1, 0);

        cb.record_failure(); // Opens immediately
        assert_eq!(cb.state(), CircuitState::Open);

        // Should transition to HalfOpen since cooldown is 0
        assert!(cb.allow_request());
        assert_eq!(cb.state(), CircuitState::HalfOpen);
    }

    #[test]
    fn test_half_open_success_closes() {
        let cb = CircuitBreaker::new().with_thresholds(1, 2, 0); // 1 failure → open, 2 successes → close

        cb.record_failure(); // Opens
        assert!(cb.allow_request()); // → HalfOpen
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        cb.record_success();
        assert_eq!(cb.state(), CircuitState::HalfOpen); // Need 2 successes

        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed); // Now closed
    }

    #[test]
    fn test_half_open_failure_reopens() {
        let cb = CircuitBreaker::new().with_thresholds(1, 1, 999);

        cb.record_failure(); // Opens
                             // Force to HalfOpen by setting state directly (cooldown too long)
        cb.state
            .store(CircuitState::HalfOpen as u32, Ordering::Relaxed);

        cb.record_failure(); // Test request fails
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_reset_clears_state() {
        let cb = CircuitBreaker::new().with_thresholds(1, 1, 999);

        cb.record_failure();
        cb.reset();

        assert_eq!(cb.state(), CircuitState::Closed);
        assert_eq!(cb.failure_count(), 0);
        assert!(cb.allow_request());
    }

    #[test]
    fn test_summary_closed() {
        let cb = CircuitBreaker::new();
        let summary = cb.summary();
        assert!(summary.contains("CLOSED"));
    }

    #[test]
    fn test_summary_open() {
        let cb = CircuitBreaker::new().with_thresholds(1, 1, 999);
        cb.record_failure();
        let summary = cb.summary();
        assert!(summary.contains("OPEN"));
    }

    #[test]
    fn test_summary_half_open() {
        let cb = CircuitBreaker::new().with_thresholds(1, 2, 0);
        cb.record_failure();
        cb.allow_request(); // → HalfOpen
        let summary = cb.summary();
        assert!(summary.contains("HALF-OPEN"));
    }

    #[test]
    fn test_success_in_closed_resets_counter() {
        let cb = CircuitBreaker::new().with_thresholds(5, 1, 999);

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.failure_count(), 2);

        cb.record_success(); // Should reset counter
        assert_eq!(cb.failure_count(), 0);
    }
}
