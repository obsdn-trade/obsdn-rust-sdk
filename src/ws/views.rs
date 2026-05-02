//! Typed views over [`super::WsUpdate::data`].
//!
//! Pulse emits `data` as channel-specific JSON. The thin/managed clients
//! keep `data` as `serde_json::Value` to stay schema-agnostic; this module
//! adds opt-in typed deserializers for the channels callers actually
//! consume in tight loops.
//!
//! All numeric fields stay as `String` — pulse encodes prices/sizes as
//! decimal strings (no f64 round-trip risk) and the EIP-712 signing
//! pipeline already handles the `String → u128` scaling. Use
//! [`crate::sign::scale_decimal_str`] when you need the integer form.
//!
//! Schema source: `docs/api/ws-integration.md` (book, ticker, oracle,
//! trade, order). Forward-compatible: unknown fields are ignored.

use serde::Deserialize;

use crate::error::{Error, Result};

use super::event::WsUpdate;
use super::ChannelName;

/// Read `update.data` into a typed view, validating the channel matches.
fn parse_view<T: for<'de> Deserialize<'de>>(update: &WsUpdate, expected: ChannelName) -> Result<T> {
    if update.channel != expected {
        return Err(Error::Decode(serde::de::Error::custom(format!(
            "expected channel {:?}, got {:?}",
            expected, update.channel
        ))));
    }
    serde_json::from_value(update.data.clone()).map_err(Error::from)
}

/// Order-book frame (`book` channel).
///
/// Snapshot and update share the same shape. Distinguish via
/// `update.kind` — snapshots replace state, updates patch it (`size = "0"`
/// removes the level). See `docs/api/ws-integration.md#order-book-maintenance`
/// for the maintenance algorithm.
#[derive(Debug, Clone, Deserialize)]
pub struct BookView {
    /// `[price, size]` rows. Snapshot: descending by price.
    #[serde(default)]
    pub bids: Vec<[String; 2]>,
    /// `[price, size]` rows. Snapshot: ascending by price.
    #[serde(default)]
    pub asks: Vec<[String; 2]>,
}

/// Best bid/ask frame (`ticker` channel).
#[derive(Debug, Clone, Deserialize)]
pub struct TickerView {
    /// Best bid level.
    pub bid: TickerLevel,
    /// Best ask level.
    pub ask: TickerLevel,
}

/// One side of a [`TickerView`] (best bid OR best ask).
#[derive(Debug, Clone, Deserialize)]
pub struct TickerLevel {
    /// Price as decimal string.
    pub px: String,
    /// Size at that price as decimal string.
    pub sz: String,
}

/// Oracle price frame (`oracle` channel).
#[derive(Debug, Clone, Deserialize)]
pub struct OracleView {
    /// Asset symbol (`"BTC"`, `"ETH"`, ...).
    pub asset: String,
    /// Mark price — used for margin/liquidation. Decimal string.
    pub mark_px: String,
    /// Index price — spot reference. Decimal string.
    pub idx_px: String,
    /// Mark price timestamp (nanoseconds, JSON string).
    pub mark_px_ts: String,
    /// Index price timestamp (nanoseconds, JSON string).
    pub idx_px_ts: String,
}

/// Public trade execution (`trade` channel — update only, no snapshot).
#[derive(Debug, Clone, Deserialize)]
pub struct TradeView {
    /// Unique trade id.
    pub id: String,
    /// Maker side string (`"ORDER_SIDE_BUY"` / `"ORDER_SIDE_SELL"`).
    pub mkr_sd: String,
    /// Execution price as decimal string.
    pub px: String,
    /// Trade size in base asset as decimal string.
    pub sz: String,
    /// Trade size in quote asset as decimal string.
    pub quote_sz: String,
}

/// User order frame (`order` channel — payload is an array of orders).
///
/// Snapshot is the open-orders set; updates wrap the changed orders in an
/// array. Most fields use string-encoded numbers and enum strings — see
/// `docs/api/ws-integration.md#order--user-orders-private` for the full
/// catalog. Unknown fields are ignored to allow server additions.
#[derive(Debug, Clone, Deserialize)]
pub struct OrderView {
    /// Order id (UUID).
    pub oid: String,
    /// Market symbol (e.g. `"BTC-PERP"`).
    pub mkt_id: String,
    /// `"ORDER_SIDE_BUY"` / `"ORDER_SIDE_SELL"`.
    pub sd: String,
    /// `"ORDER_TYPE_LIMIT"` / `..._MARKET` / `..._STOP` / `..._TWAP`.
    pub ot: String,
    /// Order size (base asset, decimal string).
    pub sz: String,
    /// Limit price (quote asset, decimal string).
    pub px: String,
    /// Sender wallet (`0x...`).
    pub sndr: String,
    /// Decimal string of u64.
    pub nonce: String,
    /// Self-trade prevention enum string.
    pub stp: String,
    /// Post-only flag.
    pub po: bool,
    /// Time-in-force enum string.
    pub tif: String,
    /// Reduce-only flag.
    pub ro: bool,
    /// `"ORDER_STATUS_OPEN"` / `..._DONE` / etc.
    pub st: String,
    /// Completion reason (`"filled"`, `"canceled"`, ...). Empty until done.
    #[serde(default)]
    pub done_rsn: String,
    /// Filled size so far.
    #[serde(default)]
    pub filled_sz: String,
    /// Average fill price.
    #[serde(default)]
    pub avg_px: String,
    /// Total fees paid (USD, decimal string).
    #[serde(default)]
    pub tot_fees: String,
    /// Created-at nanoseconds (JSON string).
    #[serde(default)]
    pub crt_ts: String,
    /// Last-updated-at nanoseconds (JSON string).
    #[serde(default)]
    pub upd_ts: String,
    /// Caller-assigned client id (empty if none).
    #[serde(default)]
    pub cl_oid: String,
    /// `true` once a cancel was requested.
    #[serde(default)]
    pub cancel_req: bool,
}

impl WsUpdate {
    /// Decode `data` as a [`BookView`]. Returns `Error::Decode` on shape
    /// mismatch, or if the update is for a different channel.
    pub fn as_book(&self) -> Result<BookView> {
        parse_view(self, ChannelName::Book)
    }

    /// Decode `data` as a [`TickerView`].
    pub fn as_ticker(&self) -> Result<TickerView> {
        parse_view(self, ChannelName::Ticker)
    }

    /// Decode `data` as an [`OracleView`].
    pub fn as_oracle(&self) -> Result<OracleView> {
        parse_view(self, ChannelName::Oracle)
    }

    /// Decode `data` as a [`TradeView`].
    pub fn as_trade(&self) -> Result<TradeView> {
        parse_view(self, ChannelName::Trade)
    }

    /// Decode `data` as a list of [`OrderView`]. Server wraps both
    /// snapshot and update payloads in a JSON array.
    pub fn as_orders(&self) -> Result<Vec<OrderView>> {
        parse_view(self, ChannelName::Order)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ws::event::WsUpdateKind;
    use serde_json::json;

    fn fake_update(channel: ChannelName, data: serde_json::Value) -> WsUpdate {
        WsUpdate {
            kind: WsUpdateKind::Snapshot,
            channel,
            gsn: 1,
            ts: 0,
            filter: String::new(),
            data,
        }
    }

    #[test]
    fn book_view_roundtrip() {
        let u = fake_update(
            ChannelName::Book,
            json!({
                "bids": [["43250.00", "1.500"], ["43249.00", "2.000"]],
                "asks": [["43251.00", "0.800"]]
            }),
        );
        let book = u.as_book().unwrap();
        assert_eq!(book.bids.len(), 2);
        assert_eq!(book.asks[0][0], "43251.00");
    }

    #[test]
    fn ticker_view_roundtrip() {
        let u = fake_update(
            ChannelName::Ticker,
            json!({
                "bid": {"px": "43250.00", "sz": "1.5"},
                "ask": {"px": "43251.00", "sz": "0.8"}
            }),
        );
        let t = u.as_ticker().unwrap();
        assert_eq!(t.bid.px, "43250.00");
        assert_eq!(t.ask.sz, "0.8");
    }

    #[test]
    fn oracle_view_roundtrip() {
        let u = fake_update(
            ChannelName::Oracle,
            json!({
                "asset": "BTC",
                "mark_px": "43250.50",
                "idx_px": "43248.10",
                "mark_px_ts": "1700000000000000000",
                "idx_px_ts": "1700000000000000000"
            }),
        );
        let o = u.as_oracle().unwrap();
        assert_eq!(o.asset, "BTC");
    }

    #[test]
    fn channel_mismatch_errors() {
        let u = fake_update(ChannelName::Book, json!({"bids": [], "asks": []}));
        let err = u.as_ticker().unwrap_err();
        assert!(matches!(err, Error::Decode(_)));
    }

    #[test]
    fn order_view_unknown_fields_ignored() {
        let u = fake_update(
            ChannelName::Order,
            json!([
                {
                    "oid": "abc",
                    "mkt_id": "BTC-PERP",
                    "sd": "ORDER_SIDE_BUY",
                    "ot": "ORDER_TYPE_LIMIT",
                    "sz": "1.0",
                    "px": "43000.00",
                    "sndr": "0xabc",
                    "nonce": "1",
                    "stp": "SELF_TRADE_PREVENTION_UNSPECIFIED",
                    "po": false,
                    "tif": "TIME_IN_FORCE_GTC",
                    "ro": false,
                    "st": "ORDER_STATUS_OPEN",
                    "future_field_we_dont_know": 42
                }
            ]),
        );
        let orders = u.as_orders().unwrap();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].oid, "abc");
    }
}
