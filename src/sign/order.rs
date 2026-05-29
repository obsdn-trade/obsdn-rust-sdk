//! `Order` EIP-712 signer.
//!
//! Matches the exchange's reference `Order` signer byte-for-byte.
//! Field order is canonical — any change silently breaks matching-engine
//! verification. Golden test `tests/eip712_golden.rs::order_hash_matches_go`
//! is the gate.

use alloy_primitives::Address;
use alloy_sol_types::{sol, Eip712Domain, SolStruct};

use crate::error::Result;
use crate::sign::Eip712Signer;

sol! {
    /// Wire shape for EIP-712 hashing. Field order is canonical.
    /// `side`: `0` = buy, `1` = sell.
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

impl TryFrom<crate::types::v1::OrderSide> for OrderSide {
    type Error = crate::error::Error;

    /// Convert the REST/proto [`Side`](crate::Side) into the signing side.
    /// `Unspecified` has no on-chain representation and returns
    /// [`Error::Sign`](crate::Error::Sign).
    fn try_from(side: crate::types::v1::OrderSide) -> crate::error::Result<Self> {
        use crate::types::v1::OrderSide as ProtoSide;
        match side {
            ProtoSide::Buy => Ok(OrderSide::Buy),
            ProtoSide::Sell => Ok(OrderSide::Sell),
            other => Err(crate::error::Error::Sign(format!(
                "order side must be Buy or Sell, got {other:?}"
            ))),
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
    /// Market index - looked up from the `markets()` endpoint.
    pub market_index: u16,
    /// Buy / sell.
    pub side: OrderSide,
    /// Size in base-asset 18-decimal fixed-point.
    pub size: u128,
    /// Limit price in quote-asset 18-decimal fixed-point.
    pub price: u128,
    /// Anti-replay nonce - typically the current Unix nanosecond timestamp.
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
    signer: &dyn Eip712Signer,
    domain: &Eip712Domain,
    payload: OrderPayload,
) -> Result<[u8; 65]> {
    let hash = payload.into_sol().eip712_signing_hash(domain);
    signer.sign_hash_sync(hash)
}
