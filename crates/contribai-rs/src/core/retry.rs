//! Exponential backoff retry logic.

use std::time::Duration;
use tokio::time::sleep;
use tracing::warn;

/// Retry an async operation with exponential backoff.
pub async fn retry_with_backoff<F, Fut, T, E>(
    operation_name: &str,
    max_retries: u32,
    mut f: F,
) -> std::result::Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, E>>,
    E: std::fmt::Display,
{
    let mut attempt = 0;
    loop {
        match f().await {
            Ok(val) => return Ok(val),
            Err(e) => {
                attempt += 1;
                if attempt >= max_retries {
                    warn!(
                        "{}: failed after {} retries: {}",
                        operation_name, max_retries, e
                    );
                    return Err(e);
                }
                let delay = Duration::from_millis(500 * 2u64.pow(attempt));
                warn!(
                    "{}: attempt {}/{} failed ({}), retrying in {:?}",
                    operation_name, attempt, max_retries, e, delay
                );
                sleep(delay).await;
            }
        }
    }
}
