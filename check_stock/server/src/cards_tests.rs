use super::*;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The first bytes of a JPEG file, enough for [`ImageKind::sniff`].
pub(crate) const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F'];

fn local_images(url: &str) -> bool {
    url.starts_with("http://127.0.0.1:")
}

fn card_json(set: &str, number: &str, image: Option<String>) -> serde_json::Value {
    serde_json::json!({
        "id": "uuid",
        "name": "Test Card",
        "set": set,
        "set_name": "Test Set",
        "collector_number": number,
        "rarity": "rare",
        "image_uris": image.map(|url| serde_json::json!({ "normal": url })),
    })
}

fn lookup(dir: &Path, server: &MockServer) -> CardLookup {
    CardLookup::with_endpoints(dir, &server.uri(), local_images, Duration::ZERO)
}

async fn mount_card(server: &MockServer, set: &str, number: &str, calls: u64) {
    let image = format!("{}/img/{set}{number}.jpg", server.uri());
    Mock::given(method("GET"))
        .and(path(format!("/cards/{set}/{number}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(card_json(set, number, Some(image))))
        .expect(calls)
        .mount(server)
        .await;
}

#[test]
fn only_jpeg_and_png_are_images() {
    assert_eq!(ImageKind::sniff(JPEG), Some(ImageKind::Jpeg));
    assert_eq!(
        ImageKind::sniff(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"),
        Some(ImageKind::Png)
    );
    for not_an_image in [&b""[..], b"<html>", b"<svg xmlns=", b"GIF89a", b"\xFF\xD8"] {
        assert_eq!(ImageKind::sniff(not_an_image), None);
    }
    assert_eq!(ImageKind::Jpeg.content_type(), "image/jpeg");
    assert_eq!(ImageKind::Png.content_type(), "image/png");
}

#[tokio::test]
async fn cards_are_fetched_once_and_kept_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    mount_card(&server, "hou", "120", 1).await;

    let cards = lookup(dir.path(), &server);
    assert_eq!(cards.card("hou", "120").await.unwrap().name, "Test Card");
    assert_eq!(cards.card("HOU", "120").await.unwrap().name, "Test Card");
    assert!(dir.path().join(CardCache::FILE_NAME).exists());

    // A restarted server finds it in the cache file.
    let offline = MockServer::start().await;
    let restarted = lookup(dir.path(), &offline);
    assert_eq!(
        restarted.card("hou", "120").await.unwrap().name,
        "Test Card"
    );
    assert!(offline.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn unknown_cards_are_not_cached() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/cards/hou/999"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "object": "error", "status": 404, "code": "not_found", "details": "No card found"
        })))
        .expect(2)
        .mount(&server)
        .await;
    let cards = lookup(dir.path(), &server);
    assert_eq!(
        cards.card("hou", "999").await.unwrap_err(),
        LookupError::NotFound
    );
    assert_eq!(
        cards.card("hou", "999").await.unwrap_err(),
        LookupError::NotFound
    );
    assert!(!dir.path().join(CardCache::FILE_NAME).exists());
}

#[tokio::test]
async fn a_bare_404_also_means_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404).set_body_string("nope"))
        .mount(&server)
        .await;
    let cards = lookup(dir.path(), &server);
    assert_eq!(
        cards.card("hou", "1").await.unwrap_err(),
        LookupError::NotFound
    );
}

#[tokio::test]
async fn scryfall_errors_mean_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let cards = lookup(dir.path(), &server);
    assert_eq!(
        cards.card("hou", "1").await.unwrap_err(),
        LookupError::Unavailable
    );
    let unreachable = CardLookup::with_endpoints(
        dir.path(),
        "http://127.0.0.1:9",
        local_images,
        Duration::ZERO,
    );
    assert_eq!(
        unreachable.card("hou", "1").await.unwrap_err(),
        LookupError::Unavailable
    );
}

#[tokio::test]
async fn unsafe_keys_are_refused_without_a_request() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    let cards = lookup(dir.path(), &server);
    for (set, number) in [
        ("..", "1"),
        ("hou", "1/2"),
        ("ho u", "1"),
        ("", "1"),
        ("hou", ""),
        ("höu", "1"),
    ] {
        assert_eq!(
            cards.card(set, number).await.unwrap_err(),
            LookupError::InvalidKey
        );
        assert_eq!(
            cards.image(set, number).await.unwrap_err(),
            LookupError::InvalidKey
        );
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn images_are_fetched_once_and_kept_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    mount_card(&server, "hou", "120", 1).await;
    Mock::given(method("GET"))
        .and(path("/img/hou120.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(JPEG))
        .expect(1)
        .mount(&server)
        .await;
    let cards = lookup(dir.path(), &server);
    let (bytes, kind) = cards.image("hou", "120").await.unwrap();
    assert_eq!((bytes.as_slice(), kind), (JPEG, ImageKind::Jpeg));
    let (again, _) = cards.image("HOU", "120").await.unwrap();
    assert_eq!(again, JPEG);
    assert!(dir
        .path()
        .join(ImageCache::DIR_NAME)
        .join("hou_120.jpg")
        .exists());
}

#[tokio::test]
async fn image_urls_outside_scryfall_are_never_fetched() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    mount_card(&server, "hou", "120", 1).await;
    Mock::given(method("GET"))
        .and(path("/img/hou120.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(JPEG))
        .expect(0)
        .mount(&server)
        .await;
    // The production rule: only https://cards.scryfall.io.
    let cards = CardLookup::with_endpoints(
        dir.path(),
        &server.uri(),
        is_scryfall_image_url,
        Duration::ZERO,
    );
    assert_eq!(
        cards.image("hou", "120").await.unwrap_err(),
        LookupError::NoImage
    );
}

#[tokio::test]
async fn cards_without_an_image_have_none() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/cards/hou/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(card_json("hou", "7", None)))
        .mount(&server)
        .await;
    let cards = lookup(dir.path(), &server);
    assert_eq!(
        cards.image("hou", "7").await.unwrap_err(),
        LookupError::NoImage
    );
}

#[tokio::test]
async fn other_data_than_jpeg_or_png_is_neither_served_nor_kept() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    mount_card(&server, "hou", "120", 1).await;
    Mock::given(method("GET"))
        .and(path("/img/hou120.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>",
        ))
        .mount(&server)
        .await;
    let cards = lookup(dir.path(), &server);
    assert_eq!(
        cards.image("hou", "120").await.unwrap_err(),
        LookupError::NoImage
    );
    assert!(!dir
        .path()
        .join(ImageCache::DIR_NAME)
        .join("hou_120.jpg")
        .exists());
}

#[tokio::test]
async fn a_broken_cached_image_is_fetched_again() {
    let dir = tempfile::tempdir().unwrap();
    let images = ImageCache::in_root(dir.path());
    images.insert("hou", "120", b"<html>broken</html>");
    let server = MockServer::start().await;
    mount_card(&server, "hou", "120", 1).await;
    Mock::given(method("GET"))
        .and(path("/img/hou120.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(JPEG))
        .expect(1)
        .mount(&server)
        .await;
    let cards = lookup(dir.path(), &server);
    assert_eq!(cards.image("hou", "120").await.unwrap().0, JPEG);
    assert_eq!(images.get("hou", "120").unwrap(), JPEG);
}

#[tokio::test]
async fn oversized_images_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    mount_card(&server, "hou", "120", 1).await;
    let mut huge = JPEG.to_vec();
    huge.resize(MAX_IMAGE_BYTES + 1, 0);
    Mock::given(method("GET"))
        .and(path("/img/hou120.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(huge))
        .mount(&server)
        .await;
    let cards = lookup(dir.path(), &server);
    assert_eq!(
        cards.image("hou", "120").await.unwrap_err(),
        LookupError::Unavailable
    );
}

/// Serves `body` without a Content-Length (chunked), as any server may, so
/// only the limit while reading can stop an oversized image.
async fn chunked_image_server(body: Vec<u8>) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let body = body.clone();
            tokio::spawn(async move {
                let mut request = [0u8; 4096];
                let _ = socket.read(&mut request).await;
                let _ = socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\n\
                          Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                for chunk in body.chunks(64 * 1024) {
                    let _ = socket
                        .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                        .await;
                    let _ = socket.write_all(chunk).await;
                    let _ = socket.write_all(b"\r\n").await;
                }
                let _ = socket.write_all(b"0\r\n\r\n").await;
            });
        }
    });
    format!("http://{addr}/image.jpg")
}

async fn mount_card_with_image(server: &MockServer, number: &str, image: String) {
    Mock::given(method("GET"))
        .and(path(format!("/cards/hou/{number}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(card_json(
            "hou",
            number,
            Some(image),
        )))
        .mount(server)
        .await;
}

#[tokio::test]
async fn images_without_a_length_are_limited_while_reading() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    mount_card_with_image(&server, "1", chunked_image_server(JPEG.to_vec()).await).await;
    let mut huge = JPEG.to_vec();
    huge.resize(MAX_IMAGE_BYTES + 1, 0);
    mount_card_with_image(&server, "2", chunked_image_server(huge).await).await;
    let cards = lookup(dir.path(), &server);
    assert_eq!(cards.image("hou", "1").await.unwrap().0, JPEG);
    assert_eq!(
        cards.image("hou", "2").await.unwrap_err(),
        LookupError::Unavailable
    );
    assert!(!dir
        .path()
        .join(ImageCache::DIR_NAME)
        .join("hou_2.jpg")
        .exists());
}

#[tokio::test]
async fn image_redirects_are_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    mount_card(&server, "hou", "120", 1).await;
    Mock::given(method("GET"))
        .and(path("/img/hou120.jpg"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/internal", server.uri())),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/internal"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(JPEG))
        .expect(0)
        .mount(&server)
        .await;
    let cards = lookup(dir.path(), &server);
    assert_eq!(
        cards.image("hou", "120").await.unwrap_err(),
        LookupError::Unavailable
    );
}

#[tokio::test]
async fn scryfall_requests_are_spaced() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    for number in ["1", "2", "3"] {
        mount_card(&server, "hou", number, 1).await;
    }
    let spacing = Duration::from_millis(150);
    let cards = CardLookup::with_endpoints(dir.path(), &server.uri(), local_images, spacing);
    let start = Instant::now();
    let (a, b, c) = tokio::join!(
        cards.card("hou", "1"),
        cards.card("hou", "2"),
        cards.card("hou", "3")
    );
    assert!(a.is_ok() && b.is_ok() && c.is_ok());
    // Three requests: the second waits one spacing, the third two.
    assert!(start.elapsed() >= spacing * 2, "{:?}", start.elapsed());
}

#[tokio::test]
async fn a_waiting_lookup_uses_what_the_one_before_fetched() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    mount_card(&server, "hou", "120", 1).await;
    // Long enough for the first (local) request to finish while the second waits.
    let cards = CardLookup::with_endpoints(
        dir.path(),
        &server.uri(),
        local_images,
        Duration::from_secs(1),
    );
    let (a, b) = tokio::join!(cards.card("hou", "120"), cards.card("hou", "120"));
    assert!(a.is_ok() && b.is_ok());
}
