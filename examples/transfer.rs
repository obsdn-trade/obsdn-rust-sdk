//! Send funds to another OBSDN account (sub-account or peer).
//!
//! ```bash
//! OBSDN_API_KEY=... OBSDN_API_SECRET=... OBSDN_PRIVATE_KEY=0x... \
//!     OBSDN_TO=0x... OBSDN_TOKEN=0x... \
//!     cargo run --example transfer
//! ```
//!
//! Signs the EIP-712 `Transfer` payload with the local key, then POSTs
//! `/transfers/send-funds`.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use alloy_primitives::Address;
use anyhow::{Context, Result};
use obsdn_sdk::sign::{scale_decimal_str, sign_transfer, transfer::TransferPayload};
use obsdn_sdk::types::v1::SendFundsRequest;
use obsdn_sdk::{Client, EipSigner, Env, LocalSigner};

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
    let amount = std::env::var("OBSDN_AMOUNT").unwrap_or_else(|_| "1.0".into());

    let signer = Arc::new(LocalSigner::from_hex(&private_key)?);
    let client = Client::builder()
        .env(Env::Staging)
        .api_key(api_key, api_secret)
        .eip_signer(signer.clone())
        .build()?;

    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as u64;
    let payload = TransferPayload {
        from: signer.address(),
        to,
        token,
        amount: scale_decimal_str(&amount)?,
        nonce,
    };
    let sig = sign_transfer(signer.as_ref(), client.eip712_domain(), payload)?;

    let req = SendFundsRequest {
        from: format!("{:#x}", signer.address()),
        to: format!("{to:#x}"),
        tkn: format!("{token:#x}"),
        amt: amount.clone(),
        nonce,
        sig: obsdn_sdk::sign::signature_hex(&sig),
    };
    let resp = client.account().send_funds(req).await?;
    tracing::info!(?resp, "send_funds result");
    Ok(())
}
