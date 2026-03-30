//! Scheduled pipeline execution.
//!
//! Port from Python `scheduler/scheduler.py`.
//! Uses tokio cron for periodic automated runs
//! with graceful shutdown and logging.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::signal;
use tracing::{error, info, warn};

/// Parsed cron schedule.
#[derive(Debug, Clone)]
pub struct CronSchedule {
    pub minute: String,
    pub hour: String,
    pub day: String,
    pub month: String,
    pub day_of_week: String,
}

impl CronSchedule {
    /// Parse a 5-field cron expression.
    pub fn parse(cron_expr: &str) -> Result<Self, String> {
        let parts: Vec<&str> = cron_expr.trim().split_whitespace().collect();
        if parts.len() != 5 {
            return Err(format!(
                "Invalid cron expression: {:?}. Expected 5 fields: minute hour day month day_of_week",
                cron_expr
            ));
        }
        Ok(Self {
            minute: parts[0].into(),
            hour: parts[1].into(),
            day: parts[2].into(),
            month: parts[3].into(),
            day_of_week: parts[4].into(),
        })
    }

    /// Convert to seconds until next trigger (simplified: calculate from hour/minute).
    pub fn seconds_until_next(&self) -> u64 {
        use chrono::{Local, Timelike};
        let now = Local::now();
        let current_minutes = now.hour() as u64 * 60 + now.minute() as u64;

        let target_hour: u64 = self.hour.parse().unwrap_or(0);
        let target_minute: u64 = self.minute.parse().unwrap_or(0);
        let target_minutes = target_hour * 60 + target_minute;

        let minutes_until = if target_minutes > current_minutes {
            target_minutes - current_minutes
        } else {
            24 * 60 - current_minutes + target_minutes
        };

        minutes_until * 60
    }
}

/// Scheduler for automated pipeline runs.
pub struct ContribScheduler {
    cron: CronSchedule,
    enabled: bool,
    running: Arc<AtomicBool>,
}

impl ContribScheduler {
    pub fn new(cron_expr: &str, enabled: bool) -> Result<Self, String> {
        let cron = CronSchedule::parse(cron_expr)?;
        Ok(Self {
            cron,
            enabled,
            running: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Start the scheduler (blocking).
    ///
    /// `run_fn` is called on each trigger. It receives no args
    /// and returns a Result indicating success/failure.
    pub async fn start<F, Fut>(&self, run_fn: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), String>> + Send,
    {
        if !self.enabled {
            warn!("Scheduler is disabled. Set scheduler.enabled=true to enable.");
            return;
        }

        self.running.store(true, Ordering::SeqCst);
        let running = self.running.clone();

        info!(
            cron = format!("{} {} {} {} {}", self.cron.minute, self.cron.hour, self.cron.day, self.cron.month, self.cron.day_of_week),
            "Scheduler started"
        );

        // Simple loop: sleep until next trigger, then run
        loop {
            let wait_secs = self.cron.seconds_until_next();
            info!(
                next_run_in_minutes = wait_secs / 60,
                "Waiting for next scheduled run"
            );

            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(wait_secs)) => {
                    if !running.load(Ordering::SeqCst) {
                        break;
                    }
                    info!("Scheduled pipeline run starting...");
                    match run_fn().await {
                        Ok(()) => info!("Scheduled run complete"),
                        Err(e) => error!(error = %e, "Scheduled pipeline run failed"),
                    }
                }
                _ = signal::ctrl_c() => {
                    info!("Received shutdown signal");
                    running.store(false, Ordering::SeqCst);
                    break;
                }
            }
        }

        info!("Scheduler stopped.");
    }

    /// Stop the scheduler (can be called from another task).
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        info!("Scheduler stop requested.");
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cron_valid() {
        let cron = CronSchedule::parse("0 3 * * *").unwrap();
        assert_eq!(cron.minute, "0");
        assert_eq!(cron.hour, "3");
        assert_eq!(cron.day, "*");
        assert_eq!(cron.month, "*");
        assert_eq!(cron.day_of_week, "*");
    }

    #[test]
    fn test_parse_cron_invalid_fields() {
        assert!(CronSchedule::parse("0 3 *").is_err());
        assert!(CronSchedule::parse("0 3 * * * *").is_err());
    }

    #[test]
    fn test_scheduler_disabled() {
        let sched = ContribScheduler::new("0 3 * * *", false).unwrap();
        assert!(!sched.is_running());
    }

    #[test]
    fn test_scheduler_enabled() {
        let sched = ContribScheduler::new("0 3 * * *", true).unwrap();
        assert!(!sched.is_running()); // not started yet
    }

    #[test]
    fn test_scheduler_stop() {
        let sched = ContribScheduler::new("0 3 * * *", true).unwrap();
        sched.running.store(true, Ordering::SeqCst);
        assert!(sched.is_running());
        sched.stop();
        assert!(!sched.is_running());
    }

    #[test]
    fn test_seconds_until_next() {
        let cron = CronSchedule::parse("0 3 * * *").unwrap();
        let secs = cron.seconds_until_next();
        // Should be between 0 and 24*60*60
        assert!(secs <= 24 * 60 * 60);
    }
}
