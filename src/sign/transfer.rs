//! `Transfer` EIP-712 signer — mirrors
//! `pkg/ethsig/template/transfer.json.tmpl` + `sign_transfer.go`.

use alloy_primitives::Address;
use alloy_sol_types::{sol, Eip712Domain, SolStruct};

use crate::error::Result;
use crate::sign::EipSigner;

sol! {
    /// Spot/sub-account transfer between two addresses on the same chain.
    /// `amount` is `uint128` (matches Go template), with the same 18-decimal
    /// scaling convention as Order.size — see [`crate::sign::scale_decimal_str`].
    #[derive(Debug)]
    struct Transfer {
        address from;
        address to;
        address token;
        uint128 amount;
        uint64 nonce;
    }
}

/// Transfer payload (pre-scaled).
#[derive(Debug, Clone)]
pub struct TransferPayload {
    /// Sender wallet address.
    pub from: Address,
    /// Recipient wallet address.
    pub to: Address,
    /// Token contract address (e.g. USDC).
    pub token: Address,
    /// Amount in 18-decimal fixed-point.
    pub amount: u128,
    /// Anti-replay nonce.
    pub nonce: u64,
}

impl TransferPayload {
    fn into_sol(self) -> Transfer {
        Transfer {
            from: self.from,
            to: self.to,
            token: self.token,
            amount: self.amount,
            nonce: self.nonce,
        }
    }
}

/// Sign a [`TransferPayload`] under the given domain. Returns the 65-byte
/// `r || s || v` signature with `v ∈ {27, 28}`.
pub fn sign_transfer(
    signer: &dyn EipSigner,
    domain: &Eip712Domain,
    payload: TransferPayload,
) -> Result<[u8; 65]> {
    let hash = payload.into_sol().eip712_signing_hash(domain);
    signer.sign_hash_sync(hash)
}
