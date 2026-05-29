//! WebSocket client.
//!
//! The public [`Session`] is the managed client: a single supervisor task
//! multiplexes every subscription on a shared connection, auto-reconnects
//! on drop, and replays auth + subs. The raw `WsConnection` is retained
//! internally as the supervisor's transport and is not part of the public
//! surface.
//!
//! No gap detection: pulse stamps every frame with `gsn`, a single global
//! event watermark, not a dense per-subscription sequence - channels emit
//! selectively (throttling/filtering), so per-sub GSNs jump arbitrarily and
//! gap inference is meaningless. The server never drops individual messages
//! mid-session (it closes the connection on outbox overflow, which the
//! supervisor handles via reconnect). `update.gsn` is exposed raw for
//! callers who want their own monotonic checks.
//!
//! ## Quick start
//!
//! ```no_run
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! use futures_util::StreamExt;
//! use obsdn_sdk::{Client, Env};
//! use obsdn_sdk::ws::{Channel, Event};
//!
//! let client = Client::builder().env(Env::Production).build()?;
//! let ws = client.ws();
//! let mut stream = ws.subscribe(Channel::Book { market: "BTC-PERP".into() }).await?;
//! while let Some(evt) = stream.next().await {
//!     match evt {
//!         Event::Update(u) => println!("gsn={} kind={:?}", u.gsn, u.kind),
//!         Event::Reconnected => eprintln!("re-attached"),
//!         Event::Unauthorized(msg) => eprintln!("auth replay failed: {msg}"),
//!     }
//! }
//! ws.shutdown().await?;
//! # Ok(()) }
//! ```

mod auth;
mod channel;
mod connection;
mod event;
mod frame;
mod managed;
pub mod views;

pub use channel::{Channel, ChannelName};
pub use event::{Event, Update, UpdateKind};
pub use managed::{Session, SubscriptionStream};
pub use views::{
    Book, CollateralAsset, Oracle, Order, Portfolio, Position, Ticker, TickerLevel, Trade,
};
