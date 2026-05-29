//! Send funds to another OBSDN account (sub-account or peer).
//!
//! ```bash
//! OBSDN_API_KEY=... OBSDN_API_SECRET=... OBSDN_PRIVATE_KEY=0x... \
//!     OBSDN_TO=0x... OBSDN_TOKEN=0x... OBSDN_AMOUNT=1.0 \
//!     cargo run --example transfer
//! ```

use std::sync::Arc;

use alloy_primitives::Address;
use anyhow::{Context, Result};
use obsdn_sdk::{Client, Env, LocalSigner};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let api_key = std::env::var("OBSDN_API_KEY").context("OBSDN_API_KEY")?;
    let api_secret = std::env::var("OBSDN_API_SECRET").context("OBSDN_API_SECRET")?;
    let private_key = std::env::var("OBSDN_PRIVATE_KEY").context("OBSDN_PRIVATE_KEY")?;
    let to: Address = std::env::var("OBSDN_TO").context("OBSDN_TO")?.parse()?;
    let token: Address = std::env::var("OBSDN_TOKEN")
        .context("OBSDN_TOKEN")?
        .parse()?;
    let amount: f64 = std::env::var("OBSDN_AMOUNT")
        .unwrap_or_else(|_| "1.0".into())
        .parse()?;

    let signer = Arc::new(LocalSigner::from_hex(&private_key)?);
    let client = Client::builder()
        .env(Env::Production)
        .api_key(api_key, api_secret)
        .eip712_signer(signer)
        .build()?;

    // One call scales the amount, signs the EIP-712 Transfer, and posts it.
    let resp = client.account().transfer(to, token, amount).await?;
    tracing::info!(?resp, "transfer result");
    Ok(())
}
