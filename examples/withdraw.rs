//! Withdraw collateral on-chain.
//!
//! ```bash
//! OBSDN_API_KEY=... OBSDN_API_SECRET=... OBSDN_PRIVATE_KEY=0x... \
//!     OBSDN_TOKEN=0x... OBSDN_AMOUNT=10.5 \
//!     cargo run --example withdraw
//! ```
//!
//! Signs the EIP-712 `Withdraw` payload, then POSTs `/transfers/withdraw`.
//! The chain-writer service picks up the request and submits the on-chain
//! transaction; observe completion via the `notification` WS channel.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use alloy_primitives::Address;
use anyhow::{Context, Result};
use obsdn_sdk::sign::{scale_decimal_str, sign_withdraw, withdraw::WithdrawPayload};
use obsdn_sdk::types::v1::WithdrawCollateralRequest;
use obsdn_sdk::{Client, EipSigner, Env, LocalSigner};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let api_key = std::env::var("OBSDN_API_KEY").context("OBSDN_API_KEY")?;
    let api_secret = std::env::var("OBSDN_API_SECRET").context("OBSDN_API_SECRET")?;
    let private_key = std::env::var("OBSDN_PRIVATE_KEY").context("OBSDN_PRIVATE_KEY")?;
    let token: Address = std::env::var("OBSDN_TOKEN")
        .context("OBSDN_TOKEN")?
        .parse()?;
    let amount = std::env::var("OBSDN_AMOUNT").unwrap_or_else(|_| "1.0".into());

    let signer = Arc::new(LocalSigner::from_hex(&private_key)?);
    let client = Client::builder()
        .env(Env::Staging)
        .api_key(api_key, api_secret)
        .eip_signer(signer.clone())
        .build()?;

    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as u64;
    let payload = WithdrawPayload {
        sender: signer.address(),
        token,
        amount: scale_decimal_str(&amount)?,
        nonce,
    };
    let sig = sign_withdraw(signer.as_ref(), client.eip712_domain(), payload)?;

    let req = WithdrawCollateralRequest {
        tkn: format!("{token:#x}"),
        amt: amount,
        nonce,
        sig: obsdn_sdk::sign::signature_hex(&sig),
    };
    let resp = client.account().withdraw_collateral(req).await?;
    tracing::info!(?resp, "withdraw_collateral result");
    Ok(())
}
