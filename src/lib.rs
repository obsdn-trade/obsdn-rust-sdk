//! # OBSDN Rust SDK
//!
//! Async Rust SDK for the OBSDN perpetual exchange. Covers the public
//! REST surface (~50 RPCs across 11 services), EIP-712 signing for
//! orders/transfers/withdrawals, and a managed WebSocket client with
//! auto-reconnect and typed channel views. The WS layer does not do gap
//! detection (`gsn` is a sparse watermark); resync via REST after a
//! reconnect if you need byte-perfect catch-up.
//!
//! ## Quick start - REST
//!
//! ```no_run
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! use obsdn_sdk::{Client, Env};
//!
//! let client = Client::builder()
//!     .env(Env::Production)
//!     .api_key("my-api-key", "my-api-secret")
//!     .build()?;
//!
//! let markets = client.markets().list().await?;
//! println!("{} markets", markets.markets().len());
//! # Ok(()) }
//! ```
//!
//! ## Quick start - place an order
//!
//! ```no_run
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! use std::sync::Arc;
//! use obsdn_sdk::rest::orders::LimitOrder;
//! use obsdn_sdk::types::v1::OrderSide;
//! use obsdn_sdk::{Client, Env, LocalSigner};
//!
//! let signer = Arc::new(LocalSigner::from_hex("0x...")?);
//! let client = Client::builder()
//!     .env(Env::Production)
//!     .api_key("key", "secret")
//!     .eip712_signer(signer)
//!     .build()?;
//!
//! let resp = client
//!     .orders()
//!     .place_limit(LimitOrder::new("BTC-PERP", OrderSide::Buy, "50000", "0.001"))
//!     .await?;
//! # Ok(()) }
//! ```
//!
//! ## Quick start - WebSocket
//!
//! ```no_run
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! use futures_util::StreamExt;
//! use obsdn_sdk::ws::{Channel, Event};
//! use obsdn_sdk::{Client, Env};
//!
//! let client = Client::builder().env(Env::Production).build()?;
//! let ws = client.ws();
//! let mut stream = ws
//!     .subscribe(Channel::Book { market: "BTC-PERP".into() })
//!     .await?;
//! while let Some(evt) = stream.next().await {
//!     if let Event::Update(u) = evt {
//!         let book = u.as_book()?;
//!         println!("{} bids / {} asks", book.bids.len(), book.asks.len());
//!     }
//! }
//! # Ok(()) }
//! ```
//!
//! See `examples/` for end-to-end flows (place_order,
//! ws_book, transfer, withdraw, book_with_resync, ...).

#![forbid(unsafe_code)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]

pub(crate) mod auth;
pub mod builder;
pub mod env;
pub mod error;
pub(crate) mod market_cache;
pub mod rest;
pub mod sign;
pub mod types;
pub mod ws;

pub use builder::{Client, ClientBuilder};
pub use env::Env;
pub use error::{Error, Result};
pub use sign::{Eip712Signer, LocalSigner};
/// Order side. The full [`OrderSide`] name and the short `Side` alias are
/// both re-exported; use whichever reads best at the call site (`Side::Buy` /
/// `Side::Sell`).
pub use types::v1::OrderSide;
pub use types::v1::OrderSide as Side;

/// Common imports for getting started. `use obsdn_sdk::prelude::*;` brings the
/// client, environment, error types, signer, the order-building types, and the
/// core WebSocket types into scope.
pub mod prelude {
    pub use crate::rest::orders::LimitOrder;
    pub use crate::types::v1::{SelfTradePrevention, TimeInForce};
    pub use crate::ws::{Channel, Event, Session, UpdateKind};
    pub use crate::{
        Client, ClientBuilder, Eip712Signer, Env, Error, LocalSigner, OrderSide, Result, Side,
    };
}
