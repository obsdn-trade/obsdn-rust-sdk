//! Subscribe to the book channel and print typed snapshots/updates.
//!
//! ```bash
//! cargo run --example ws_book -- BTC-PERP
//! ```
//!
//! Public channel — no auth required. Stops after 10 frames.

use anyhow::{Context, Result};
use futures_util::StreamExt;
use obsdn_sdk::ws::{Channel, WsEvent};
use obsdn_sdk::{Client, Env};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let market = std::env::args().nth(1).unwrap_or_else(|| "BTC-PERP".into());
    let client = Client::builder().env(Env::Production).build()?;
    let ws = client.ws();
    let mut stream = ws
        .subscribe(Channel::Book {
            market: market.clone(),
        })
        .await
        .context("subscribe book")?;

    let mut printed = 0;
    while let Some(evt) = stream.next().await {
        match evt {
            WsEvent::Update(u) => {
                let book = u.as_book()?;
                tracing::info!(
                    gsn = u.gsn,
                    kind = ?u.kind,
                    bids = book.bids.len(),
                    asks = book.asks.len(),
                    top_bid = ?book.bids.first(),
                    top_ask = ?book.asks.first(),
                    "{market}",
                );
                printed += 1;
                if printed >= 10 {
                    break;
                }
            }
            WsEvent::Reconnected => tracing::info!("reconnected"),
            WsEvent::Unauthorized(msg) => tracing::error!(%msg, "unauthorized"),
        }
    }
    ws.shutdown().await.ok();
    Ok(())
}
