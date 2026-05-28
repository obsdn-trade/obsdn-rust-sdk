//! Staging integration smoke tests.
//!
//! Run: OBSDN_STAGING=1 cargo test --test staging_smoke -- --nocapture
//!
//! Requires staging creds via env vars or falls back to hardcoded test key.

use obsdn_sdk::types::v1::{GetClientInfoRequest, GetFeeTiersRequest, GetPortfolioRequest};
use obsdn_sdk::{Client, Env};

fn skip_unless_staging() -> bool {
    if std::env::var("OBSDN_STAGING").is_err() {
        eprintln!("skipping: set OBSDN_STAGING=1 to enable");
        return true;
    }
    false
}

fn staging_client_unauthed() -> Client {
    Client::builder()
        .env(Env::Staging)
        .danger_accept_invalid_certs(true)
        .build()
        .expect("build staging client")
}

fn staging_client_authed() -> Client {
    let key = std::env::var("OBSDN_API_KEY")
        .unwrap_or_else(|_| "0ede9a77f5651c4c6c2acd76b20078bc".to_string());
    let secret = std::env::var("OBSDN_API_SECRET").unwrap_or_else(|_| {
        "4b29e2587ee4b4cd89e78904f72d06ed644bca2f5c437643326c911912a3a958".to_string()
    });
    Client::builder()
        .env(Env::Staging)
        .api_key(key, secret)
        .danger_accept_invalid_certs(true)
        .build()
        .expect("build staging authed client")
}

#[tokio::test]
async fn staging_get_markets() {
    if skip_unless_staging() {
        return;
    }
    let client = staging_client_unauthed();
    let resp = client.markets().get_markets().await.expect("get_markets");
    assert!(!resp.mkts.is_empty(), "staging should have markets");
    for m in &resp.mkts {
        let idx: u16 = m.idx.parse().expect("idx parses as u16");
        eprintln!("  {} idx={idx}", m.mkt_id);
    }
    eprintln!("OK: {} markets", resp.mkts.len());
}

#[tokio::test]
async fn staging_get_fee_tiers() {
    if skip_unless_staging() {
        return;
    }
    let client = staging_client_unauthed();
    let resp = client
        .general()
        .get_fee_tiers(GetFeeTiersRequest {})
        .await
        .expect("get_fee_tiers");
    assert!(!resp.tiers.is_empty(), "staging should have fee tiers");
    for t in &resp.tiers {
        eprintln!(
            "  {} maker={} taker={}",
            t.name, t.maker_fee_rate, t.taker_fee_rate
        );
    }
    eprintln!("OK: {} fee tiers", resp.tiers.len());
}

#[tokio::test]
async fn staging_get_client_info() {
    if skip_unless_staging() {
        return;
    }
    let client = staging_client_unauthed();
    let resp = client
        .general()
        .get_client_info(GetClientInfoRequest {})
        .await
        .expect("get_client_info");
    eprintln!("OK: client info received");
    eprintln!("  {:?}", resp);
}

#[tokio::test]
async fn staging_get_portfolio_authed() {
    if skip_unless_staging() {
        return;
    }
    let client = staging_client_authed();
    let resp = client
        .portfolio()
        .get(GetPortfolioRequest::default())
        .await
        .expect("get_portfolio");
    eprintln!("OK: portfolio received");
    if let Some(ft) = &resp.fee_tier {
        eprintln!(
            "  fee tier: {} mkr={} tkr={} vol30d={}",
            ft.tier_nm, ft.mkr_fee_rt, ft.tkr_fee_rt, ft.vol_30d
        );
    }
}
