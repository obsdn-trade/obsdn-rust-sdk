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
