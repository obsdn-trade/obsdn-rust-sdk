//! A minimal market-maker loop showing the recommended OBSDN SDK patterns:
//! one authenticated WebSocket [`Session`], a multiplexed fan-out across the
//! pricing and private channels, the full [`Event`] handling contract
//! (including `Lagged` -> resubscribe and `Reconnected` -> REST resync), a
//! mark-relative post-only quote, and a `cancel_all` on shutdown.
//!
//! ```bash
//! OBSDN_API_KEY=... OBSDN_API_SECRET=... OBSDN_PRIVATE_KEY=0x... \
//!     cargo run --example market_maker -- BTC-PERP
//! ```
//!
//! This is illustrative, not a trading strategy: it places one resting quote
//! on each side, then logs fills/positions until a time budget elapses, then
//! cancels everything. A real maker would re-quote on every book/ticker tick.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use obsdn_sdk::rest::orders::LimitOrder;
use obsdn_sdk::types::v1::{CancelAllOrdersRequest, OrderSide};
use obsdn_sdk::ws::{Channel, Event, Session, Update};
use obsdn_sdk::{Client, Env, LocalSigner};
use tokio_stream::StreamMap;

/// How long to run the demo loop before cancelling and exiting.
const RUN_FOR: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let market = std::env::args().nth(1).unwrap_or_else(|| "BTC-PERP".into());

    // --- Client: HMAC for REST + private channels, EIP-712 signer for orders.
    let signer = Arc::new(LocalSigner::from_hex(
        &std::env::var("OBSDN_PRIVATE_KEY").context("OBSDN_PRIVATE_KEY")?,
    )?);
    let client = Client::builder()
        .env(Env::Production)
        .api_key(
            std::env::var("OBSDN_API_KEY").context("OBSDN_API_KEY")?,
            std::env::var("OBSDN_API_SECRET").context("OBSDN_API_SECRET")?,
        )
        .eip712_signer(signer)
        .build()?;

    // --- One Session, authenticated once. Private channels require auth; the
    // supervisor replays it on every reconnect.
    let ws = client.ws();
    ws.authenticate().await.context("ws authenticate")?;

    // --- Fan out across the channels a maker needs, multiplexed into one loop.
    // Keys label each stream so we can resubscribe a single channel on lag.
    let mut streams: StreamMap<&'static str, _> = StreamMap::new();
    streams.insert("book", ws.subscribe(Channel::book(&market)).await?);
    streams.insert("ticker", ws.subscribe(Channel::ticker(&market)).await?);
    streams.insert("order", ws.subscribe(Channel::order(None)).await?);
    streams.insert("position", ws.subscribe(Channel::position(None)).await?);

    // --- Seed a quote around the current mark. `place_limit` resolves the
    // market index, scales + signs the EIP-712 order, and POSTs it. post_only
    // rejects anything that would take liquidity (a maker never crosses).
    let mkt = client.resolve_market(&market).await?;
    if let Some(mark) = mkt.mark_price() {
        place_quotes(&client, &market, mark).await;
    }

    // --- Event loop. Handle every Event variant - this is the contract.
    let deadline = tokio::time::Instant::now() + RUN_FOR;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, streams.next()).await {
            Err(_) => break,   // time budget elapsed
            Ok(None) => break, // all streams ended
            Ok(Some((key, evt))) => handle_event(&ws, &market, key, evt).await,
        }
    }

    // --- Always flatten quotes on the way out.
    let _ = client
        .orders()
        .cancel_all(CancelAllOrdersRequest::default())
        .await;
    ws.shutdown().await.ok();
    tracing::info!("market maker stopped; orders cancelled");
    Ok(())
}

/// Place a resting post-only quote on each side, 0.1% off the mark.
async fn place_quotes(client: &Client, market: &str, mark: f64) {
    let bid = mark * 0.999;
    let ask = mark * 1.001;
    for (side, px) in [(OrderSide::Buy, bid), (OrderSide::Sell, ask)] {
        let res = client
            .orders()
            .place_limit(LimitOrder::new(market, side, px, 0.001).post_only(true))
            .await;
        match res {
            Ok(_) => tracing::info!(?side, px, "quote placed"),
            Err(e) => tracing::warn!(?side, px, error = %e, "quote rejected"),
        }
    }
}

/// Dispatch one `(channel, Event)`. On `Lagged` the subscription was dropped,
/// so resubscribe it; on `Reconnected` re-seed from REST if you keep local
/// state; on `Unauthorized` the private feed stopped.
async fn handle_event(ws: &Session, market: &str, key: &'static str, evt: Event) {
    match evt {
        Event::Update(u) => log_update(key, &u),
        Event::Lagged { channel, .. } => {
            tracing::warn!(%key, ?channel, "lagged; resubscribing to resync");
            // The old registration is gone; resubscribe to get a fresh snapshot.
            // (Errors here just mean the session is shutting down.)
            let _ = resubscribe(ws, key, market).await;
        }
        Event::Reconnected => {
            // The socket reconnected and the server will re-send a snapshot.
            // If you maintain a local book, also refetch via REST here.
            tracing::info!(%key, "reconnected; awaiting fresh snapshot");
        }
        Event::Unauthorized(msg) => {
            tracing::error!(%key, %msg, "auth lost; private feed paused (will retry on reconnect)");
        }
        _ => {}
    }
}

async fn resubscribe(ws: &Session, key: &'static str, market: &str) -> Result<()> {
    let channel = match key {
        "book" => Channel::book(market),
        "ticker" => Channel::ticker(market),
        "order" => Channel::order(None),
        "position" => Channel::position(None),
        _ => return Ok(()),
    };
    // In a full maker you would re-insert the returned stream into the
    // StreamMap; here we just demonstrate the recovery call.
    ws.subscribe(channel).await?;
    Ok(())
}

fn log_update(key: &str, u: &Update) {
    match key {
        "ticker" => {
            if let Ok(t) = u.as_ticker() {
                tracing::info!(bid = %t.bid.price, ask = %t.ask.price, "ticker");
            }
        }
        "order" => {
            if let Ok(orders) = u.as_orders() {
                for o in orders {
                    tracing::info!(oid = %o.oid, status = %o.status, filled = %o.filled_size, "order");
                }
            }
        }
        "position" => {
            if let Ok(ps) = u.as_positions() {
                for p in ps {
                    tracing::info!(market = %p.market_id, net = %p.net_size, upnl = %p.unrealized_pnl, "position");
                }
            }
        }
        _ => {}
    }
}
