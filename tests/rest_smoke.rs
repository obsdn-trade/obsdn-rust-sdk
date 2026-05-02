//! Phase 2 REST smoke tests against an in-process wiremock server.
//!
//! Goals:
//!   1. Confirm the gateway envelope (`{"data":...}`) is unwrapped.
//!   2. Confirm HMAC headers land on auth-required calls.
//!   3. Confirm public endpoints don't carry HMAC headers even when a
//!      signer is configured.
//!   4. Confirm error responses decode into `Error::Api`.

use obsdn_sdk::types::v1::{
    GetMarketsResponse, Market, Order, OrderSide, OrderStatus, OrderType, PlaceOrderRequest,
    PlaceOrderResponse, SelfTradePrevention, TimeInForce,
};
use obsdn_sdk::{Client, Env, Error};
use wiremock::matchers::{body_json, header, header_exists, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn mock_market() -> Market {
    Market {
        mkt_id: "BTC-PERP".into(),
        disp_name: "BTC Perp".into(),
        enabled: true,
        ..Default::default()
    }
}

fn build_client(server: &MockServer) -> Client {
    Client::builder()
        .env(Env::Local)
        .rest_base_url(server.uri())
        .api_key("KEY", "SECRET")
        .build()
        .expect("build client")
}

#[tokio::test]
async fn get_markets_unwraps_envelope_and_skips_auth_headers() {
    let server = MockServer::start().await;

    let envelope = serde_json::json!({
        "data": GetMarketsResponse {
            mkts: vec![mock_market()],
        },
        "request_id": "req-1",
    });

    Mock::given(method("GET"))
        .and(path("/markets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&envelope))
        .mount(&server)
        .await;

    let client = build_client(&server);
    let resp = client.markets().get_markets().await.expect("get_markets");
    assert_eq!(resp.mkts.len(), 1);
    assert_eq!(resp.mkts[0].mkt_id, "BTC-PERP");

    // Public endpoint must NOT receive HMAC headers, even when client has
    // a signer configured. Inspect the captured request to verify.
    let received = &server.received_requests().await.expect("requests")[0];
    assert!(
        received.headers.get("x-api-key").is_none(),
        "public GET /markets must not carry x-api-key, got: {:?}",
        received.headers
    );
    assert!(received.headers.get("x-api-signature").is_none());
    assert!(received.headers.get("x-api-timestamp").is_none());
}

#[tokio::test]
async fn place_order_injects_hmac_headers() {
    let server = MockServer::start().await;

    let req = PlaceOrderRequest {
        mkt_id: "BTC-PERP".into(),
        sd: OrderSide::Buy as i32,
        ot: OrderType::Limit as i32,
        sz: 1.0,
        px: 100.0,
        tif: TimeInForce::Gtc as i32,
        po: false,
        ro: false,
        stp: SelfTradePrevention::CancelTaker as i32,
        cl_oid: "client-1".into(),
        nonce: 1_700_000_000_000_000_000,
        sig: "0xdeadbeef".into(),
        ..Default::default()
    };

    let resp_envelope = serde_json::json!({
        "data": PlaceOrderResponse {
            ord: Some(Order {
                oid: "ord-1".into(),
                mkt_id: req.mkt_id.clone(),
                sd: req.sd,
                ot: req.ot,
                sz: "1".into(),
                px: "100".into(),
                cl_oid: req.cl_oid.clone(),
                st: OrderStatus::Open as i32,
                ..Default::default()
            }),
        },
    });

    Mock::given(method("POST"))
        .and(path("/orders"))
        .and(header_exists("x-api-key"))
        .and(header_exists("x-api-signature"))
        .and(header_exists("x-api-timestamp"))
        .and(header("x-api-key", "KEY"))
        .and(header("content-type", "application/json"))
        .and(body_json(&req))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_envelope))
        .mount(&server)
        .await;

    let client = build_client(&server);
    let resp = client.orders().place(req).await.expect("place");
    assert_eq!(resp.ord.expect("ord present").oid, "ord-1");

    // Signature must base64-decode to 32 bytes (HMAC-SHA256). Exact value
    // depends on `now()` so we can't golden-match end-to-end here; the
    // value test lives in `auth::tests`.
    let sig = signature_header(&server.received_requests().await.unwrap()[0]);
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(sig.as_bytes())
        .expect("signature decodes as base64");
    assert_eq!(raw.len(), 32, "HMAC-SHA256 must be 32 bytes");
}

#[tokio::test]
async fn place_order_without_signer_returns_auth_error() {
    let server = MockServer::start().await;
    let client = Client::builder()
        .env(Env::Local)
        .rest_base_url(server.uri())
        .build()
        .expect("build no-auth client");

    let err = client
        .orders()
        .place(PlaceOrderRequest {
            mkt_id: "BTC-PERP".into(),
            ..Default::default()
        })
        .await
        .expect_err("must fail without signer");
    match err {
        Error::Auth(_) => {}
        other => panic!("expected Error::Auth, got {other:?}"),
    }
}

#[tokio::test]
async fn server_error_envelope_decodes_to_api_error() {
    let server = MockServer::start().await;

    let body = serde_json::json!({
        "error": {
            "code": "InvalidArgument",
            "message": "market not found",
        },
        "request_id": "req-bad",
    });
    Mock::given(method("GET"))
        .and(path("/markets"))
        .respond_with(ResponseTemplate::new(400).set_body_json(&body))
        .mount(&server)
        .await;

    let client = build_client(&server);
    let err = client
        .markets()
        .get_markets()
        .await
        .expect_err("must surface server error");
    match err {
        Error::Api {
            status,
            code,
            message,
            request_id,
        } => {
            assert_eq!(status, 400);
            assert_eq!(code, "InvalidArgument");
            assert_eq!(message, "market not found");
            assert_eq!(request_id.as_deref(), Some("req-bad"));
        }
        other => panic!("expected Error::Api, got {other:?}"),
    }
}

fn signature_header(req: &Request) -> String {
    req.headers
        .get("x-api-signature")
        .map(|v| v.to_str().expect("ascii sig").to_string())
        .expect("missing x-api-signature header")
}
