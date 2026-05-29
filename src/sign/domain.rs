//! Per-environment EIP-712 domain values.
//!
//! - **Staging** → Monad testnet (chain 10143)
//! - **Production** → Monad mainnet (chain 143)
//!
//! For any other target (forked stack, custom chain) use [`custom_domain`]
//! with [`crate::Env::Custom`].

use alloy_primitives::{address, Address};
use alloy_sol_types::{eip712_domain, Eip712Domain};

use crate::env::Env;

/// Returns the EIP-712 domain for the given environment.
///
/// All signer families (orders, transfers, withdrawals, vaults, registers)
/// share a single domain. Panics if called with [`Env::Custom`] — use
/// [`custom_domain`] or `ClientBuilder::eip712_domain()` instead.
pub fn default_eip712_domain(env: &Env) -> Eip712Domain {
    match env {
        Env::Staging => staging_domain(),
        Env::Production => production_domain(),
        Env::Custom { .. } => panic!(
            "default_eip712_domain() cannot determine the correct domain for Env::Custom - \
             use custom_domain() or ClientBuilder::eip712_domain() instead"
        ),
    }
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
/// Canonical source: `GET /chain/config`. Changes are announced publicly.
fn production_domain() -> Eip712Domain {
    eip712_domain! {
        name: "Obsidian",
        version: "1",
        chain_id: 143u64,
        verifying_contract: address!("90c3747cd4E6bC6FbebB1b3C54D99737590eBE45"),
    }
}
