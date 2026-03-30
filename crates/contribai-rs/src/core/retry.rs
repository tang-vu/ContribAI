//! Retry utilities with exponential backoff and LRU cache.
//!
//! Port from Python `core/retry.py`.

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;
use tracing::warn;

/// Async retry with exponential backoff + jitter.
pub async fn async_retry<F, Fut, T, E>(
    f: F,
    max_retries: u32,
    base_delay: f64,
    max_delay: f64,
    backoff_factor: f64,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut last_error: Option<E> = None;
    for attempt in 0..=max_retries {
        match f().await {
            Ok(val) => return Ok(val),
            Err(e) => {
                if attempt >= max_retries {
                    last_error = Some(e);
                    break;
                }
                let delay = (base_delay * backoff_factor.powi(attempt as i32)).min(max_delay);
                // jitter ±25%
                let jitter = 0.75 + rand_f64() * 0.5;
                let final_delay = delay * jitter;
                warn!(
                    attempt = attempt + 1,
                    max = max_retries,
                    delay_sec = final_delay,
                    error = %e,
                    "Retrying"
                );
                tokio::time::sleep(Duration::from_secs_f64(final_delay)).await;
                last_error = Some(e);
            }
        }
    }
    Err(last_error.unwrap())
}

/// Simple pseudo-random f64 in [0, 1) using time-based seed.
fn rand_f64() -> f64 {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos % 1000) as f64 / 1000.0
}

/// GitHub API retry preset: 3 retries, 2s base, 60s max.
pub async fn github_retry<F, Fut, T, E>(f: F) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    async_retry(f, 3, 2.0, 60.0, 2.0).await
}

/// LLM API retry preset: 3 retries, 3s base, 60s max.
pub async fn llm_retry<F, Fut, T, E>(f: F) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    async_retry(f, 3, 3.0, 60.0, 2.0).await
}

/// Rate limit retry preset: 5 retries, 10s base, 120s max.
pub async fn rate_limit_retry<F, Fut, T, E>(f: F) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    async_retry(f, 5, 10.0, 120.0, 2.0).await
}

// ── LRU Cache ────────────────────────────────────────

/// Simple LRU cache for API/LLM responses.
pub struct LruCache<V> {
    entries: HashMap<String, V>,
    order: Vec<String>,
    max_size: usize,
    hits: u64,
    misses: u64,
}

impl<V: Clone> LruCache<V> {
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: Vec::new(),
            max_size,
            hits: 0,
            misses: 0,
        }
    }

    pub fn get(&mut self, key: &str) -> Option<V> {
        if let Some(val) = self.entries.get(key) {
            self.hits += 1;
            // Move to end (most recently used)
            self.order.retain(|k| k != key);
            self.order.push(key.to_string());
            Some(val.clone())
        } else {
            self.misses += 1;
            None
        }
    }

    pub fn put(&mut self, key: String, value: V) {
        if self.entries.contains_key(&key) {
            self.order.retain(|k| k != &key);
        } else if self.entries.len() >= self.max_size {
            // Evict LRU
            if let Some(oldest) = self.order.first().cloned() {
                self.entries.remove(&oldest);
                self.order.remove(0);
            }
        }
        self.entries.insert(key.clone(), value);
        self.order.push(key);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.hits = 0;
        self.misses = 0;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        self.hits as f64 / total as f64 * 100.0
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits,
            misses: self.misses,
            hit_rate: self.hit_rate(),
            size: self.entries.len(),
            max_size: self.max_size,
        }
    }
}

#[derive(Debug)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub hit_rate: f64,
    pub size: usize,
    pub max_size: usize,
}

/// Make a cache key from arbitrary args via hashing.
pub fn make_cache_key(args: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    args.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lru_cache_basic() {
        let mut cache = LruCache::new(3);
        cache.put("a".into(), 1);
        cache.put("b".into(), 2);
        cache.put("c".into(), 3);
        assert_eq!(cache.get("a"), Some(1));
        assert_eq!(cache.get("d"), None);
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn test_lru_cache_eviction() {
        let mut cache = LruCache::new(2);
        cache.put("a".into(), 1);
        cache.put("b".into(), 2);
        cache.put("c".into(), 3); // should evict "a"
        assert_eq!(cache.get("a"), None);
        assert_eq!(cache.get("b"), Some(2));
        assert_eq!(cache.get("c"), Some(3));
    }

    #[test]
    fn test_lru_cache_update() {
        let mut cache = LruCache::new(2);
        cache.put("a".into(), 1);
        cache.put("b".into(), 2);
        cache.put("a".into(), 10); // update "a"
        cache.put("c".into(), 3); // should evict "b" (LRU)
        assert_eq!(cache.get("a"), Some(10));
        assert_eq!(cache.get("b"), None);
    }

    #[test]
    fn test_lru_cache_stats() {
        let mut cache = LruCache::new(10);
        cache.put("a".into(), 1);
        cache.get("a"); // hit
        cache.get("b"); // miss
        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_lru_cache_clear() {
        let mut cache = LruCache::new(10);
        cache.put("a".into(), 1);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_make_cache_key() {
        let k1 = make_cache_key("test args");
        let k2 = make_cache_key("test args");
        let k3 = make_cache_key("other args");
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[tokio::test]
    async fn test_async_retry_succeeds_first_try() {
        let result: Result<i32, String> =
            async_retry(|| async { Ok(42) }, 3, 0.01, 1.0, 2.0).await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_async_retry_all_fail() {
        let result: Result<i32, String> =
            async_retry(|| async { Err("fail".to_string()) }, 2, 0.01, 1.0, 2.0).await;
        assert!(result.is_err());
    }
}
