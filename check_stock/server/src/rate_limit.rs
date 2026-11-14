//! Login attempts per client address.
//!
//! On top of the per-account lockout: an address may try at most
//! [`MAX_LOGIN_ATTEMPTS`] logins per [`LOGIN_WINDOW`], whichever accounts it
//! tries. That bounds password guessing across accounts and keeps the
//! deliberately slow password hashing from being used to tie up the server.
//! Kept in memory: a restart clears it, the account lockouts stay.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const MAX_LOGIN_ATTEMPTS: u32 = 10;
pub const LOGIN_WINDOW: Duration = Duration::from_secs(5 * 60);

/// Above this many tracked addresses, expired windows are swept.
const SWEEP_ABOVE: usize = 10_000;

/// Fixed-window counter per address.
#[derive(Debug)]
pub struct LoginRateLimiter {
    max: u32,
    window: Duration,
    windows: Mutex<HashMap<IpAddr, (Instant, u32)>>,
}

impl Default for LoginRateLimiter {
    fn default() -> Self {
        Self::new(MAX_LOGIN_ATTEMPTS, LOGIN_WINDOW)
    }
}

impl LoginRateLimiter {
    pub fn new(max: u32, window: Duration) -> Self {
        Self {
            max,
            window,
            windows: Mutex::new(HashMap::new()),
        }
    }

    /// Counts an attempt from `ip`. `Err(wait)` when the address has used up
    /// its attempts; `wait` is how long until the window resets.
    pub fn attempt(&self, ip: IpAddr, now: Instant) -> Result<(), Duration> {
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if windows.len() > SWEEP_ABOVE {
            let window = self.window;
            windows.retain(|_, (start, _)| now.duration_since(*start) < window);
        }
        let entry = windows.entry(ip).or_insert((now, 0));
        if now.duration_since(entry.0) >= self.window {
            *entry = (now, 0);
        }
        if entry.1 >= self.max {
            return Err(self.window.saturating_sub(now.duration_since(entry.0)));
        }
        entry.1 += 1;
        Ok(())
    }

    /// Addresses currently tracked.
    pub fn tracked(&self) -> usize {
        self.windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

#[cfg(test)]
#[path = "rate_limit_tests.rs"]
mod tests;
