//! Place a single LIMIT order on production.
//!
//! ```bash
//! OBSDN_API_KEY=... OBSDN_API_SECRET=... OBSDN_PRIVATE_KEY=0x... \
//!     cargo run --example place_order
//! ```
//!
//! Resolves the market, signs the order, and posts it. The price is biased
//! 5% below mark so the order rests on the book without filling.

use std::sync::Arc;

use anyhow::{Context, Result};
use obsdn_sdk::rest::orders::LimitOrder;
use obsdn_sdk::{Client, Env, LocalSigner, Side};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let api_key = std::env::var("OBSDN_API_KEY").context("OBSDN_API_KEY")?;
    let api_secret = std::env::var("OBSDN_API_SECRET").context("OBSDN_API_SECRET")?;
    let private_key = std::env::var("OBSDN_PRIVATE_KEY").context("OBSDN_PRIVATE_KEY")?;

    let signer = Arc::new(LocalSigner::from_hex(&private_key)?);
    let client = Client::builder()
        .env(Env::Production)
        .api_key(api_key, api_secret)
        .eip712_signer(signer)
        .build()?;

    // Quote a non-filling buy 5% below the current mark price.
    let market = client.resolve_market("BTC-PERP").await?;
    let mark = market.mark_price().unwrap_or(50_000.0);
    let bid = (mark * 0.95).round();
    tracing::info!(mark, bid, "quoting limit buy 5% under mark");

    let resp = client
        .orders()
        .place_limit(LimitOrder::new("BTC-PERP", Side::Buy, bid, 0.001).post_only(true))
        .await?;
    tracing::info!(?resp, "order placed");
    Ok(())
}
