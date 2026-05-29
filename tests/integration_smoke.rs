//! Integration smoke tests against the live production gateway.
//!
//! Skipped silently when `OBSDN_API_KEY` / `..._SECRET` are absent
//! so CI / local `cargo test` runs don't fail without credentials.
//!
//! Run with creds:
//!
//! ```sh
//! OBSDN_SMOKE=1 OBSDN_API_KEY=... OBSDN_API_SECRET=... \
//!     cargo test --test integration_smoke -- --nocapture
//! ```
//!
//! These tests exercise the public `GET /markets` (no auth) and the
//! authenticated `GET /accounts/me`-equivalent - kept narrow so a
//! registered key is enough; we don't place real orders here.

use obsdn_sdk::{Client, Env};

fn creds() -> Option<(String, String)> {
    let key = std::env::var("OBSDN_API_KEY").ok()?;
    let secret = std::env::var("OBSDN_API_SECRET").ok()?;
    Some((key, secret))
}

#[tokio::test]
async fn get_markets_smoke() {
    // Public endpoint - no creds needed, but we still skip in CI by
    // requiring an explicit opt-in env var so untrusted networks don't
    // hammer production from PR CI.
    if std::env::var("OBSDN_SMOKE").is_err() {
        eprintln!("skipping: set OBSDN_SMOKE=1 to enable");
        return;
    }
    let client = Client::builder()
        .env(Env::Production)
        .build()
        .expect("build client");
    let resp = client.markets().get_markets().await.expect("get_markets");
    assert!(
        !resp.mkts.is_empty(),
        "production should expose at least one market"
    );
    eprintln!("markets: {}", resp.mkts.len());
}

#[tokio::test]
async fn authenticated_smoke() {
    let Some((key, secret)) = creds() else {
        eprintln!("skipping: OBSDN_API_KEY/SECRET not set");
        return;
    };
    let client = Client::builder()
        .env(Env::Production)
        .api_key(key, secret)
        .build()
        .expect("build authed client");
    let resp = client
        .markets()
        .get_markets()
        .await
        .expect("authed get_markets");
    assert!(!resp.mkts.is_empty());
}
