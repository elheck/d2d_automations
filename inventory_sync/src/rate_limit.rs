//! Per-client limits for the open LAN API (token buckets).
//!
//! inventory_sync has no login (owner's decision: its own page calls the
//! API from the browser), so every client address gets a bucket: requests
//! take a token, tokens refill at a steady rate up to the bucket size. A
//! second, smaller limiter guards the lookups that reach Scryfall.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Requests per minute and client, and how many may come at once (a search
/// page loads up to 100 thumbnails).
pub const REQUESTS_PER_MINUTE: u32 = 600;
pub const REQUEST_BURST: u32 = 120;
/// Scryfall lookups (cache misses) per minute and client.
pub const SCRYFALL_PER_MINUTE: u32 = 60;
pub const SCRYFALL_BURST: u32 = 60;
/// Clients tracked before idle ones are forgotten.
pub const MAX_TRACKED: usize = 4096;

#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    updated: Instant,
}

/// A token bucket per client address.
#[derive(Debug)]
pub struct RateLimiter {
    capacity: f64,
    per_second: f64,
    buckets: Mutex<HashMap<IpAddr, Bucket>>,
}

impl RateLimiter {
    /// `burst` requests at once, refilled at `per_minute`.
    pub fn new(burst: u32, per_minute: u32) -> Self {
        Self {
            capacity: f64::from(burst.max(1)),
            per_second: f64::from(per_minute.max(1)) / 60.0,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// The limiter for all requests.
    pub fn requests() -> Self {
        Self::new(REQUEST_BURST, REQUESTS_PER_MINUTE)
    }

    /// The limiter for lookups that reach Scryfall.
    pub fn scryfall() -> Self {
        Self::new(SCRYFALL_BURST, SCRYFALL_PER_MINUTE)
    }

    fn refilled(&self, bucket: Bucket, now: Instant) -> f64 {
        let elapsed = now.saturating_duration_since(bucket.updated).as_secs_f64();
        (bucket.tokens + elapsed * self.per_second).min(self.capacity)
    }

    /// Takes a token for `ip` at `now`; otherwise how long until one is free.
    pub fn check(&self, ip: IpAddr, now: Instant) -> Result<(), Duration> {
        let mut buckets = self.buckets.lock().unwrap_or_else(PoisonError::into_inner);
        if buckets.len() >= MAX_TRACKED && !buckets.contains_key(&ip) {
            // Forget clients whose bucket is full again (idle long enough).
            buckets.retain(|_, b| self.refilled(*b, now) < self.capacity);
            if buckets.len() >= MAX_TRACKED {
                log::warn!("rate limiter: more than {MAX_TRACKED} active clients, starting over");
                buckets.clear();
            }
        }
        let bucket = buckets.entry(ip).or_insert(Bucket {
            tokens: self.capacity,
            updated: now,
        });
        let tokens = self.refilled(*bucket, now);
        if tokens >= 1.0 {
            *bucket = Bucket {
                tokens: tokens - 1.0,
                updated: now,
            };
            Ok(())
        } else {
            *bucket = Bucket {
                tokens,
                updated: now,
            };
            Err(Duration::from_secs_f64((1.0 - tokens) / self.per_second))
        }
    }

    /// Clients currently tracked.
    pub fn tracked(&self) -> usize {
        self.buckets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

/// Seconds for a `Retry-After` header (at least 1).
pub fn retry_after_secs(wait: Duration) -> u64 {
    wait.as_secs_f64().ceil().max(1.0) as u64
}

#[cfg(test)]
#[path = "rate_limit_tests.rs"]
mod tests;
