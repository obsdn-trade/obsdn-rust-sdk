//! Per-environment EIP-712 domain values.
//!
//! Mirrors the chain configs each environment's backend loads:
//! - **Staging** → monad-testnet (`configs/shared/chain/monad_testnet.yaml`)
//! - **Production** → monad-mainnet (`configs/shared/chain/monad_mainnet.yaml`)
//!
//! Other targets (a forked staging stack, an internal host, a local backend)
//! go through [`custom_domain`] paired with [`crate::Env::Custom`].

use alloy_primitives::{address, Address};
use alloy_sol_types::{eip712_domain, Eip712Domain};

use crate::env::Env;

/// Standard SDK domain - every template family in this crate uses this same
/// value. Go uses one `config.Domain` for all signers (orders, transfers,
/// withdrawals, vaults, registers); we keep that 1:1.
///
/// **Staging** → monad-testnet. **Production** → monad-mainnet. Each must
/// match the chain config the corresponding backend loads, or every
/// signature is rejected.
pub fn sdk_domain(env: &Env) -> Eip712Domain {
    match env {
        Env::Staging => staging_domain(),
        Env::Production => production_domain(),
        Env::Custom { .. } => panic!(
            "sdk_domain() cannot determine the correct domain for Env::Custom - \
             use custom_domain() or ClientBuilder::eip712_domain() instead"
        ),
    }
}

/// Backwards-compatible alias for callers that thought of the domain as
/// "the order domain" - every template currently shares the same domain.
pub fn order_domain(env: &Env) -> Eip712Domain {
    sdk_domain(env)
}

/// Build a fully-custom domain. Useful when targeting a non-public chain or
/// a forked staging contract.
pub fn custom_domain(name: &str, version: &str, chain_id: u64, contract: Address) -> Eip712Domain {
    eip712_domain! {
        name: name.to_string(),
        version: version.to_string(),
        chain_id: chain_id,
        verifying_contract: contract,
    }
}

/// Canonical staging / Monad-testnet domain.
fn staging_domain() -> Eip712Domain {
    eip712_domain! {
        name: "Obsidian",
        version: "1",
        chain_id: 10143u64,
        verifying_contract: address!("B95aE40b700FDBb0906b8Dc2AeBBDd94848325Df"),
    }
}

/// Production domain - Monad mainnet (chain 143).
/// Values at time of writing; canonical source: `GET /chain/config`.
/// These rarely change - any change will be announced publicly.
fn production_domain() -> Eip712Domain {
    eip712_domain! {
        name: "Obsidian",
        version: "1",
        chain_id: 143u64,
        verifying_contract: address!("90c3747cd4E6bC6FbebB1b3C54D99737590eBE45"),
    }
}
