//! Keyed rate limiting functionality.
//!
//! This module provides rate limiting with per-key isolation, allowing independent
//! rate limiting across different clients, users, or request types.

use std::{fmt::Debug, hash::Hash, time::Duration};

use async_trait::async_trait;
use dashmap::{mapref::one::Ref, DashMap};

use crate::{
    algorithms::RateLimitAlgorithm,
    limiter::{DefaultRateLimiter, RateLimiter, RequestOutcome, Token},
};

/// Controls the rate of requests over time with per-key rate limiting.
///
/// Each key maintains its own rate limit state, allowing for independent
/// rate limiting across different clients, users, or request types.
#[async_trait]
pub trait RateLimiterKeyed<K>: Sync
where
    K: Hash + Eq + Send + Sync,
{
    /// Acquire permission to make a request for a specific key. Waits until a token is available.
    async fn acquire(&self, key: &K) -> Token;

    /// Acquire permission to make a request for a specific key with a timeout.
    /// Returns a token if successful.
    async fn acquire_timeout(&self, key: &K, duration: Duration) -> Option<Token>;

    /// Release the token and record the outcome of the request for the specific key.
    /// The response time is calculated from when the token was acquired.
    async fn release(&self, key: &K, token: Token, outcome: Option<RequestOutcome>);
}

/// A keyed rate limiter that maintains separate rate limiters for each key.
///
/// Uses DashMap for efficient concurrent access to per-key rate limiters.
/// Each key gets its own independent rate limiter instance.
pub struct DefaultRateLimiterKeyed<T, K, F>
where
    T: RateLimitAlgorithm + Debug,
    K: Hash + Eq + Send + Sync,
    F: Fn() -> T + Send + Sync,
{
    limiters: DashMap<K, DefaultRateLimiter<T>>,
    algorithm_factory: F,
}

impl<T, K, F> DefaultRateLimiterKeyed<T, K, F>
where
    T: RateLimitAlgorithm + Debug,
    K: Hash + Eq + Clone + Send + Sync,
    F: Fn() -> T + Send + Sync,
{
    /// Create a new keyed rate limiter with the given algorithm factory function.
    /// Each key will get a fresh instance of the algorithm created by calling the factory.
    pub fn new(algorithm_factory: F) -> Self {
        Self {
            limiters: DashMap::new(),
            algorithm_factory,
        }
    }

    /// Get or create a rate limiter for the given key.
    fn get_or_create_limiter(&self, key: &K) -> Ref<'_, K, DefaultRateLimiter<T>> {
        if !self.limiters.contains_key(key) {
            self.limiters.insert(
                key.clone(),
                DefaultRateLimiter::new((self.algorithm_factory)()),
            );
        }

        self.limiters.get(key).unwrap()
    }

    /// Get the number of active keys being tracked.
    pub fn active_keys(&self) -> usize {
        self.limiters.len()
    }

    /// Remove a key and its associated rate limiter.
    /// Returns true if the key existed and was removed.
    pub fn remove_key(&self, key: &K) -> bool {
        self.limiters.remove(key).is_some()
    }

    /// Clear all keys and their associated rate limiters.
    pub fn clear(&self) {
        self.limiters.clear();
    }
}

#[async_trait]
impl<T, K, F> RateLimiterKeyed<K> for DefaultRateLimiterKeyed<T, K, F>
where
    T: RateLimitAlgorithm + Send + Sync + Debug,
    K: Hash + Eq + Clone + Send + Sync,
    F: Fn() -> T + Send + Sync,
{
    async fn acquire(&self, key: &K) -> Token {
        let limiter = self.get_or_create_limiter(key);
        limiter.acquire().await
    }

    async fn acquire_timeout(&self, key: &K, duration: Duration) -> Option<Token> {
        let limiter = self.get_or_create_limiter(key);
        limiter.acquire_timeout(duration).await
    }

    async fn release(&self, key: &K, token: Token, outcome: Option<RequestOutcome>) {
        let limiter = self.get_or_create_limiter(key);
        limiter.release(token, outcome).await
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        algorithms::Fixed,
        keyed::{DefaultRateLimiterKeyed, RateLimiterKeyed},
        limiter::RequestOutcome,
    };

    #[tokio::test]
    async fn keyed_rate_limiter_works_independently_per_key() {
        let limiter = DefaultRateLimiterKeyed::<_, String, _>::new(|| Fixed::new(1));

        let key1 = "key1".to_string();
        let key2 = "key2".to_string();
        // Acquire tokens for different keys - should work independently
        let token1 = limiter.acquire(&key1).await;
        let token2 = limiter.acquire(&key2).await;

        // Both should succeed because they're different keys
        limiter
            .release(&key1, token1, Some(RequestOutcome::Success))
            .await;
        limiter
            .release(&key2, token2, Some(RequestOutcome::Success))
            .await;

        assert_eq!(limiter.active_keys(), 2);
    }

    #[tokio::test]
    async fn keyed_rate_limiter_manages_keys() {
        let limiter = DefaultRateLimiterKeyed::<_, String, _>::new(|| Fixed::new(10));

        // Create limiters for multiple keys
        let _token1 = limiter.acquire(&"user1".to_string()).await;
        let _token2 = limiter.acquire(&"user2".to_string()).await;
        let _token3 = limiter.acquire(&"user3".to_string()).await;

        assert_eq!(limiter.active_keys(), 3);

        // Remove one key
        assert!(limiter.remove_key(&"user2".to_string()));
        assert_eq!(limiter.active_keys(), 2);

        // Try to remove non-existent key
        assert!(!limiter.remove_key(&"nonexistent".to_string()));

        // Clear all keys
        limiter.clear();
        assert_eq!(limiter.active_keys(), 0);
    }
}
