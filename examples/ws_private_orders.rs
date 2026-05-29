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
use obsdn_sdk::ws::{Channel, Event};
use obsdn_sdk::{Client, Env};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let api_key = std::env::var("OBSDN_API_KEY").context("OBSDN_API_KEY")?;
    let api_secret = std::env::var("OBSDN_API_SECRET").context("OBSDN_API_SECRET")?;
    let client = Client::builder()
        .env(Env::Production)
        .api_key(api_key, api_secret)
        .build()?;

    let ws = client.ws();
    let address = ws.authenticate().await?;
    tracing::info!(%address, "ws authenticated");

    let mut stream = ws.subscribe(Channel::order(None)).await?;
    while let Some(evt) = stream.next().await {
        match evt {
            Event::Update(u) => {
                let orders = u.as_orders()?;
                for o in orders {
                    tracing::info!(
                        oid = %o.oid,
                        market = %o.market_id,
                        status = %o.status,
                        filled = %o.filled_size,
                        "order update"
                    );
                }
            }
            Event::Reconnected => tracing::info!("reconnected"),
            Event::Unauthorized(msg) => {
                tracing::error!(%msg, "unauthorized - auth replay failed");
                break;
            }
        }
    }
    Ok(())
}
