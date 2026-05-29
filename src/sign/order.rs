//! `Order` EIP-712 signer.
//!
//! Mirrors `pkg/ethsig/template/order.json.tmpl` and `sign_order.go`.
//! The struct field order is canonical - changing it will silently break
//! verification at the matching engine. Golden test
//! `tests/eip712_golden.rs::order_hash_matches_go` is the gate.

use alloy_primitives::Address;
use alloy_sol_types::{sol, Eip712Domain, SolStruct};

use crate::error::Result;
use crate::sign::EipSigner;

sol! {
    /// Wire shape encoded for EIP-712 hashing. Field order MUST match
    /// `pkg/ethsig/template/order.json.tmpl`. `side` is `0` for buy,
    /// `1` for sell - matches `pkg/ethsig/verify_order.go::computeOrderStructHash`.
    #[derive(Debug)]
    struct Order {
        address sender;
        uint16 marketIndex;
        uint8 side;
        uint128 size;
        uint128 price;
        uint64 nonce;
    }
}

/// Order side - encoded as `uint8` (`Buy = 0`, `Sell = 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderSide {
    /// Buy / long.
    Buy,
    /// Sell / short.
    Sell,
}

impl OrderSide {
    fn as_u8(self) -> u8 {
        match self {
            OrderSide::Buy => 0,
            OrderSide::Sell => 1,
        }
    }
}

/// User-facing order payload - already pre-scaled. Use
/// [`crate::sign::scale_decimal_str`] to convert REST decimal strings to
/// the `u128` representation hashed on-chain.
#[derive(Debug, Clone)]
pub struct OrderPayload {
    /// Sender address (the trader's main wallet).
    pub sender: Address,
    /// Market index - looked up from `markets()`/proto `Market.idx`.
    pub market_index: u16,
    /// Buy / sell.
    pub side: OrderSide,
    /// Size in base-asset 18-decimal fixed-point.
    pub size: u128,
    /// Limit price in quote-asset 18-decimal fixed-point.
    pub price: u128,
    /// Anti-replay nonce - typically `time.Now().UnixNano()`.
    pub nonce: u64,
}

impl OrderPayload {
    fn into_sol(self) -> Order {
        Order {
            sender: self.sender,
            marketIndex: self.market_index,
            side: self.side.as_u8(),
            size: self.size,
            price: self.price,
            nonce: self.nonce,
        }
    }
}

/// Compute the EIP-712 signing hash without signing - useful for offline
/// verification, audit logging, or batched signing flows.
pub fn order_signing_hash(domain: &Eip712Domain, payload: OrderPayload) -> [u8; 32] {
    payload.into_sol().eip712_signing_hash(domain).0
}

/// Sign an [`OrderPayload`] under the given domain. Returns the 65-byte
/// `r || s || v` signature with `v ∈ {27, 28}` - wire-ready for
/// `PlaceOrderRequest.sig`.
pub fn sign_order(
    signer: &dyn EipSigner,
    domain: &Eip712Domain,
    payload: OrderPayload,
) -> Result<[u8; 65]> {
    let hash = payload.into_sol().eip712_signing_hash(domain);
    signer.sign_hash_sync(hash)
}
