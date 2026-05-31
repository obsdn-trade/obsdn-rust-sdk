//! Unit coverage for the one-call `Account::transfer` / `withdraw` helpers.

use std::sync::Arc;

use alloy_primitives::{address, Address};
use obsdn_sdk::types::v1::{
    SendFundsRequest, SendFundsResponse, WithdrawCollateralRequest, WithdrawCollateralResponse,
};
use obsdn_sdk::{Client, Env, Error, LocalSigner};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TEST_KEY: &str = "0x0101010101010101010101010101010101010101010101010101010101010101";

fn signed_client(server: &MockServer) -> Client {
    Client::builder()
        .env(Env::Staging)
        .rest_base_url(server.uri())
        .api_key("KEY", "SECRET")
        .eip712_signer(Arc::new(LocalSigner::from_hex(TEST_KEY).unwrap()))
        .build()
        .unwrap()
}

#[tokio::test]
async fn transfer_scales_signs_and_posts() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/transfers/send-funds"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data": SendFundsResponse::default()})),
        )
        .mount(&server)
        .await;

    let to: Address = address!("0000000000000000000000000000000000000001");
    let token: Address = address!("0000000000000000000000000000000000000002");
    signed_client(&server)
        .account()
        .transfer(to, token, 1.5)
        .await
        .expect("transfer");

    let reqs = server.received_requests().await.unwrap();
    let r = reqs
        .iter()
        .find(|r| r.url.path() == "/transfers/send-funds")
        .expect("send-funds posted");
    assert!(r.headers.get("x-api-key").is_some());
    let body: SendFundsRequest = serde_json::from_slice(&r.body).expect("body decodes");
    assert_eq!(body.amt, "1.5");
    assert_eq!(body.to, format!("{to:#x}"));
    assert_eq!(body.tkn, format!("{token:#x}"));
    assert!(!body.from.is_empty(), "sender address filled in");
    assert!(body.sig.starts_with("0x") && body.sig.len() == 132);
}

#[tokio::test]
async fn withdraw_scales_signs_and_posts() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/transfers/withdraw"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data": WithdrawCollateralResponse::default()})),
        )
        .mount(&server)
        .await;

    let token: Address = address!("0000000000000000000000000000000000000002");
    signed_client(&server)
        .account()
        .withdraw(token, 2.25)
        .await
        .expect("withdraw");

    let reqs = server.received_requests().await.unwrap();
    let r = reqs
        .iter()
        .find(|r| r.url.path() == "/transfers/withdraw")
        .expect("withdraw posted");
    let body: WithdrawCollateralRequest = serde_json::from_slice(&r.body).expect("body decodes");
    assert_eq!(body.amt, "2.25");
    assert_eq!(body.tkn, format!("{token:#x}"));
    assert!(body.sig.starts_with("0x") && body.sig.len() == 132);
}

#[tokio::test]
async fn transfer_with_nonce_threads_the_given_nonce() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/transfers/send-funds"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data": SendFundsResponse::default()})),
        )
        .mount(&server)
        .await;

    let to: Address = address!("0000000000000000000000000000000000000001");
    let token: Address = address!("0000000000000000000000000000000000000002");
    // A fixed nonce makes the signed transfer idempotent: a retry with the same
    // nonce is deduplicated server-side instead of moving funds twice.
    let nonce = 1_700_000_000_000_000_000u64;
    signed_client(&server)
        .account()
        .transfer_with_nonce(to, token, 1.5, nonce)
        .await
        .expect("transfer_with_nonce");

    let reqs = server.received_requests().await.unwrap();
    let r = reqs
        .iter()
        .find(|r| r.url.path() == "/transfers/send-funds")
        .expect("send-funds posted");
    let body: SendFundsRequest = serde_json::from_slice(&r.body).expect("body decodes");
    assert_eq!(body.nonce, nonce, "the supplied nonce is signed and sent");
}

#[tokio::test]
async fn withdraw_with_nonce_threads_the_given_nonce() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/transfers/withdraw"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data": WithdrawCollateralResponse::default()})),
        )
        .mount(&server)
        .await;

    let token: Address = address!("0000000000000000000000000000000000000002");
    let nonce = 1_700_000_000_000_000_001u64;
    signed_client(&server)
        .account()
        .withdraw_with_nonce(token, 2.25, nonce)
        .await
        .expect("withdraw_with_nonce");

    let reqs = server.received_requests().await.unwrap();
    let r = reqs
        .iter()
        .find(|r| r.url.path() == "/transfers/withdraw")
        .expect("withdraw posted");
    let body: WithdrawCollateralRequest = serde_json::from_slice(&r.body).expect("body decodes");
    assert_eq!(body.nonce, nonce, "the supplied nonce is signed and sent");
}

#[tokio::test]
async fn transfer_without_signer_errors() {
    let client = Client::builder()
        .env(Env::Staging)
        .api_key("KEY", "SECRET")
        .build()
        .unwrap();
    let to: Address = address!("0000000000000000000000000000000000000001");
    let token: Address = address!("0000000000000000000000000000000000000002");
    let err = client
        .account()
        .transfer(to, token, 1.0)
        .await
        .expect_err("must require a signer");
    assert!(matches!(err, Error::Sign(_)), "got {err:?}");
}
