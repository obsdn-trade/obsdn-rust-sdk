//! `Withdraw` EIP-712 signer.

use alloy_primitives::Address;
use alloy_sol_types::{sol, Eip712Domain, SolStruct};

use crate::error::Result;
use crate::sign::Eip712Signer;

sol! {
    /// On-chain withdrawal request.
    #[derive(Debug)]
    struct Withdraw {
        address sender;
        address token;
        uint128 amount;
        uint64 nonce;
    }
}

/// Withdraw payload (pre-scaled). `amount` is 18-decimal fixed-point -
/// see [`crate::sign::scale_decimal_str`].
#[derive(Debug, Clone)]
pub struct WithdrawPayload {
    /// Sender wallet - must match the signer's address.
    pub sender: Address,
    /// Token contract address.
    pub token: Address,
    /// Amount in 18-decimal fixed-point.
    pub amount: u128,
    /// Anti-replay nonce.
    pub nonce: u64,
}

impl WithdrawPayload {
    fn into_sol(self) -> Withdraw {
        Withdraw {
            sender: self.sender,
            token: self.token,
            amount: self.amount,
            nonce: self.nonce,
        }
    }
}

/// Sign a [`WithdrawPayload`]. Returns the 65-byte `r || s || v`
/// signature.
pub fn sign_withdraw(
    signer: &dyn Eip712Signer,
    domain: &Eip712Domain,
    payload: WithdrawPayload,
) -> Result<[u8; 65]> {
    let hash = payload.into_sol().eip712_signing_hash(domain);
    signer.sign_hash_sync(hash)
}
