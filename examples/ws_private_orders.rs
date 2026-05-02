//! Authenticate and subscribe to the private `order` channel.
//!
//! ```bash
//! OBSDN_API_KEY=... OBSDN_API_SECRET=... cargo run --example ws_private_orders
//! ```
//!
//! Streams every order lifecycle update for the authenticated account.
//! Runs until Ctrl-C.

use anyhow::{Context, Result};
use futures_util::StreamExt;
use obsdn_sdk::ws::{Channel, WsEvent};
use obsdn_sdk::{Client, Env};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let api_key = std::env::var("OBSDN_API_KEY").context("OBSDN_API_KEY")?;
    let api_secret = std::env::var("OBSDN_API_SECRET").context("OBSDN_API_SECRET")?;
    let client = Client::builder()
        .env(Env::Staging)
        .api_key(api_key, api_secret)
        .build()?;

    let ws = client.ws();
    let address = ws.authenticate().await?;
    tracing::info!(%address, "ws authenticated");

    let mut stream = ws.subscribe(Channel::Order { market: None }).await?;
    while let Some(evt) = stream.next().await {
        match evt {
            WsEvent::Update(u) => {
                let orders = u.as_orders()?;
                for o in orders {
                    tracing::info!(
                        oid = %o.oid,
                        mkt = %o.mkt_id,
                        st = %o.st,
                        filled = %o.filled_sz,
                        "order update"
                    );
                }
            }
            WsEvent::Gap { from, to } => tracing::warn!(from, to, "gap"),
            WsEvent::Reconnected => tracing::info!("reconnected"),
            WsEvent::Unauthorized(msg) => {
                tracing::error!(%msg, "unauthorized — auth replay failed");
                break;
            }
        }
    }
    Ok(())
}
