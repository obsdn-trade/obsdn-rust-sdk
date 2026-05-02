//! Place a single LIMIT order on staging.
//!
//! Run:
//! ```bash
//! OBSDN_API_KEY=... OBSDN_API_SECRET=... OBSDN_PRIVATE_KEY=0x...
//!     cargo run --example place_order
//! ```
//!
//! Resolves the market index via the lazy cache, signs the EIP-712 `Order`
//! payload with the local key, and posts `/orders`. Bias the price low
//! (`buy` 1k below mark) so the order rests on the book without filling.

use std::sync::Arc;

use anyhow::{Context, Result};
use obsdn_sdk::rest::orders::PlaceEasy;
use obsdn_sdk::types::v1::OrderSide;
use obsdn_sdk::{Client, Env, LocalSigner};

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
        .env(Env::Staging)
        .api_key(api_key, api_secret)
        .eip_signer(signer)
        .build()?;

    // Pull a fresh mark price so we can quote a non-filling buy.
    let market = client.resolve_market("BTC-PERP").await?;
    let mark: f64 = market.mark_px.parse().unwrap_or(50_000.0);
    let bid_px = (mark * 0.95_f64).round();
    tracing::info!(mark, bid_px, "quoting limit buy 5% under mark");

    let resp = client
        .orders()
        .place_easy(PlaceEasy::limit("BTC-PERP", OrderSide::Buy, bid_px, 0.001))
        .await?;
    tracing::info!(?resp, "order placed");
    Ok(())
}
