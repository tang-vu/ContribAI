pub mod config;
pub mod crypto;
pub mod error;
pub mod events;
pub mod i18n;
pub mod leaderboard;
pub mod logging;
pub mod middleware;
pub mod models;
pub mod permissions;
pub mod plugins;
pub mod profiles;
pub mod prompt_sanitize;
pub mod quotas;
pub mod retry;
pub mod snapshots;

/// Truncate a string at a char boundary, never panicking.
/// Returns a slice of at most `max_bytes` bytes, ending on a valid UTF-8 boundary.
#[inline]
pub fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
