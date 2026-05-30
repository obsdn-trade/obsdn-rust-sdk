//! EIP-712 signers - byte-equal to the exchange's reference implementation.
//!
//! | Module        | Template family                                  |
//! |---------------|--------------------------------------------------|
//! | [`order`]     | `Order`                                          |
//! | [`transfer`]  | `Transfer`                                       |
//! | [`withdraw`]  | `Withdraw`                                       |
//! | [`vault`]     | `CreateVault`, `StakeVault`, `UnstakeVault`      |
//! | [`subaccount`]| `CreateSubaccount`, `RegisterChildAccountSigner` |
//! | [`register`]  | `Register`, `DelegatedSigner`                    |
//!
//! Correctness gate: `tests/eip712_golden.rs` loads JSON fixtures and asserts
//! the Rust hash matches byte-for-byte. A subtle type-encoding bug would
//! silently reject orders at the matching engine.

pub mod domain;
pub mod order;
pub mod register;
pub mod scale;
pub mod subaccount;
pub mod transfer;
pub mod vault;
pub mod withdraw;

pub use domain::{custom_domain, default_eip712_domain};
pub use order::{order_signing_hash, sign_order, OrderPayload, OrderSide};
pub use register::{sign_delegated_signer, sign_register, DelegatedSignerPayload, RegisterPayload};
pub use scale::{scale_decimal_str, scale_f64};
pub use subaccount::{
    sign_create_subaccount, sign_register_child_account_signer, CreateSubaccountPayload,
    RegisterChildAccountSignerPayload,
};
pub use transfer::{sign_transfer, TransferPayload};
pub use vault::{
    sign_create_vault, sign_stake_vault, sign_unstake_vault, CreateVaultPayload, StakeVaultPayload,
    UnstakeVaultPayload,
};
pub use withdraw::{sign_withdraw, WithdrawPayload};

use alloy_primitives::{Address, B256};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;

use zeroize::Zeroize;

use crate::error::{Error, Result};

/// Object-safe abstraction over an EIP-712 signer.
///
/// `sign_hash_sync` takes the EIP-712 *signing hash*
/// (`keccak256(0x1901 || domainSeparator || structHash)`) and returns a
/// 65-byte `r || s || v` signature with `v ∈ {27, 28}` - the wire format
/// expected by the gateway in `req.sig`.
pub trait Eip712Signer: Send + Sync {
    /// Public address derived from the signer's key.
    fn address(&self) -> Address;

    /// Sign the 32-byte EIP-712 digest, returning a 65-byte
    /// `r || s || v` signature with `v ∈ {27, 28}`.
    fn sign_hash_sync(&self, hash: B256) -> Result<[u8; 65]>;
}

/// Default [`Eip712Signer`] backed by a local secp256k1 key (`alloy_signer_local`).
///
/// Wraps [`PrivateKeySigner`]: exposes the derived address and normalizes the
/// recovery id to `{27, 28}`. The underlying `SecretKey` is zeroed on drop.
#[derive(Clone)]
pub struct LocalSigner {
    inner: PrivateKeySigner,
}

impl LocalSigner {
    /// Build from a 32-byte secp256k1 secret. Returns an error if the bytes
    /// are not a valid scalar (zero or >= curve order).
    pub fn from_bytes(secret: &[u8; 32]) -> Result<Self> {
        let inner = PrivateKeySigner::from_slice(secret)
            .map_err(|e| Error::Sign(format!("invalid secret key: {e}")))?;
        Ok(Self { inner })
    }

    /// Build from a hex-encoded private key (`0x`-prefixed or bare).
    ///
    /// The intermediate decoded bytes are zeroized internally, but this
    /// function cannot zeroize `hex_str` itself (it is caller-owned). If the
    /// key text came from a `String` or similar, zeroize that source after
    /// this call, or load raw key bytes through [`Self::from_bytes`] from a
    /// `Zeroizing<[u8; 32]>` to avoid a plaintext copy entirely.
    pub fn from_hex(hex_str: &str) -> Result<Self> {
        let stripped = hex_str.strip_prefix("0x").unwrap_or(hex_str);
        let mut bytes = hex::decode(stripped)
            .map_err(|e| Error::Sign(format!("invalid hex private key: {e}")))?;
        if bytes.len() != 32 {
            bytes.zeroize();
            return Err(Error::Sign(format!(
                "private key must be 32 bytes, got {}",
                bytes.len()
            )));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        bytes.zeroize();
        let result = Self::from_bytes(&arr);
        arr.zeroize();
        result
    }

    /// Borrow the underlying alloy signer (e.g., to attach a chain id).
    pub fn inner(&self) -> &PrivateKeySigner {
        &self.inner
    }
}

impl std::fmt::Debug for LocalSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalSigner")
            .field("address", &self.inner.address())
            .field("secret", &"<redacted>")
            .finish()
    }
}

impl Eip712Signer for LocalSigner {
    fn address(&self) -> Address {
        self.inner.address()
    }

    fn sign_hash_sync(&self, hash: B256) -> Result<[u8; 65]> {
        // alloy returns parity ∈ {0, 1}; gateway expects v ∈ {27, 28}.
        let sig = SignerSync::sign_hash_sync(&self.inner, &hash)
            .map_err(|e| Error::Sign(format!("local signer: {e}")))?;
        let mut out = [0u8; 65];
        out[..32].copy_from_slice(&sig.r().to_be_bytes::<32>());
        out[32..64].copy_from_slice(&sig.s().to_be_bytes::<32>());
        out[64] = if sig.v() { 28 } else { 27 };
        Ok(out)
    }
}

/// Format a 65-byte signature as a `0x`-prefixed lowercase hex string,
/// ready to submit as `PlaceOrderRequest.sig` or any equivalent field.
pub fn signature_hex(sig: &[u8; 65]) -> String {
    let mut s = String::with_capacity(2 + 130);
    s.push_str("0x");
    s.push_str(&hex::encode(sig));
    s
}
