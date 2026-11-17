use super::*;

fn ip(last: u8) -> IpAddr {
    IpAddr::from([192, 168, 178, last])
}

#[test]
fn allows_up_to_the_limit_then_refuses() {
    let limiter = LoginRateLimiter::new(3, Duration::from_secs(60));
    let now = Instant::now();
    for _ in 0..3 {
        assert!(limiter.attempt(ip(1), now).is_ok());
    }
    let wait = limiter
        .attempt(ip(1), now + Duration::from_secs(10))
        .unwrap_err();
    assert_eq!(wait, Duration::from_secs(50));
}

#[test]
fn the_window_resets() {
    let limiter = LoginRateLimiter::new(1, Duration::from_secs(60));
    let now = Instant::now();
    assert!(limiter.attempt(ip(1), now).is_ok());
    assert!(limiter
        .attempt(ip(1), now + Duration::from_secs(59))
        .is_err());
    assert!(limiter
        .attempt(ip(1), now + Duration::from_secs(60))
        .is_ok());
}

#[test]
fn addresses_are_counted_separately() {
    let limiter = LoginRateLimiter::new(1, Duration::from_secs(60));
    let now = Instant::now();
    assert!(limiter.attempt(ip(1), now).is_ok());
    assert!(limiter.attempt(ip(1), now).is_err());
    assert!(limiter.attempt(ip(2), now).is_ok());
    assert_eq!(limiter.tracked(), 2);
}

#[test]
fn old_entries_are_swept_when_many_addresses_were_seen() {
    let limiter = LoginRateLimiter::new(5, Duration::from_secs(60));
    let start = Instant::now();
    for i in 0..=SWEEP_ABOVE as u32 {
        limiter
            .attempt(IpAddr::from(i.to_be_bytes()), start)
            .unwrap();
    }
    assert!(limiter.tracked() > SWEEP_ABOVE);
    limiter
        .attempt(ip(1), start + Duration::from_secs(61))
        .unwrap();
    assert_eq!(limiter.tracked(), 1);
}

#[test]
fn defaults() {
    assert_eq!(MAX_LOGIN_ATTEMPTS, 10);
    assert_eq!(LOGIN_WINDOW, Duration::from_secs(300));
}
