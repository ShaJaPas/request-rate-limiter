# request-rate-limiter

Dynamic request rate limiting with adaptive algorithms for controlling request throughput.

[![Crates.io](https://img.shields.io/crates/v/request-rate-limiter.svg)](https://crates.io/crates/request-rate-limiter)
[![Documentation](https://docs.rs/request-rate-limiter/badge.svg)](https://docs.rs/request-rate-limiter)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](https://github.com/shajapas/request-rate-limiter)

## Overview

A Rust library for request rate limiting using adaptive algorithms. The library provides rate limiting that automatically adjusts request rates based on success/failure patterns, helping prevent overload while maximizing throughput.

P.S. The idea was taken from [congestion-limiter](https://github.com/ThomWright/congestion-limiter) crate

## Features

- **Adaptive Rate Limiting**: Automatically adjusts request rates based on success/failure patterns
- **Multiple Algorithms**: More than 2 in the future. Or you can make your own :)
- **Async/Await Support**: Built for modern Rust async applications
- **Thread-Safe**: Safe for use in concurrent environments
- **Real-time Adaptation**: Responds to system feedback in real-time

## Algorithms

### AIMD (Additive Increase Multiplicative Decrease)
Loss-based rate limiting that increases the rate linearly on success and decreases it multiplicatively on overload.

- **Increases rate**: When requests succeed
- **Decreases rate**: When overload is detected (timeouts, 429/503 responses)
- **Best for**: Systems with clear overload signals

### Fixed Rate
Simple, constant rate limiting that maintains a fixed requests-per-second limit.

- **Constant rate**: Never changes regardless of outcomes
- **Best for**: Predictable workloads with known capacity limits

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
request-rate-limiter = "0.1.0"
```

## Quick Start

```rust
use std::sync::Arc;
use request_rate_limiter::{
    algorithms::Aimd,
    limiter::{DefaultRateLimiter, RateLimiter, RequestOutcome}
};

#[tokio::main]
async fn main() {
    // Create a rate limiter with AIMD algorithm
    let limiter = Arc::new(DefaultRateLimiter::new(
        Aimd::new_with_initial_rate(10)
            .decrease_factor(0.9)
            .increase_by(1)
    ));

    // Acquire permission to make a request
    let token = limiter.acquire().await;
    
    // Simulate doing work
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    
    // Release the token with outcome
    limiter.release(token, Some(RequestOutcome::Success)).await;
    
    println!("Request completed successfully!");
}
```


## License

Licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
