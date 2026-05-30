//! Book subscriber with REST-based resync on reconnect.
//!
//! ```bash
//! cargo run --example book_with_resync -- BTC-PERP
//! ```
//!
//! Pulse does NOT replay missed updates across a dropped connection. After
//! the supervisor reconnects it auto-resubscribes and pulse sends a fresh
//! `Snapshot` frame, so the local book rebuilds itself. This example also
//! shows fetching a REST snapshot via `markets().order_book(...)` on
//! `Reconnected` as a belt-and-suspenders rebuild.

use std::collections::BTreeMap;

use anyhow::Result;
use futures_util::StreamExt;
use obsdn_sdk::ws::{Book, Channel, Event};
use obsdn_sdk::{Client, Env};

#[derive(Default)]
struct LocalBook {
    bids: BTreeMap<String, String>,
    asks: BTreeMap<String, String>,
}

impl LocalBook {
    fn replace_with(&mut self, view: &Book) {
        self.bids.clear();
        self.asks.clear();
        for [px, sz] in &view.bids {
            self.bids.insert(px.clone(), sz.clone());
        }
        for [px, sz] in &view.asks {
            self.asks.insert(px.clone(), sz.clone());
        }
    }
    fn apply_diff(&mut self, view: &Book) {
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
    let client = Client::builder().env(Env::Production).build()?;
    let ws = client.ws();
    let mut stream = ws.subscribe(Channel::book(market.clone())).await?;
    let mut book = LocalBook::default();

    while let Some(evt) = stream.next().await {
        match evt {
            Event::Update(u) => {
                let view = u.as_book()?;
                match u.kind {
                    obsdn_sdk::ws::UpdateKind::Snapshot => book.replace_with(&view),
                    obsdn_sdk::ws::UpdateKind::Update => book.apply_diff(&view),
                    _ => {}
                }
                let (bid, ask, nb, na) = book.summary();
                tracing::info!(?bid, ?ask, nb, na, gsn = u.gsn, "book");
            }
            Event::Reconnected => {
                tracing::info!("reconnected - refetching REST snapshot");
                let snap = client.markets().order_book(&market).await?;
                tracing::info!(
                    levels_b = snap.book.as_ref().map(|b| b.bids.len()).unwrap_or(0),
                    levels_a = snap.book.as_ref().map(|b| b.asks.len()).unwrap_or(0),
                    "rest snapshot"
                );
                // Caller would seed `book` from `snap.book` here. The WS
                // stream also delivers a fresh `Snapshot` frame on resub,
                // which `replace_with` applies - either path rebuilds.
            }
            Event::Unauthorized(msg) => tracing::error!(%msg, "unauthorized"),
            Event::Lagged { channel, filter } => {
                tracing::warn!(?channel, %filter, "lagged - reseed from a REST snapshot");
            }
            _ => {}
        }
    }
    Ok(())
}
