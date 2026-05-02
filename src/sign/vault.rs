//! Vault EIP-712 signers — `CreateVault`, `StakeVault`, `UnstakeVault`.
//!
//! Mirrors templates `create_vault.json.tmpl`, `stake_vault.json.tmpl`,
//! `unstake_vault.json.tmpl` + their `sign_*.go` counterparts.
//!
//! Note the type asymmetry vs Order/Transfer/Withdraw: vault `amount` is
//! `uint256` in the templates (not `uint128`) — accept `U256` directly so
//! callers can hash values that overflow `u128`.

use alloy_primitives::{Address, U256};
use alloy_sol_types::{sol, Eip712Domain, SolStruct};

use crate::error::Result;
use crate::sign::EipSigner;

sol! {
    /// Create a profit-sharing vault.
    #[derive(Debug)]
    struct CreateVault {
        address main;
        address vault;
        uint256 profitShareBps;
    }
    /// Stake into a vault.
    #[derive(Debug)]
    struct StakeVault {
        address vault;
        address staker;
        address token;
        uint256 amount;
        uint64 nonce;
    }
    /// Withdraw stake from a vault.
    #[derive(Debug)]
    struct UnstakeVault {
        address vault;
        address staker;
        address token;
        uint256 amount;
        uint64 nonce;
    }
}

/// `CreateVault` payload. `profit_share_bps` is `uint256` per template; we
/// expose `U256` so callers can pass arbitrary values without truncation.
#[derive(Debug, Clone)]
pub struct CreateVaultPayload {
    /// Main wallet that owns the vault.
    pub main: Address,
    /// Vault sub-account address.
    pub vault: Address,
    /// Profit share in basis points (1 bps = 0.01%).
    pub profit_share_bps: U256,
}

impl CreateVaultPayload {
    fn into_sol(self) -> CreateVault {
        CreateVault {
            main: self.main,
            vault: self.vault,
            profitShareBps: self.profit_share_bps,
        }
    }
}

/// Sign a [`CreateVaultPayload`].
pub fn sign_create_vault(
    signer: &dyn EipSigner,
    domain: &Eip712Domain,
    payload: CreateVaultPayload,
) -> Result<[u8; 65]> {
    let hash = payload.into_sol().eip712_signing_hash(domain);
    signer.sign_hash_sync(hash)
}

/// `StakeVault` payload — stake tokens into a vault.
#[derive(Debug, Clone)]
pub struct StakeVaultPayload {
    /// Vault address receiving the stake.
    pub vault: Address,
    /// Staker wallet.
    pub staker: Address,
    /// Token contract.
    pub token: Address,
    /// Amount in 18-decimal fixed-point.
    pub amount: U256,
    /// Anti-replay nonce.
    pub nonce: u64,
}

impl StakeVaultPayload {
    fn into_sol(self) -> StakeVault {
        StakeVault {
            vault: self.vault,
            staker: self.staker,
            token: self.token,
            amount: self.amount,
            nonce: self.nonce,
        }
    }
}

/// Sign a [`StakeVaultPayload`].
pub fn sign_stake_vault(
    signer: &dyn EipSigner,
    domain: &Eip712Domain,
    payload: StakeVaultPayload,
) -> Result<[u8; 65]> {
    let hash = payload.into_sol().eip712_signing_hash(domain);
    signer.sign_hash_sync(hash)
}

/// `UnstakeVault` payload — withdraw stake from a vault.
#[derive(Debug, Clone)]
pub struct UnstakeVaultPayload {
    /// Vault address.
    pub vault: Address,
    /// Staker wallet.
    pub staker: Address,
    /// Token contract.
    pub token: Address,
    /// Amount in 18-decimal fixed-point.
    pub amount: U256,
    /// Anti-replay nonce.
    pub nonce: u64,
}

impl UnstakeVaultPayload {
    fn into_sol(self) -> UnstakeVault {
        UnstakeVault {
            vault: self.vault,
            staker: self.staker,
            token: self.token,
            amount: self.amount,
            nonce: self.nonce,
        }
    }
}

/// Sign an [`UnstakeVaultPayload`].
pub fn sign_unstake_vault(
    signer: &dyn EipSigner,
    domain: &Eip712Domain,
    payload: UnstakeVaultPayload,
) -> Result<[u8; 65]> {
    let hash = payload.into_sol().eip712_signing_hash(domain);
    signer.sign_hash_sync(hash)
}
