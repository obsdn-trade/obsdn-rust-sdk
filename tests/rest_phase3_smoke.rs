//! Extended REST smoke tests — additional request patterns not covered by
//! the basic REST suite.
//!
//! Covers:
//!   1. Path parameter substitution + URL encoding (cancel by oid).
//!   2. Query-string serialization for GET (list open orders).
//!   3. DELETE without body (cancel by oid).
//!   4. DELETE with body (cancel multiple by criteria).
//!   5. DELETE with query string (cancel all).

use obsdn_sdk::types::v1::{
    CancelAllOrdersRequest, CancelAllOrdersResponse, CancelOrderResponse, CancelOrdersRequest,
    CancelOrdersResponse, GetMarketsResponse, ListOpenOrdersRequest, ListOpenOrdersResponse, Order,
};
use obsdn_sdk::{Client, Env};
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> Client {
    Client::builder()
        .env(Env::Staging)
        .rest_base_url(server.uri())
        .api_key("KEY", "SECRET")
        .build()
        .expect("build client")
}

#[tokio::test]
async fn cancel_order_uses_path_param_and_no_body() {
    let server = MockServer::start().await;

    let envelope = serde_json::json!({
        "data": CancelOrderResponse { ord: Some(Order::default()) },
    });
    Mock::given(method("DELETE"))
        // Slash-containing oid would be percent-encoded; here the value is
        // safe ASCII. Verify the literal mounts.
        .and(path("/orders/abc-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&envelope))
        .mount(&server)
        .await;

    let resp = client(&server)
        .orders()
        .cancel("abc-123")
        .await
        .expect("cancel");
    assert!(resp.ord.is_some());
}

#[tokio::test]
async fn cancel_order_percent_encodes_unsafe_chars() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        // " " → "%20", "/" → "%2F". The path matcher in wiremock matches
        // the URL path before percent-decoding, so we assert the encoded
        // form is on the wire.
        .and(path("/orders/abc%20%2F123"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data": CancelOrderResponse{ord: None}})),
        )
        .mount(&server)
        .await;

    let _ = client(&server).orders().cancel("abc /123").await.unwrap();
}

#[tokio::test]
async fn list_open_orders_serializes_query_string() {
    let server = MockServer::start().await;

    let body = serde_json::json!({
        "data": ListOpenOrdersResponse { ords: vec![] },
    });
    Mock::given(method("GET"))
        .and(path("/orders"))
        .and(query_param("mktId", "BTC-PERP"))
        // Enums are serialized as SCREAMING_SNAKE strings; the query
        // encoder forwards the JSON form unchanged.
        .and(query_param("ot", "ORDER_TYPE_LIMIT"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let resp = client(&server)
        .orders()
        .list_open(ListOpenOrdersRequest {
            mkt_id: "BTC-PERP".into(),
            ot: obsdn_sdk::types::v1::OrderType::Limit as i32,
            ..Default::default()
        })
        .await
        .expect("list_open");
    assert!(resp.ords.is_empty());
}

#[tokio::test]
async fn cancel_orders_sends_json_body_on_delete() {
    let server = MockServer::start().await;

    let req = CancelOrdersRequest {
        oids: vec!["a".into(), "b".into()],
        ..Default::default()
    };
    let resp = serde_json::json!({"data": CancelOrdersResponse::default()});
    Mock::given(method("DELETE"))
        .and(path("/orders"))
        .and(body_json(&req))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp))
        .mount(&server)
        .await;

    let _ = client(&server).orders().cancel_many(req).await.unwrap();
}

#[tokio::test]
async fn cancel_all_uses_delete_with_query() {
    let server = MockServer::start().await;
    let resp = serde_json::json!({"data": CancelAllOrdersResponse::default()});
    Mock::given(method("DELETE"))
        .and(path("/orders/all"))
        .and(query_param("mktId", "ETH-PERP"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp))
        .mount(&server)
        .await;

    let _ = client(&server)
        .orders()
        .cancel_all(CancelAllOrdersRequest {
            mkt_id: "ETH-PERP".into(),
        })
        .await
        .unwrap();
}

/// Quick exercise of every API accessor - confirms wiring + types
/// resolve. We don't actually invoke RPCs (would need 10+ mock setups);
/// just instantiate each handle.
#[tokio::test]
async fn all_api_accessors_resolve() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/markets"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data": GetMarketsResponse::default()})),
        )
        .mount(&server)
        .await;

    let c = client(&server);
    let _ = c.orders();
    let _ = c.markets();
    let _ = c.account();
    let _ = c.asset();
    let _ = c.auth();
    let _ = c.chain();
    let _ = c.general();
    let _ = c.portfolio();
    let _ = c.price();
    let _ = c.subaccount();
    let _ = c.vault();
    // One real call to confirm the markets handle still works through
    // the new wiring.
    let _ = c.markets().list().await.unwrap();
}
