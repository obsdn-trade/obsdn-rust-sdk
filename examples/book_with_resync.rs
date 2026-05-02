//! Book subscriber with REST-based resync on `Gap`.
//!
//! ```bash
//! cargo run --example book_with_resync -- BTC-PERP
//! ```
//!
//! Pulse does NOT replay missed updates — when GSNs skip, the local book
//! is stale and must be rebuilt. This example fetches a fresh REST
//! snapshot via `markets().get_order_book(...)` whenever the supervisor
//! emits a `Gap` event.

use std::collections::BTreeMap;

use anyhow::Result;
use futures_util::StreamExt;
use obsdn_sdk::ws::{BookView, Channel, WsEvent};
use obsdn_sdk::{Client, Env};

#[derive(Default)]
struct Book {
    bids: BTreeMap<String, String>,
    asks: BTreeMap<String, String>,
}

impl Book {
    fn replace_with(&mut self, view: &BookView) {
        self.bids.clear();
        self.asks.clear();
        for [px, sz] in &view.bids {
            self.bids.insert(px.clone(), sz.clone());
        }
        for [px, sz] in &view.asks {
            self.asks.insert(px.clone(), sz.clone());
        }
    }
    fn apply_diff(&mut self, view: &BookView) {
        for [px, sz] in &view.bids {
            apply_level(&mut self.bids, px, sz);
        }
        for [px, sz] in &view.asks {
            apply_level(&mut self.asks, px, sz);
        }
    }
    fn summary(&self) -> (Option<&String>, Option<&String>, usize, usize) {
        (
            self.bids.keys().next_back(),
            self.asks.keys().next(),
            self.bids.len(),
            self.asks.len(),
        )
    }
}

fn apply_level(side: &mut BTreeMap<String, String>, px: &str, sz: &str) {
    if sz == "0" || sz == "0.0" {
        side.remove(px);
    } else {
        side.insert(px.to_string(), sz.to_string());
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let market = std::env::args().nth(1).unwrap_or_else(|| "BTC-PERP".into());
    let client = Client::builder().env(Env::Staging).build()?;
    let ws = client.ws();
    let mut stream = ws
        .subscribe(Channel::Book {
            market: market.clone(),
        })
        .await?;
    let mut book = Book::default();

    while let Some(evt) = stream.next().await {
        match evt {
            WsEvent::Update(u) => {
                let view = u.as_book()?;
                match u.kind {
                    obsdn_sdk::ws::WsUpdateKind::Snapshot => book.replace_with(&view),
                    obsdn_sdk::ws::WsUpdateKind::Update => book.apply_diff(&view),
                }
                let (bid, ask, nb, na) = book.summary();
                tracing::info!(?bid, ?ask, nb, na, gsn = u.gsn, "book");
            }
            WsEvent::Gap { from, to } => {
                tracing::warn!(from, to, "gap — refetching REST snapshot");
                let snap = client.markets().get_order_book(&market).await?;
                tracing::info!(
                    levels_b = snap.book.as_ref().map(|b| b.bids.len()).unwrap_or(0),
                    levels_a = snap.book.as_ref().map(|b| b.asks.len()).unwrap_or(0),
                    "rest snapshot"
                );
                // Caller would seed `book` from `snap.book` here. The WS
                // stream resumes at the live head — the gap window is gone.
            }
            WsEvent::Reconnected => tracing::info!("reconnected — next frame is a fresh snapshot"),
            WsEvent::Unauthorized(msg) => tracing::error!(%msg, "unauthorized"),
        }
    }
    Ok(())
}
