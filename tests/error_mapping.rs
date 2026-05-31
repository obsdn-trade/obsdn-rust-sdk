//! Coverage for the error-mapping paths not exercised elsewhere.

use std::time::Duration;

use obsdn_sdk::{Client, Env, Error};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> Client {
    Client::builder()
        .env(Env::Staging)
        .rest_base_url(server.uri())
        .api_key("KEY", "SECRET")
        .build()
        .unwrap()
}

#[tokio::test]
async fn non_2xx_unparseable_body_maps_to_unparsed_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/markets"))
        .respond_with(ResponseTemplate::new(500).set_body_string("internal boom"))
        .mount(&server)
        .await;

    let err = client(&server)
        .markets()
        .list()
        .await
        .expect_err("500 should error");
    match err {
        Error::UnparsedBody { status, body } => {
            assert_eq!(status, 500);
            assert!(body.contains("internal boom"), "raw body surfaced: {body}");
        }
        other => panic!("expected UnparsedBody, got {other:?}"),
    }
}

#[tokio::test]
async fn oversized_error_body_is_truncated() {
    let server = MockServer::start().await;
    // A pathological multi-MB non-JSON error page (e.g. a WAF block page).
    let huge = "x".repeat(100_000);
    Mock::given(method("GET"))
        .and(path("/markets"))
        .respond_with(ResponseTemplate::new(502).set_body_string(huge))
        .mount(&server)
        .await;

    let err = client(&server)
        .markets()
        .list()
        .await
        .expect_err("502 should error");
    match err {
        Error::UnparsedBody { status, body } => {
            assert_eq!(status, 502);
            // Capped at 4096 chars + the truncation marker, not the full 100k.
            assert!(
                body.len() < 5_000,
                "body should be truncated, got {} bytes",
                body.len()
            );
            assert!(body.ends_with("… (truncated)"), "truncation marker present");
        }
        other => panic!("expected UnparsedBody, got {other:?}"),
    }
}

#[tokio::test]
async fn oversized_multibyte_error_body_truncates_without_panic() {
    let server = MockServer::start().await;
    // 3-byte chars, so the 4096-byte cap lands mid-character. The byte-slice
    // truncation must not panic on the split boundary - `from_utf8_lossy`
    // repairs the partial trailing char.
    let huge = "€".repeat(5_000); // 15_000 bytes
    Mock::given(method("GET"))
        .and(path("/markets"))
        .respond_with(ResponseTemplate::new(502).set_body_string(huge))
        .mount(&server)
        .await;

    let err = client(&server)
        .markets()
        .list()
        .await
        .expect_err("502 should error");
    match err {
        Error::UnparsedBody { status, body } => {
            assert_eq!(status, 502);
            assert!(body.len() < 5_000, "truncated, got {} bytes", body.len());
            assert!(body.ends_with("… (truncated)"), "truncation marker present");
        }
        other => panic!("expected UnparsedBody, got {other:?}"),
    }
}

#[tokio::test]
async fn connection_refused_maps_to_transport() {
    // Port 1 has no listener, so the connection is refused immediately.
    let client = Client::builder()
        .env(Env::Staging)
        .rest_base_url("http://127.0.0.1:1")
        .timeout(Duration::from_millis(500))
        .build()
        .unwrap();
    let err = client
        .markets()
        .list()
        .await
        .expect_err("connection must fail");
    assert!(matches!(err, Error::Transport(_)), "got {err:?}");
}

#[tokio::test]
async fn success_with_garbage_body_maps_to_decode() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/markets"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .mount(&server)
        .await;

    let err = client(&server)
        .markets()
        .list()
        .await
        .expect_err("garbage 200 should error");
    assert!(matches!(err, Error::Decode(_)), "got {err:?}");
}
