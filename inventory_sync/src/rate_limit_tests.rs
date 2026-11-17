use super::*;
use std::net::Ipv4Addr;

fn ip(last: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(192, 168, 1, last))
}

#[test]
fn a_burst_passes_then_the_rate_holds() {
    let limiter = RateLimiter::new(3, 60);
    let t = Instant::now();
    for _ in 0..3 {
        assert_eq!(limiter.check(ip(1), t), Ok(()));
    }
    let wait = limiter.check(ip(1), t).unwrap_err();
    assert!(
        wait > Duration::from_millis(900) && wait <= Duration::from_secs(1),
        "{wait:?}"
    );
    // One token per second comes back.
    assert_eq!(limiter.check(ip(1), t + Duration::from_secs(1)), Ok(()));
    assert!(limiter.check(ip(1), t + Duration::from_secs(1)).is_err());
}

#[test]
fn clients_have_their_own_buckets() {
    let limiter = RateLimiter::new(1, 60);
    let t = Instant::now();
    assert_eq!(limiter.check(ip(1), t), Ok(()));
    assert!(limiter.check(ip(1), t).is_err());
    assert_eq!(limiter.check(ip(2), t), Ok(()));
}

#[test]
fn buckets_never_hold_more_than_the_burst() {
    let limiter = RateLimiter::new(2, 60);
    let t = Instant::now();
    assert_eq!(limiter.check(ip(1), t), Ok(()));
    let later = t + Duration::from_secs(3600);
    assert_eq!(limiter.check(ip(1), later), Ok(()));
    assert_eq!(limiter.check(ip(1), later), Ok(()));
    assert!(limiter.check(ip(1), later).is_err());
}

#[test]
fn refused_requests_do_not_extend_the_wait() {
    let limiter = RateLimiter::new(1, 60);
    let t = Instant::now();
    limiter.check(ip(1), t).unwrap();
    for i in 0..10 {
        assert!(limiter
            .check(ip(1), t + Duration::from_millis(i * 10))
            .is_err());
    }
    assert_eq!(limiter.check(ip(1), t + Duration::from_secs(1)), Ok(()));
}

#[test]
fn idle_clients_are_forgotten_when_many_are_tracked() {
    let limiter = RateLimiter::new(1, 60);
    let t = Instant::now();
    for i in 0..MAX_TRACKED {
        let addr = IpAddr::V6(std::net::Ipv6Addr::from(i as u128 + 1));
        limiter.check(addr, t).unwrap();
    }
    assert_eq!(limiter.tracked(), MAX_TRACKED);
    // A minute later every bucket is full again: all are forgotten.
    limiter.check(ip(1), t + Duration::from_secs(60)).unwrap();
    assert_eq!(limiter.tracked(), 1);
}

#[test]
fn the_real_limits_allow_a_search_page_and_a_market_load() {
    let limiter = RateLimiter::requests();
    let t = Instant::now();
    // A search page: the search, up to 100 thumbnails, the card info.
    for _ in 0..102 {
        assert_eq!(limiter.check(ip(1), t), Ok(()));
    }
    let scryfall = RateLimiter::scryfall();
    for _ in 0..SCRYFALL_BURST {
        assert_eq!(scryfall.check(ip(1), t), Ok(()));
    }
    assert!(scryfall.check(ip(1), t).is_err());
}

#[test]
fn retry_after_is_whole_seconds() {
    assert_eq!(retry_after_secs(Duration::from_millis(1)), 1);
    assert_eq!(retry_after_secs(Duration::from_millis(1500)), 2);
    assert_eq!(retry_after_secs(Duration::ZERO), 1);
}
