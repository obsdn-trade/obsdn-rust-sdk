//! Managed-WS smoke tests against the live production pulse.
//!
//! Skipped silently when the gating env vars aren't set so CI doesn't
//! hammer production from PR runs. To enable:
//!
//! ```sh
//! OBSDN_SMOKE=1 cargo test --test ws_smoke -- --nocapture
//! OBSDN_API_KEY=... OBSDN_API_SECRET=... \
//!     cargo test --test ws_smoke ws_authenticated_smoke -- --nocapture
//! ```

use std::time::Duration;

use futures_util::StreamExt;
use obsdn_sdk::ws::{Channel, ChannelName, WsEvent, WsUpdateKind};
use obsdn_sdk::{Client, Env};

fn opt_in() -> bool {
    std::env::var("OBSDN_SMOKE").is_ok()
}

fn creds() -> Option<(String, String)> {
    let key = std::env::var("OBSDN_API_KEY").ok()?;
    let secret = std::env::var("OBSDN_API_SECRET").ok()?;
    Some((key, secret))
}

#[tokio::test]
async fn ws_book_subscribe_smoke() {
    if !opt_in() {
        eprintln!("skipping: set OBSDN_SMOKE=1 to enable");
        return;
    }
    let client = Client::builder()
        .env(Env::Production)
        .build()
        .expect("build client");
    let ws = client.ws();
    let mut stream = ws
        .subscribe(Channel::Book {
            market: "BTC-PERP".into(),
        })
        .await
        .expect("subscribe book BTC-PERP");

    // Snapshot is sent shortly after the `subscribed` ack. Allow up to
    // 5s. The first event MUST be an Update with kind=Snapshot - the
    // managed client never injects Reconnected before any data.
    let first = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("snapshot/update within 5s")
        .expect("stream not closed early");
    let WsEvent::Update(u) = first else {
        panic!("expected Update first, got {first:?}");
    };
    assert_eq!(u.channel, ChannelName::Book);
    assert_eq!(u.filter, "BTC-PERP");
    assert!(u.gsn > 0, "snapshot should carry a real gsn");
    assert_eq!(u.kind, WsUpdateKind::Snapshot);
    eprintln!("first frame: gsn={} kind={:?}", u.gsn, u.kind);

    ws.unsubscribe(Channel::Book {
        market: "BTC-PERP".into(),
    })
    .await
    .expect("unsubscribe");
    ws.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn ws_authenticated_smoke() {
    if !opt_in() {
        eprintln!("skipping: set OBSDN_SMOKE=1 to enable");
        return;
    }
    let Some((key, secret)) = creds() else {
        eprintln!("skipping: OBSDN_API_KEY/SECRET not set");
        return;
    };
    let client = Client::builder()
        .env(Env::Production)
        .api_key(key, secret)
        .build()
        .expect("build authed client");
    let ws = client.ws();
    let address = ws.authenticate().await.expect("auth");
    assert!(
        address.starts_with("0x"),
        "address should be hex: {address}"
    );
    eprintln!("authenticated as: {address}");
    ws.shutdown().await.expect("shutdown");
}
