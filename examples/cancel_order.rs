//! Cancel an order by id.
//!
//! ```bash
//! OBSDN_API_KEY=... OBSDN_API_SECRET=... \
//!     cargo run --example cancel_order -- <order-id>
//! ```
//!
//! Cancels do NOT require an EIP-712 signer — server treats the HMAC'd
//! REST request as authorization. The order id is the UUID returned from
//! `OrdersApi::place`.

use anyhow::{anyhow, Context, Result};
use obsdn_sdk::{Client, Env};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let oid = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow!("usage: cancel_order <order-id>"))?;
    let api_key = std::env::var("OBSDN_API_KEY").context("OBSDN_API_KEY")?;
    let api_secret = std::env::var("OBSDN_API_SECRET").context("OBSDN_API_SECRET")?;

    let client = Client::builder()
        .env(Env::Staging)
        .api_key(api_key, api_secret)
        .build()?;

    let resp = client.orders().cancel(&oid).await?;
    tracing::info!(?resp, "cancel result");
    Ok(())
}
