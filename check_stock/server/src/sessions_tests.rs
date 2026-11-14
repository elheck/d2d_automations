use super::*;
use crate::db::open_writer;
use crate::permissions::PermissionSet;
use axum::http::HeaderValue;
use check_stock_core::inventory_db::InventoryDb;
use chrono::TimeZone;

fn db_with_user() -> (tempfile::TempDir, Connection, i64) {
    let dir = tempfile::tempdir().unwrap();
    let conn = open_writer(&InventoryDb::in_dir(dir.path())).unwrap();
    let id =
        crate::users::create(&conn, "anna", "Anna", "$h", &PermissionSet::new(), t0()).unwrap();
    (dir, conn, id)
}

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 14, 8, 0, 0).unwrap()
}

#[test]
fn tokens_are_random_hex_and_never_printed() {
    let a = SessionToken::generate().unwrap();
    let b = SessionToken::generate().unwrap();
    assert_ne!(a, b);
    assert_eq!(a.as_str().len(), 64);
    assert!(a.as_str().bytes().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(format!("{a:?}"), "SessionToken(…)");
}

#[test]
fn only_well_formed_tokens_are_accepted() {
    let token = SessionToken::generate().unwrap();
    assert_eq!(SessionToken::parse(token.as_str()), Some(token.clone()));
    assert!(SessionToken::parse("").is_none());
    assert!(SessionToken::parse(&token.as_str()[1..]).is_none());
    assert!(SessionToken::parse(&token.as_str().to_uppercase()).is_none());
    assert!(SessionToken::parse(&format!("{}g", &token.as_str()[1..])).is_none());
}

#[test]
fn the_database_only_sees_the_hash() {
    let (_dir, conn, id) = db_with_user();
    let token = SessionToken::generate().unwrap();
    create(&conn, &token, id, Some("Firefox"), t0()).unwrap();
    let stored: Vec<u8> = conn
        .query_row("SELECT token_hash FROM srv_sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stored, token.hash());
    assert_eq!(stored.len(), 32);
    assert_ne!(stored, token.as_str().as_bytes());
}

#[test]
fn cookies_are_parsed_from_the_request() {
    let token = SessionToken::generate().unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        HeaderValue::from_str(&format!(
            "theme=dark; {COOKIE_NAME}={} ; other=1",
            token.as_str()
        ))
        .unwrap(),
    );
    assert_eq!(token_from_headers(&headers), Some(token));

    let mut wrong_name = HeaderMap::new();
    wrong_name.insert(header::COOKIE, HeaderValue::from_static("session=abc"));
    assert_eq!(token_from_headers(&wrong_name), None);
    assert_eq!(token_from_headers(&HeaderMap::new()), None);
}

#[test]
fn the_cookie_has_every_protection() {
    let token = SessionToken::generate().unwrap();
    let cookie = set_cookie(&token);
    assert!(cookie.starts_with(&format!("{COOKIE_NAME}={};", token.as_str())));
    for attribute in [
        "Path=/",
        "HttpOnly",
        "Secure",
        "SameSite=Strict",
        "Max-Age=604800",
    ] {
        assert!(cookie.contains(attribute), "{cookie} lacks {attribute}");
    }
    assert!(
        !cookie.contains("Domain"),
        "__Host- cookies must not name a domain"
    );
    let cleared = clear_cookie();
    assert!(cleared.contains("Max-Age=0") && cleared.starts_with(&format!("{COOKIE_NAME}=;")));
}

#[test]
fn sessions_idle_out_and_expire() {
    let session = Session {
        user_id: 1,
        last_seen_at: t0(),
        expires_at: t0() + ABSOLUTE_LIFETIME,
    };
    assert_eq!(
        session.status(t0()),
        SessionStatus::Valid { needs_touch: false }
    );
    assert_eq!(
        session.status(t0() + TOUCH_INTERVAL),
        SessionStatus::Valid { needs_touch: true }
    );
    assert_eq!(session.status(t0() + IDLE_TIMEOUT), SessionStatus::Expired);

    // Kept active, it still ends at the absolute lifetime.
    let active = Session {
        last_seen_at: t0() + ABSOLUTE_LIFETIME - Duration::minutes(1),
        ..session
    };
    assert_eq!(
        active.status(t0() + ABSOLUTE_LIFETIME),
        SessionStatus::Expired
    );
}

#[test]
fn find_touch_and_revoke() {
    let (_dir, conn, id) = db_with_user();
    let token = SessionToken::generate().unwrap();
    create(&conn, &token, id, None, t0()).unwrap();
    let found = find(&conn, &token.hash()).unwrap().unwrap();
    assert_eq!(found.user_id, id);
    assert_eq!(found.expires_at, t0() + ABSOLUTE_LIFETIME);

    touch(&conn, &token.hash(), t0() + Duration::hours(1)).unwrap();
    assert_eq!(
        find(&conn, &token.hash()).unwrap().unwrap().last_seen_at,
        t0() + Duration::hours(1)
    );

    revoke(&conn, &token.hash()).unwrap();
    assert_eq!(find(&conn, &token.hash()).unwrap(), None);
}

#[test]
fn all_sessions_of_a_user_can_be_ended() {
    let (_dir, conn, id) = db_with_user();
    for _ in 0..3 {
        create(&conn, &SessionToken::generate().unwrap(), id, None, t0()).unwrap();
    }
    assert_eq!(revoke_all_for_user(&conn, id).unwrap(), 3);
}

#[test]
fn expired_sessions_are_pruned() {
    let (_dir, conn, id) = db_with_user();
    let old = SessionToken::generate().unwrap();
    let fresh = SessionToken::generate().unwrap();
    create(&conn, &old, id, None, t0()).unwrap();
    create(&conn, &fresh, id, None, t0() + Duration::hours(7)).unwrap();
    assert_eq!(prune_expired(&conn, t0() + IDLE_TIMEOUT).unwrap(), 1);
    assert!(find(&conn, &old.hash()).unwrap().is_none());
    assert!(find(&conn, &fresh.hash()).unwrap().is_some());
}

#[test]
fn user_agents_are_trimmed() {
    let (_dir, conn, id) = db_with_user();
    let token = SessionToken::generate().unwrap();
    create(
        &conn,
        &token,
        id,
        Some(&format!("{}\n{}", "a".repeat(300), "b")),
        t0(),
    )
    .unwrap();
    let ua: String = conn
        .query_row("SELECT user_agent FROM srv_sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ua.chars().count(), 200);
    assert!(!ua.contains('\n'));
}
