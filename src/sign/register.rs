//! API-key registration EIP-712 signers.
//!
//! `Register` is signed by the sender (main wallet); `DelegatedSigner` is
//! signed by the new signer key to prove ownership. Both signatures are
//! required by the `RegisterSigner` endpoint.

use alloy_primitives::Address;
use alloy_sol_types::{sol, Eip712Domain, SolStruct};

use crate::error::Result;
use crate::sign::Eip712Signer;

sol! {
    /// Sender authorizes a new signer key. primaryType: `Register`.
    #[derive(Debug)]
    struct Register {
        address sender;
        address signer;
        string message;
        uint64 nonce;
    }

    /// Signer proves ownership by signing the main account address.
    /// primaryType: `DelegatedSigner`.
    #[derive(Debug)]
    struct DelegatedSigner {
        address account;
    }
}

/// `Register` payload - signed by the **sender** (main wallet) to
/// authorize a new signer key.
#[derive(Debug, Clone)]
pub struct RegisterPayload {
    /// Sender address (the main wallet authorizing the signer).
    pub sender: Address,
    /// Signing key being authorized.
    pub signer: Address,
    /// Human-readable consent message.
    pub message: String,
    /// Anti-replay nonce.
    pub nonce: u64,
}

impl RegisterPayload {
    fn into_sol(self) -> Register {
        Register {
            sender: self.sender,
            signer: self.signer,
            message: self.message,
            nonce: self.nonce,
        }
    }
}

/// Sign a [`RegisterPayload`] (sender side).
pub fn sign_register(
    signer: &dyn Eip712Signer,
    domain: &Eip712Domain,
    payload: RegisterPayload,
) -> Result<[u8; 65]> {
    let hash = payload.into_sol().eip712_signing_hash(domain);
    signer.sign_hash_sync(hash)
}

/// `DelegatedSigner` payload - signed by the **signer key** to prove
/// ownership of the main account it's being authorized for.
#[derive(Debug, Clone)]
pub struct DelegatedSignerPayload {
    /// Main account being claimed.
    pub account: Address,
}

impl DelegatedSignerPayload {
    fn into_sol(self) -> DelegatedSigner {
        DelegatedSigner {
            account: self.account,
        }
    }
}

/// Sign a [`DelegatedSignerPayload`] (signer-key side).
pub fn sign_delegated_signer(
    signer: &dyn Eip712Signer,
    domain: &Eip712Domain,
    payload: DelegatedSignerPayload,
) -> Result<[u8; 65]> {
    let hash = payload.into_sol().eip712_signing_hash(domain);
    signer.sign_hash_sync(hash)
}
