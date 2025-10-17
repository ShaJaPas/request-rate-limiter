//! Dynamic request rate limiting with adaptive algorithms for controlling request throughput.

#![deny(missing_docs)]

#[cfg(doctest)]
use doc_comment::doctest;
#[cfg(doctest)]
doctest!("../README.md");

pub mod algorithms;
pub mod limiter;

#[cfg(feature = "key")]
pub mod keyed;

// Re-export main types for convenience
pub use limiter::{DefaultRateLimiter, RateLimiter, RequestOutcome, Token};

#[cfg(feature = "key")]
pub use keyed::{DefaultRateLimiterKeyed, RateLimiterKeyed};
