//! Algorithms for controlling request rate limits.

/// Additive Increase Multiplicative Decrease algorithm.
mod aimd;
mod fixed;

use async_trait::async_trait;
use std::time::Duration;

use crate::limiter::RequestOutcome;

pub use aimd::Aimd;
pub use fixed::Fixed;

/// An algorithm for controlling request rate limits.
#[async_trait]
pub trait RateLimitAlgorithm {
    /// The current requests per second limit.
    fn requests_per_second(&self) -> u64;

    /// Update the rate limit in response to a request completion.
    async fn update(&self, sample: RequestSample) -> u64;
}

/// The result of a request, including the [RequestOutcome] and response time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestSample {
    /// Response time for the request
    pub response_time: Duration,
    /// Current requests per second when the sample was taken
    pub current_rps: u64,
    /// Outcome of the request
    pub outcome: RequestOutcome,
    /// Timestamp when the request was made
    pub timestamp: std::time::Instant,
}

impl RequestSample {
    /// Create a new request sample
    pub fn new(response_time: Duration, current_rps: u64, outcome: RequestOutcome) -> Self {
        Self {
            response_time,
            current_rps,
            outcome,
            timestamp: std::time::Instant::now(),
        }
    }
}
