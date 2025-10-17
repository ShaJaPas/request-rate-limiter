//! Rate limiters for controlling request throughput.

use std::{
    fmt::Debug,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use tokio::time::{sleep, timeout};

use crate::algorithms::{RateLimitAlgorithm, RequestSample};

type RequestCount = u64;
type AtomicRequestCount = AtomicU64;

/// A token representing permission to make a request.
/// The token tracks when the request was started for timing measurements.
#[derive(Debug)]
pub struct Token {
    start_time: Instant,
}

/// Controls the rate of requests over time.
///
/// Rate limiting is achieved by checking if a request is allowed based on the current
/// rate limit algorithm. The limiter tracks request patterns and adjusts limits dynamically
/// based on observed success/failure rates and response times.
#[async_trait]
pub trait RateLimiter: Debug + Sync {
    /// Acquire permission to make a request. Waits until a token is available.
    async fn acquire(&self) -> Token;

    /// Acquire permission to make a request with a timeout. Returns a token if successful.
    async fn acquire_timeout(&self, duration: Duration) -> Option<Token>;

    /// Release the token and record the outcome of the request.
    /// The response time is calculated from when the token was acquired.
    async fn release(&self, token: Token, outcome: Option<RequestOutcome>);
}

/// A token bucket based rate limiter.
///
/// Cheaply cloneable.
#[derive(Debug)]
pub struct DefaultRateLimiter<T> {
    algorithm: T,
    tokens: Arc<AtomicRequestCount>,
    last_refill: Arc<std::sync::Mutex<Instant>>,
    requests_per_second: Arc<AtomicRequestCount>,
    bucket_capacity: RequestCount,
}

/// A snapshot of the state of the rate limiter.
///
/// Not guaranteed to be consistent under high concurrency.
#[derive(Debug, Clone, Copy)]
pub struct RateLimiterState {
    /// Current requests per second limit
    requests_per_second: RequestCount,
    /// Available tokens in the bucket
    available_tokens: RequestCount,
    /// Maximum bucket capacity
    bucket_capacity: RequestCount,
}

/// Whether a request succeeded or failed, potentially due to overload.
///
/// Errors not considered to be caused by overload should be ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestOutcome {
    /// The request succeeded, or failed in a way unrelated to overload.
    Success,
    /// The request failed because of overload, e.g. it timed out or received a 429/503 response.
    Overload,
    /// The request failed due to client error (4xx) - not related to rate limiting.
    ClientError,
}

impl<T> DefaultRateLimiter<T>
where
    T: RateLimitAlgorithm,
{
    /// Create a rate limiter with a given rate limiting algorithm.
    pub fn new(algorithm: T) -> Self {
        let initial_rps = algorithm.requests_per_second();
        let bucket_capacity = initial_rps; // Use the same value for bucket capacity

        assert!(initial_rps >= 1);
        Self {
            algorithm,
            tokens: Arc::new(AtomicRequestCount::new(bucket_capacity)),
            last_refill: Arc::new(std::sync::Mutex::new(Instant::now())),
            requests_per_second: Arc::new(AtomicRequestCount::new(initial_rps)),
            bucket_capacity,
        }
    }

    fn refill_tokens(&self) {
        let now = Instant::now();
        if let Ok(mut last_refill) = self.last_refill.try_lock() {
            let elapsed = now.duration_since(*last_refill);
            let tokens_to_add = (elapsed.as_secs_f64()
                * self.requests_per_second.load(Ordering::Acquire) as f64)
                as u64;

            if tokens_to_add > 0 {
                let current_tokens = self.tokens.load(Ordering::Acquire);
                let new_tokens = (current_tokens + tokens_to_add).min(self.bucket_capacity);
                self.tokens.store(new_tokens, Ordering::SeqCst);
                *last_refill = now;
            }
        }
    }

    /// The current state of the rate limiter.
    pub fn state(&self) -> RateLimiterState {
        self.refill_tokens();
        RateLimiterState {
            requests_per_second: self.requests_per_second.load(Ordering::Acquire),
            available_tokens: self.tokens.load(Ordering::Acquire),
            bucket_capacity: self.bucket_capacity,
        }
    }
}

#[async_trait]
impl<T> RateLimiter for DefaultRateLimiter<T>
where
    T: RateLimitAlgorithm + Sync + Debug,
{
    async fn acquire(&self) -> Token {
        loop {
            self.refill_tokens();

            // Try to consume a token atomically
            let current_tokens = self.tokens.load(Ordering::Acquire);
            if current_tokens > 0 {
                match self.tokens.compare_exchange_weak(
                    current_tokens,
                    current_tokens - 1,
                    Ordering::Release,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        return Token {
                            start_time: Instant::now(),
                        };
                    }
                    Err(_) => continue, // Retry on contention
                }
            } else {
                // No tokens available, wait a bit before retrying
                sleep(Duration::from_millis(1)).await;
            }
        }
    }

    async fn acquire_timeout(&self, duration: Duration) -> Option<Token> {
        timeout(duration, self.acquire()).await.ok()
    }

    async fn release(&self, token: Token, outcome: Option<RequestOutcome>) {
        let response_time = token.start_time.elapsed();

        if let Some(outcome) = outcome {
            let current_rps = self.requests_per_second.load(Ordering::Acquire);
            let sample = RequestSample::new(response_time, current_rps, outcome);

            let new_rps = self.algorithm.update(sample).await;
            self.requests_per_second.store(new_rps, Ordering::Release);
        }
    }
}

impl RateLimiterState {
    /// The current requests per second limit.
    pub fn requests_per_second(&self) -> RequestCount {
        self.requests_per_second
    }
    /// The number of available tokens in the bucket.
    pub fn available_tokens(&self) -> RequestCount {
        self.available_tokens
    }
    /// The maximum bucket capacity.
    pub fn bucket_capacity(&self) -> RequestCount {
        self.bucket_capacity
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        algorithms::Fixed,
        limiter::{DefaultRateLimiter, RateLimiter, RequestOutcome},
    };
    use std::time::Duration;

    #[tokio::test]
    async fn rate_limiter_allows_requests_within_limit() {
        let limiter = DefaultRateLimiter::new(Fixed::new(10));

        // Should allow first request
        let token = limiter.acquire().await;

        // Release with successful outcome
        limiter.release(token, Some(RequestOutcome::Success)).await;
    }

    #[tokio::test]
    async fn rate_limiter_waits_for_tokens() {
        use std::sync::Arc;

        let limiter = Arc::new(DefaultRateLimiter::new(Fixed::new(1)));

        // Consume the only token
        let token1 = limiter.acquire().await;

        // Start acquiring second token (should wait)
        let limiter_clone = Arc::clone(&limiter);
        let acquire_task = tokio::spawn(async move { limiter_clone.acquire().await });

        // Give it a moment to start waiting
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Release the first token - this should allow the second acquire to complete
        limiter.release(token1, Some(RequestOutcome::Success)).await;

        // The second acquire should now complete
        let token2 = acquire_task.await.unwrap();
        limiter.release(token2, Some(RequestOutcome::Success)).await;
    }
}
