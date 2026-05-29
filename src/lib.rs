//! # OBSDN Rust SDK
//!
//! Async Rust SDK for the OBSDN perpetual exchange. Covers the public
//! REST surface (~50 RPCs across 11 services), EIP-712 signing for
//! orders/transfers/withdrawals, and a managed WebSocket client with
//! auto-reconnect, GSN gap detection, and typed channel views.
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
//! let markets = client.markets().get_markets().await?;
//! println!("{} markets", markets.mkts.len());
//! # Ok(()) }
//! ```
//!
//! ## Quick start - place an order
//!
//! ```no_run
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! use std::sync::Arc;
//! use obsdn_sdk::rest::orders::PlaceEasy;
//! use obsdn_sdk::types::v1::OrderSide;
//! use obsdn_sdk::{Client, Env, LocalSigner};
//!
//! let signer = Arc::new(LocalSigner::from_hex("0x...")?);
//! let client = Client::builder()
//!     .env(Env::Production)
//!     .api_key("key", "secret")
//!     .eip_signer(signer)
//!     .build()?;
//!
//! let resp = client
//!     .orders()
//!     .place_easy(PlaceEasy::limit("BTC-PERP", OrderSide::Buy, 50_000.0, 0.001))
//!     .await?;
//! # Ok(()) }
//! ```
//!
//! ## Quick start - WebSocket
//!
//! ```no_run
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! use futures_util::StreamExt;
//! use obsdn_sdk::ws::{Channel, WsEvent};
//! use obsdn_sdk::{Client, Env};
//!
//! let client = Client::builder().env(Env::Production).build()?;
//! let ws = client.ws();
//! let mut stream = ws
//!     .subscribe(Channel::Book { market: "BTC-PERP".into() })
//!     .await?;
//! while let Some(evt) = stream.next().await {
//!     if let WsEvent::Update(u) = evt {
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

pub mod auth;
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
pub use sign::{EipSigner, LocalSigner};
