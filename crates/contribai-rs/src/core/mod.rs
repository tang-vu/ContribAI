pub mod config;
pub mod error;
pub mod events;
pub mod leaderboard;
pub mod middleware;
pub mod models;
pub mod profiles;
pub mod quotas;
pub mod retry;

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
