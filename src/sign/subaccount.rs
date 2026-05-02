//! Subaccount-related EIP-712 signers — `CreateSubaccount` and
//! `RegisterChildAccountSigner`.

use alloy_primitives::Address;
use alloy_sol_types::{sol, Eip712Domain, SolStruct};

use crate::error::Result;
use crate::sign::EipSigner;

sol! {
    /// Authorize a new subaccount under a main address.
    #[derive(Debug)]
    struct CreateSubaccount {
        address main;
        address subaccount;
    }

    /// Authorize an additional signer key for a child account.
    /// Mirrors `register_child_account_signer.json.tmpl`.
    #[derive(Debug)]
    struct RegisterChildAccountSigner {
        address main;
        address childAccount;
        address signer;
        string message;
        uint64 nonce;
    }
}

/// `CreateSubaccount` payload — authorize a new subaccount address under
/// a main wallet.
#[derive(Debug, Clone)]
pub struct CreateSubaccountPayload {
    /// Main wallet that owns the subaccount.
    pub main: Address,
    /// New subaccount address being authorized.
    pub subaccount: Address,
}

impl CreateSubaccountPayload {
    fn into_sol(self) -> CreateSubaccount {
        CreateSubaccount {
            main: self.main,
            subaccount: self.subaccount,
        }
    }
}

/// Sign a [`CreateSubaccountPayload`].
pub fn sign_create_subaccount(
    signer: &dyn EipSigner,
    domain: &Eip712Domain,
    payload: CreateSubaccountPayload,
) -> Result<[u8; 65]> {
    let hash = payload.into_sol().eip712_signing_hash(domain);
    signer.sign_hash_sync(hash)
}

/// `RegisterChildAccountSigner` payload — authorize an extra signing key
/// for a child account.
#[derive(Debug, Clone)]
pub struct RegisterChildAccountSignerPayload {
    /// Main wallet.
    pub main: Address,
    /// Child account being granted a new signer.
    pub child_account: Address,
    /// New signing key to authorize.
    pub signer: Address,
    /// Human-readable consent message (server enforces specific text).
    pub message: String,
    /// Anti-replay nonce.
    pub nonce: u64,
}

impl RegisterChildAccountSignerPayload {
    fn into_sol(self) -> RegisterChildAccountSigner {
        RegisterChildAccountSigner {
            main: self.main,
            childAccount: self.child_account,
            signer: self.signer,
            message: self.message,
            nonce: self.nonce,
        }
    }
}

/// Sign a [`RegisterChildAccountSignerPayload`].
pub fn sign_register_child_account_signer(
    signer: &dyn EipSigner,
    domain: &Eip712Domain,
    payload: RegisterChildAccountSignerPayload,
) -> Result<[u8; 65]> {
    let hash = payload.into_sol().eip712_signing_hash(domain);
    signer.sign_hash_sync(hash)
}
