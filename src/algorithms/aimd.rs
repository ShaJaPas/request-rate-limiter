use std::{
    ops::RangeInclusive,
    sync::atomic::{AtomicU64, Ordering},
};

use async_trait::async_trait;
use conv::ConvUtil;

use crate::{algorithms::RequestSample, limiter::RequestOutcome};

use super::RateLimitAlgorithm;

/// Loss-based request rate limiting.
///
/// Additive-increase, multiplicative decrease for requests per second.
///
/// Increases request rate when:
/// 1. no overload errors are observed, and
/// 2. requests are successful.
///
/// Reduces request rate by a factor when overload is detected.
#[derive(Debug)]
pub struct Aimd {
    min_rps: u64,
    max_rps: u64,
    decrease_factor: f64,
    increase_by: u64,

    requests_per_second: AtomicU64,
}

impl Clone for Aimd {
    fn clone(&self) -> Self {
        Self {
            min_rps: self.min_rps,
            max_rps: self.max_rps,
            decrease_factor: self.decrease_factor,
            increase_by: self.increase_by,
            requests_per_second: AtomicU64::new(self.requests_per_second.load(Ordering::Acquire)),
        }
    }
}

impl Aimd {
    const DEFAULT_DECREASE_FACTOR: f64 = 0.8;
    const DEFAULT_INCREASE: u64 = 10;

    #[allow(missing_docs)]
    pub fn new_with_initial_rate(initial_rps: u64) -> Self {
        Self::new(initial_rps, 1..=10000)
    }

    #[allow(missing_docs)]
    pub fn new(initial_rps: u64, rate_range: RangeInclusive<u64>) -> Self {
        assert!(*rate_range.start() >= 1, "Rate must be at least 1");
        assert!(
            initial_rps >= *rate_range.start(),
            "Initial rate less than minimum"
        );
        assert!(
            initial_rps <= *rate_range.end(),
            "Initial rate more than maximum"
        );

        Self {
            min_rps: *rate_range.start(),
            max_rps: *rate_range.end(),
            decrease_factor: Self::DEFAULT_DECREASE_FACTOR,
            increase_by: Self::DEFAULT_INCREASE,

            requests_per_second: std::sync::atomic::AtomicU64::new(initial_rps),
        }
    }

    /// Set the multiplier which will be applied when decreasing the rate.
    pub fn decrease_factor(self, factor: f64) -> Self {
        assert!((0.1..1.0).contains(&factor));
        Self {
            decrease_factor: factor,
            ..self
        }
    }

    /// Set the increment which will be applied when increasing the rate.
    pub fn increase_by(self, increase: u64) -> Self {
        assert!(increase > 0);
        Self {
            increase_by: increase,
            ..self
        }
    }

    #[allow(missing_docs)]
    pub fn with_max_rate(self, max: u64) -> Self {
        assert!(max > 0);
        Self {
            max_rps: max,
            ..self
        }
    }
}

#[async_trait]
impl RateLimitAlgorithm for Aimd {
    fn requests_per_second(&self) -> u64 {
        self.requests_per_second.load(Ordering::Acquire)
    }

    async fn update(&self, sample: RequestSample) -> u64 {
        use RequestOutcome::*;
        match sample.outcome {
            Success | ClientError => self
                .requests_per_second
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |rps| {
                    let new_rps = (rps + self.increase_by).min(self.max_rps);
                    Some(new_rps)
                })
                .expect("we always return Some(rps)"),
            Overload => self
                .requests_per_second
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |rps| {
                    let new_rps =
                        multiplicative_decrease(rps, self.decrease_factor).max(self.min_rps);
                    Some(new_rps)
                })
                .expect("we always return Some(rps)"),
        }
    }
}

pub(super) fn multiplicative_decrease(rps: u64, decrease_factor: f64) -> u64 {
    assert!(decrease_factor <= 1.0, "should not increase the rate");

    let new_rps = rps as f64 * decrease_factor;

    // Floor instead of round, so the rate reduces even with small numbers.
    // E.g. round(2 * 0.9) = 2, but floor(2 * 0.9) = 1
    new_rps.floor().approx_as::<u64>().unwrap_or(1).max(1)
}

#[cfg(test)]
mod tests {

    use crate::limiter::{DefaultRateLimiter, RateLimiter, RequestOutcome};

    use super::*;

    #[tokio::test]
    async fn should_decrease_rate_on_overload() {
        let aimd = Aimd::new_with_initial_rate(100)
            .decrease_factor(0.5)
            .increase_by(10);

        let limiter = DefaultRateLimiter::new(aimd);

        let token = limiter.acquire().await;
        limiter.release(token, Some(RequestOutcome::Overload)).await;

        let state = limiter.state();
        assert_eq!(
            state.requests_per_second(),
            50,
            "overload: should decrease rate"
        );
    }

    #[tokio::test]
    async fn should_increase_rate_on_success() {
        let aimd = Aimd::new_with_initial_rate(50)
            .decrease_factor(0.5)
            .increase_by(20);

        let limiter = DefaultRateLimiter::new(aimd);

        let token = limiter.acquire().await;
        limiter.release(token, Some(RequestOutcome::Success)).await;

        let state = limiter.state();
        assert_eq!(
            state.requests_per_second(),
            70,
            "success: should increase rate"
        );
    }

    #[tokio::test]
    async fn should_not_change_rate_when_no_outcome() {
        let aimd = Aimd::new_with_initial_rate(100)
            .decrease_factor(0.5)
            .increase_by(10);

        let limiter = DefaultRateLimiter::new(aimd);

        let token = limiter.acquire().await;
        limiter.release(token, None).await;

        let state = limiter.state();
        assert_eq!(
            state.requests_per_second(),
            100,
            "should ignore when no outcome"
        );
    }
}
