//! Unit coverage for the one-call `Orders::place_limit` helper (wiremock).

use std::sync::Arc;

use obsdn_sdk::rest::orders::LimitOrder;
use obsdn_sdk::types::v1::{
    GetMarketsResponse, Market, OrderSide, OrderType, PlaceOrderRequest, PlaceOrderResponse,
};
use obsdn_sdk::{Client, Env, Error, LocalSigner, Side};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// Deterministic test key (32 bytes of 0x01) - never a real account.
const TEST_KEY: &str = "0x0101010101010101010101010101010101010101010101010101010101010101";

fn signed_client(server: &MockServer) -> Client {
    Client::builder()
        .env(Env::Staging)
        .rest_base_url(server.uri())
        .api_key("KEY", "SECRET")
        .eip712_signer(Arc::new(LocalSigner::from_hex(TEST_KEY).unwrap()))
        .build()
        .expect("build signed client")
}

async fn mount_markets(server: &MockServer) {
    let envelope = serde_json::json!({
        "data": GetMarketsResponse {
            mkts: vec![Market {
                mkt_id: "BTC-PERP".into(),
                idx: "1".into(),
                enabled: true,
                ..Default::default()
            }],
        },
    });
    Mock::given(method("GET"))
        .and(path("/markets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&envelope))
        .mount(server)
        .await;
}

#[tokio::test]
async fn place_limit_resolves_signs_and_posts() {
    let server = MockServer::start().await;
    mount_markets(&server).await;
    Mock::given(method("POST"))
        .and(path("/orders"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": PlaceOrderResponse { ord: None },
        })))
        .mount(&server)
        .await;

    let client = signed_client(&server);
    client
        .orders()
        .place_limit(
            LimitOrder::new("BTC-PERP", Side::Buy, 100.0, 1.0)
                .post_only(true)
                .nonce(12345),
        )
        .await
        .expect("place_limit");

    let reqs = server.received_requests().await.unwrap();
    let order_req = reqs
        .iter()
        .find(|r| r.url.path() == "/orders")
        .expect("POST /orders made");
    assert!(
        order_req.headers.get("x-api-key").is_some(),
        "authenticated request carries HMAC headers"
    );

    // The signed body reflects the builder inputs.
    let body: PlaceOrderRequest =
        serde_json::from_slice(&order_req.body).expect("order body decodes");
    assert_eq!(body.mkt_id, "BTC-PERP");
    assert_eq!(body.sd, OrderSide::Buy as i32);
    assert_eq!(body.ot, OrderType::Limit as i32);
    assert_eq!(body.sz, 1.0);
    assert_eq!(body.px, 100.0);
    assert!(body.po);
    assert_eq!(body.nonce, 12345);
    assert!(
        body.sig.starts_with("0x") && body.sig.len() == 132,
        "expected 65-byte hex signature, got {}",
        body.sig
    );
}

#[tokio::test]
async fn place_limit_without_signer_errors() {
    // The missing signer is caught before any network call.
    let client = Client::builder()
        .env(Env::Staging)
        .api_key("KEY", "SECRET")
        .build()
        .unwrap();
    let err = client
        .orders()
        .place_limit(LimitOrder::new("BTC-PERP", Side::Buy, 100.0, 1.0))
        .await
        .expect_err("must require a signer");
    assert!(matches!(err, Error::Sign(_)), "got {err:?}");
}

#[tokio::test]
async fn place_limit_rejects_non_positive_size() {
    let client = Client::builder()
        .env(Env::Staging)
        .api_key("KEY", "SECRET")
        .eip712_signer(Arc::new(LocalSigner::from_hex(TEST_KEY).unwrap()))
        .build()
        .unwrap();
    let err = client
        .orders()
        .place_limit(LimitOrder::new("BTC-PERP", Side::Buy, 100.0, 0.0))
        .await
        .expect_err("zero size must error");
    assert!(matches!(err, Error::Sign(_)), "got {err:?}");
}
