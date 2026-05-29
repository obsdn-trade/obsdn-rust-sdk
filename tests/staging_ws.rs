//! Additional staging WebSocket coverage against the live `pulse` service.
//!
//! Complements `e2e_staging.rs` (which covers the public book channel and the
//! authenticated wildcard order flow) with the ticker and oracle channels and
//! a public all-markets (wildcard) trade check. Position decode (both the
//! snapshot array and single-object update shapes) is covered deterministically
//! by `views::tests::position_update_single_object_decodes`.
//!
//! Run: `OBSDN_STAGING=1 cargo test --test staging_ws -- --nocapture --test-threads=1`
//!
//! All tests skip unless `OBSDN_STAGING=1` so the suite compiles (and
//! no-ops) in CI without network access.

use std::time::Duration;

use futures_util::StreamExt;
use obsdn_sdk::ws::{Channel, SubscriptionStream, WsEvent, WsUpdate};
use obsdn_sdk::{Client, Env};
use tokio::time::timeout;

fn skip_unless_staging() -> bool {
    if std::env::var("OBSDN_STAGING").is_err() {
        eprintln!("skipping: set OBSDN_STAGING=1 to enable");
        return true;
    }
    false
}

fn unauthed() -> Client {
    Client::builder()
        .env(Env::Staging)
        .build()
        .expect("build staging client")
}

/// Read events until the first data frame (snapshot or update) arrives or we
/// time out. Panics on an auth failure.
async fn first_data(stream: &mut SubscriptionStream, secs: u64) -> Option<WsUpdate> {
    loop {
        match timeout(Duration::from_secs(secs), stream.next()).await {
            Ok(Some(WsEvent::Update(u))) => return Some(u),
            Ok(Some(WsEvent::Unauthorized(m))) => panic!("ws auth rejected: {m}"),
            Ok(Some(WsEvent::Reconnected)) => continue,
            Ok(None) | Err(_) => return None,
        }
    }
}

#[tokio::test]
async fn staging_ws_ticker() {
    if skip_unless_staging() {
        return;
    }
    let ws = unauthed().ws();
    let mut s = ws
        .subscribe(Channel::Ticker {
            market: "BTC-PERP".into(),
        })
        .await
        .expect("subscribe ticker");
    let u = first_data(&mut s, 15)
        .await
        .expect("ticker frame within 15s");
    let t = u.as_ticker().expect("ticker decodes");
    eprintln!("OK: ticker bid={} ask={}", t.bid.px, t.ask.px);
    ws.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn staging_ws_oracle() {
    if skip_unless_staging() {
        return;
    }
    let ws = unauthed().ws();
    let mut s = ws
        .subscribe(Channel::Oracle {
            asset: "BTC".into(),
        })
        .await
        .expect("subscribe oracle");
    let u = first_data(&mut s, 15)
        .await
        .expect("oracle frame within 15s");
    let o = u.as_oracle().expect("oracle decodes");
    assert_eq!(o.asset, "BTC");
    eprintln!("OK: oracle BTC mark_px={}", o.mark_px);
    ws.shutdown().await.expect("shutdown");
}

/// C1 live check: subscribe to the all-markets trade channel (`market:
/// None`). Pulse stamps each trade frame with a concrete market filter; the
/// wildcard routing must still deliver them. Trades are organic, so this is
/// opportunistic - when the market is active it verifies routing end to end.
#[tokio::test]
async fn staging_ws_wildcard_trade_routes() {
    if skip_unless_staging() {
        return;
    }
    let ws = unauthed().ws();
    let mut s = ws
        .subscribe(Channel::Trade { market: None })
        .await
        .expect("subscribe all-markets trade");
    match first_data(&mut s, 20).await {
        Some(u) => {
            let t = u.as_trade().expect("trade decodes");
            assert!(
                !u.filter.is_empty(),
                "trade frame must carry a concrete market filter"
            );
            eprintln!("OK: wildcard trade routed live: {} px={}", u.filter, t.px);
        }
        None => eprintln!("NOTE: no trade activity in 20s window; wildcard routing not exercised"),
    }
    ws.shutdown().await.expect("shutdown");
}
