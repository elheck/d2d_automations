use super::*;
use crate::api::tests::{login_ok, request, send, TestApp};
use crate::cards::CardLookup;
use axum::http::Method;
use axum::Router;
use serde_json::{json, Value};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F'];

fn local_images(url: &str) -> bool {
    url.starts_with("http://127.0.0.1:")
}

/// A logged-in user with only `listing.use`, against a fake Scryfall and
/// (optionally) a fake inventory_sync.
async fn lister(scryfall: &MockServer, tracker_url: Option<String>) -> (TestApp, Router, String) {
    let scryfall_url = scryfall.uri();
    let app = TestApp::new().with(|state, dir| {
        state
            .with_cards(CardLookup::with_endpoints(
                &dir.join("cache"),
                &scryfall_url,
                local_images,
                std::time::Duration::ZERO,
            ))
            .with_inventory_sync(tracker_url.as_deref())
    });
    app.add_user("lister", &["listing.use"]).await;
    let router = app.router();
    let cookie = login_ok(&router, "lister", crate::api::tests::PASSWORD).await;
    (app, router, cookie)
}

fn bolt(scryfall: &MockServer) -> Value {
    json!({
        "id": "uuid",
        "name": "Lightning Bolt",
        "set": "hou",
        "set_name": "Hour of Devastation",
        "collector_number": "120",
        "rarity": "common",
        "mana_cost": "{R}",
        "type_line": "Instant",
        "cardmarket_id": 301234,
        "prices": { "eur": "0.25", "eur_foil": "1.10", "usd": null, "usd_foil": null },
        "image_uris": { "normal": format!("{}/img/hou120.jpg", scryfall.uri()) }
    })
}

async fn mount_bolt(scryfall: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/cards/hou/120"))
        .respond_with(ResponseTemplate::new(200).set_body_json(bolt(scryfall)))
        .mount(scryfall)
        .await;
}

fn latest_price_row() -> Value {
    json!({
        "id_product": 301234, "price_date": "2026-11-13",
        "avg": 0.3, "low": 0.02, "trend": 0.28, "avg1": 0.25, "avg7": 0.27, "avg30": 0.29,
        "avg_foil": 1.2, "low_foil": 0.0, "trend_foil": 1.05, "avg1_foil": null, "avg7_foil": 1.1, "avg30_foil": 1.15
    })
}

async fn get(
    router: &Router,
    uri: &str,
    cookie: &str,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    send(router, request(Method::GET, uri, None, Some(cookie))).await
}

#[tokio::test]
async fn a_lookup_returns_the_card_with_its_prices() {
    let scryfall = MockServer::start().await;
    mount_bolt(&scryfall).await;
    let tracker = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/latest-prices"))
        .and(body_json(json!({ "ids": [301234] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true, "data": [latest_price_row()]
        })))
        .expect(1)
        .mount(&tracker)
        .await;
    let (_app, router, cookie) = lister(&scryfall, Some(tracker.uri())).await;

    let (status, _, body) = get(&router, "/api/singles/lookup?input=HOU120", &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let card: SingleCard = serde_json::from_value(body).unwrap();
    assert_eq!(
        card,
        SingleCard {
            set_code: "hou".into(),
            collector_number: "120".into(),
            name: "Lightning Bolt".into(),
            set_name: "Hour of Devastation".into(),
            rarity: "common".into(),
            mana_cost: Some("{R}".into()),
            type_line: Some("Instant".into()),
            scryfall_eur: Some(0.25),
            scryfall_eur_foil: Some(1.1),
            cardmarket_id: Some(301234),
            cardmarket_url: Some(product_url(301234)),
            image_path: Some("/api/singles/image/hou/120".into()),
            prices: PriceLookup::Found {
                price_date: "2026-11-13".into(),
                regular: Box::new(PriceSet {
                    trend: Some(0.28),
                    low: Some(0.02),
                    avg: Some(0.3),
                    avg1: Some(0.25),
                    avg7: Some(0.27),
                    avg30: Some(0.29),
                }),
                // 0 means "no price", like a missing value.
                foil: Box::new(PriceSet {
                    trend: Some(1.05),
                    low: None,
                    avg: Some(1.2),
                    avg1: None,
                    avg7: Some(1.1),
                    avg30: Some(1.15),
                }),
            },
        }
    );
}

#[tokio::test]
async fn three_digits_use_the_default_set() {
    let scryfall = MockServer::start().await;
    mount_bolt(&scryfall).await;
    let (_app, router, cookie) = lister(&scryfall, None).await;
    let (status, _, body) = get(
        &router,
        "/api/singles/lookup?input=120&default_set=HOU",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "Lightning Bolt");
    assert_eq!(body["prices"], json!({ "status": "not_configured" }));
}

#[tokio::test]
async fn invalid_input_is_refused_before_asking_scryfall() {
    let scryfall = MockServer::start().await;
    let (_app, router, cookie) = lister(&scryfall, None).await;
    for query in [
        "input=",
        "input=hou",
        "input=120",
        "input=hou12a",
        "input=..%2F120",
        "input=h%2Fu120",
        "input=h%C3%B6u120",
        "input=h%00u120",
        "input=aaaaaaaaaaaaaaaaaaaaaaaaaaaa120",
        "input=120&default_set=..",
        "input=120&default_set=aaaaaaaaaaaaaaaa",
    ] {
        let (status, _, body) =
            get(&router, &format!("/api/singles/lookup?{query}"), &cookie).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
        assert_eq!(body["error"]["code"], "invalid_card_input", "{query}");
    }
    for query in ["", "default_set=hou", "input=hou120&extra=1"] {
        let (status, _, body) =
            get(&router, &format!("/api/singles/lookup?{query}"), &cookie).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
        assert_eq!(body["error"]["code"], "invalid_request", "{query}");
    }
    assert!(scryfall.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn unknown_cards_and_scryfall_outages_have_their_own_errors() {
    let scryfall = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/cards/hou/999"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "object": "error", "status": 404, "code": "not_found", "details": "No card found"
        })))
        .mount(&scryfall)
        .await;
    Mock::given(method("GET"))
        .and(path("/cards/hou/500"))
        .respond_with(ResponseTemplate::new(500).set_body_string("stack trace: secret internals"))
        .mount(&scryfall)
        .await;
    let (_app, router, cookie) = lister(&scryfall, None).await;

    let (status, _, body) = get(&router, "/api/singles/lookup?input=hou999", &cookie).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "card_not_found");

    let (status, _, body) = get(&router, "/api/singles/lookup?input=hou500", &cookie).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["error"]["code"], "scryfall_unavailable");
    assert!(!body.to_string().contains("secret"));
}

#[tokio::test]
async fn a_missing_price_never_hides_the_card() {
    let scryfall = MockServer::start().await;
    mount_bolt(&scryfall).await;
    Mock::given(method("GET"))
        .and(path("/cards/hou/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "u", "name": "Token", "set": "hou", "set_name": "Hour", "collector_number": "1", "rarity": "common"
        })))
        .mount(&scryfall)
        .await;

    // Not tracked: inventory_sync answers without a row for the product.
    let empty = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "success": true, "data": [] })),
        )
        .mount(&empty)
        .await;
    let (_app, router, cookie) = lister(&scryfall, Some(empty.uri())).await;
    let (_, _, body) = get(&router, "/api/singles/lookup?input=hou120", &cookie).await;
    assert_eq!(body["prices"], json!({ "status": "not_tracked" }));
    // No Cardmarket product: inventory_sync is not even asked.
    let (status, _, body) = get(&router, "/api/singles/lookup?input=hou001", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["prices"], json!({ "status": "no_cardmarket_id" }));
    assert_eq!(body["image_path"], Value::Null);
    assert_eq!(body["cardmarket_url"], Value::Null);
    assert_eq!(empty.received_requests().await.unwrap().len(), 1);

    // inventory_sync failing or unreachable.
    let failing = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&failing)
        .await;
    for tracker in [failing.uri(), "http://127.0.0.1:9".to_string()] {
        let (_app, router, cookie) = lister(&scryfall, Some(tracker)).await;
        let (status, _, body) = get(&router, "/api/singles/lookup?input=hou120", &cookie).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["prices"], json!({ "status": "unavailable" }));
        assert_eq!(body["name"], "Lightning Bolt");
    }
}

#[tokio::test]
async fn images_are_served_as_images_and_may_be_cached_privately() {
    let scryfall = MockServer::start().await;
    mount_bolt(&scryfall).await;
    Mock::given(method("GET"))
        .and(path("/img/hou120.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(JPEG))
        .expect(1)
        .mount(&scryfall)
        .await;
    let (_app, router, cookie) = lister(&scryfall, None).await;
    for uri in ["/api/singles/image/hou/120", "/api/singles/image/HOU/120"] {
        let response = tower::ServiceExt::oneshot(
            router.clone(),
            request(Method::GET, uri, None, Some(&cookie)),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers();
        assert_eq!(headers[header::CONTENT_TYPE], "image/jpeg");
        assert_eq!(
            headers[header::CACHE_CONTROL],
            format!("private, max-age={IMAGE_MAX_AGE_SECS}").as_str()
        );
        assert_eq!(headers["x-content-type-options"], "nosniff");
        let bytes = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        assert_eq!(bytes.as_ref(), JPEG);
    }
}

#[tokio::test]
async fn image_errors_are_json() {
    let scryfall = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/cards/hou/999"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "object": "error", "status": 404, "code": "not_found", "details": "No card found"
        })))
        .mount(&scryfall)
        .await;
    let (_app, router, cookie) = lister(&scryfall, None).await;
    let (status, _, body) = get(&router, "/api/singles/image/hou/999", &cookie).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "card_not_found");
    for bad in [
        "/api/singles/image/h.u/1",
        "/api/singles/image/hou/1%2F2",
        "/api/singles/image/h%C3%B6u/1",
    ] {
        let (status, headers, body) = get(&router, bad, &cookie).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
        assert_eq!(body["error"]["code"], "invalid_card_input", "{bad}");
        assert_eq!(headers[header::CACHE_CONTROL], "no-store", "{bad}");
    }
    let received = scryfall.received_requests().await.unwrap();
    assert_eq!(received.len(), 1, "only the unknown card was asked for");
}

#[tokio::test]
async fn the_singles_routes_only_answer_get() {
    let scryfall = MockServer::start().await;
    let (_app, router, cookie) = lister(&scryfall, None).await;
    for uri in [
        "/api/singles/lookup?input=hou120",
        "/api/singles/image/hou/120",
    ] {
        let (status, _, _) = send(
            &router,
            request(Method::POST, uri, Some(json!({})), Some(&cookie)),
        )
        .await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{uri}");
    }
}
