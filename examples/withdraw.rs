//! Withdraw collateral on-chain.
//!
//! ```bash
//! OBSDN_API_KEY=... OBSDN_API_SECRET=... OBSDN_PRIVATE_KEY=0x... \
//!     OBSDN_TOKEN=0x... OBSDN_AMOUNT=10.5 \
//!     cargo run --example withdraw
//! ```
//!
//! Signs the withdrawal and posts it; the chain-writer service submits the
//! on-chain transaction. Track completion via the `notification` WS channel.

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
    let token: Address = std::env::var("OBSDN_TOKEN")
        .context("OBSDN_TOKEN")?
        .parse()?;
    // A decimal string, passed through verbatim (no f64 round-trip).
    let amount = std::env::var("OBSDN_AMOUNT").unwrap_or_else(|_| "1".into());

    let signer = Arc::new(LocalSigner::from_hex(&private_key)?);
    let client = Client::builder()
        .env(Env::Production)
        .api_key(api_key, api_secret)
        .eip712_signer(signer)
        .build()?;

    // One call scales the amount, signs the EIP-712 Withdraw, and posts it.
    let resp = client.account().withdraw(token, amount).await?;
    tracing::info!(?resp, "withdraw result");
    Ok(())
}
