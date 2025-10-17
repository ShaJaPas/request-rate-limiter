use async_trait::async_trait;

use super::{RateLimitAlgorithm, RequestSample};

/// A simple, fixed request rate limit.
#[derive(Debug, Clone)]
pub struct Fixed(u64);
impl Fixed {
    #[allow(missing_docs)]
    pub fn new(rps: u64) -> Self {
        assert!(rps > 0);

        Self(rps)
    }
}

#[async_trait]
impl RateLimitAlgorithm for Fixed {
    fn requests_per_second(&self) -> u64 {
        self.0
    }

    async fn update(&self, _sample: RequestSample) -> u64 {
        self.0
    }
}
