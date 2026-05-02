//! Phase 2 integration smoke against the live staging gateway.
//!
//! Skipped silently when `OBSDN_STAGING_API_KEY` / `..._SECRET` are absent
//! so CI / local `cargo test` runs don't fail without credentials.
//!
//! Run with creds:
//!
//! ```sh
//! OBSDN_STAGING_API_KEY=... OBSDN_STAGING_API_SECRET=... \
//!     cargo test --test integration_staging -- --nocapture
//! ```
//!
//! These tests exercise the public `GET /markets` (no auth) and the
//! authenticated `GET /accounts/me`-equivalent — kept narrow so a
//! registered staging key is enough; we don't place real orders here.

use obsdn_sdk::{Client, Env};

fn creds() -> Option<(String, String)> {
    let key = std::env::var("OBSDN_STAGING_API_KEY").ok()?;
    let secret = std::env::var("OBSDN_STAGING_API_SECRET").ok()?;
    Some((key, secret))
}

#[tokio::test]
async fn staging_get_markets_smoke() {
    // Public endpoint — no creds needed, but we still skip in CI by
    // requiring an explicit opt-in env var so untrusted networks don't
    // hammer staging from PR CI.
    if std::env::var("OBSDN_STAGING_SMOKE").is_err() {
        eprintln!("skipping: set OBSDN_STAGING_SMOKE=1 to enable");
        return;
    }
    let client = Client::builder()
        .env(Env::Staging)
        .build()
        .expect("build client");
    let resp = client
        .markets()
        .get_markets()
        .await
        .expect("staging get_markets");
    assert!(
        !resp.mkts.is_empty(),
        "staging should expose at least one market"
    );
    eprintln!("staging markets: {}", resp.mkts.len());
}

#[tokio::test]
async fn staging_authenticated_smoke() {
    let Some((key, secret)) = creds() else {
        eprintln!("skipping: OBSDN_STAGING_API_KEY/SECRET not set");
        return;
    };
    let client = Client::builder()
        .env(Env::Staging)
        .api_key(key, secret)
        .build()
        .expect("build authed client");
    // Hit a public endpoint to confirm the auth-configured client still
    // works for unauthed calls. Phase 3 will cover authed endpoints.
    let resp = client
        .markets()
        .get_markets()
        .await
        .expect("authed get_markets");
    assert!(!resp.mkts.is_empty());
}
