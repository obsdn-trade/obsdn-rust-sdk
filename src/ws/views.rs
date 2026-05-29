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
    /// CRC32-IEEE of the full book state — validate local book after applying diffs.
    #[serde(default)]
    pub checksum: u32,
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
///
/// Note: trade timestamp is at the frame level — use `WsUpdate.ts`, not a
/// field in this struct.
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

/// User position frame (`position` channel).
///
/// Snapshot is the full position set (JSON array); updates wrap the changed
/// position in a single object. Fields match `pkg/jsonfast/position.go`.
#[derive(Debug, Clone, Deserialize)]
pub struct PositionView {
    /// Market index (uint32).
    #[serde(default)]
    pub mkt_idx: u32,
    /// Market symbol (e.g. `"BTC-PERP"`).
    #[serde(default)]
    pub mkt_id: String,
    /// Net position size (positive = long, negative = short). Decimal string.
    #[serde(default)]
    pub net_sz: String,
    /// Average entry price. Decimal string.
    #[serde(default)]
    pub avg_entry_px: String,
    /// Quote balance. Decimal string.
    #[serde(default)]
    pub quote_bal: String,
    /// Current mark price. Decimal string.
    #[serde(default)]
    pub mark_px: String,
    /// Current index price. Decimal string.
    #[serde(default)]
    pub idx_px: String,
    /// Margin mode enum string (`"MARGIN_MODE_CROSS"` / `"MARGIN_MODE_ISOLATED"`).
    #[serde(default)]
    pub mrgn_mode: String,
    /// Position leverage. Decimal string.
    #[serde(default)]
    pub lev: String,
    /// Margin balance for this position. Decimal string.
    #[serde(default)]
    pub mrgn_bal: String,
    /// Initial margin requirement. Decimal string.
    #[serde(default)]
    pub init_mrgn_req: String,
    /// Maintenance margin requirement. Decimal string.
    #[serde(default)]
    pub maint_mrgn_req: String,
    /// Estimated liquidation price. Decimal string.
    #[serde(default)]
    pub liq_px: String,
    /// Unrealized PnL. Decimal string.
    #[serde(default)]
    pub unrlzd_pnl: String,
    /// Total cumulative funding payments. Decimal string.
    #[serde(default)]
    pub tot_fund_paid: String,
    /// Isolated USDC balance. Decimal string.
    #[serde(default)]
    pub iso_usdc_bal: String,
    /// Free isolated USDC balance. Decimal string.
    #[serde(default)]
    pub free_iso_usdc_bal: String,
    /// Whether position is in isolated liquidation.
    #[serde(default)]
    pub in_iso_liq: bool,
    /// Margin ratio (maintenance margin / margin balance). Decimal string.
    #[serde(default)]
    pub mrgn_ratio: String,
}

/// Collateral asset within a [`PortfolioView`].
#[derive(Debug, Clone, Deserialize)]
pub struct CollateralAssetView {
    /// Token symbol (e.g. `"USDC"`, `"ETH"`).
    #[serde(default)]
    pub asset: String,
    /// Token contract address.
    #[serde(default)]
    pub addr: String,
    /// Token balance. Decimal string.
    #[serde(default)]
    pub bal: String,
    /// Withdrawable amount. Decimal string.
    #[serde(default)]
    pub wdrawable_amt: String,
    /// Market value in USD (before haircut). Decimal string.
    #[serde(default)]
    pub mkt_val_usd: String,
    /// Collateral value in USD (after haircut). Decimal string.
    #[serde(default)]
    pub coll_val_usd: String,
    /// Percentage of total collateral. Decimal string.
    #[serde(default)]
    pub coll_val_comp: String,
}

/// User portfolio frame (`portfolio` channel).
///
/// Both snapshot and update share the same shape — a single portfolio
/// object. Fields match `pkg/jsonfast/portfolio.go`.
#[derive(Debug, Clone, Deserialize)]
pub struct PortfolioView {
    /// Collateral mode enum string (`"COLLATERAL_MODE_USDC"` / `"COLLATERAL_MODE_MULTI"`).
    #[serde(default)]
    pub coll_mode: String,
    /// Total collateral value in USD. Decimal string.
    #[serde(default)]
    pub tot_coll_val: String,
    /// Collateral margin balance. Decimal string.
    #[serde(default)]
    pub coll_mrgn_bal: String,
    /// Cross margin balance. Decimal string.
    #[serde(default)]
    pub cross_mrgn_bal: String,
    /// Cross margin ratio. Decimal string.
    #[serde(default)]
    pub cross_mrgn_ratio: String,
    /// Cross margin usage percentage. Decimal string.
    #[serde(default)]
    pub cross_mrgn_usg: String,
    /// Cross account leverage. Decimal string.
    #[serde(default)]
    pub cross_acct_lev: String,
    /// Free collateral. Decimal string.
    #[serde(default)]
    pub free_coll: String,
    /// Total account value. Decimal string.
    #[serde(default)]
    pub tot_acct_val: String,
    /// Total cross notional value. Decimal string.
    #[serde(default)]
    pub tot_cross_ntnl: String,
    /// Total cross initial margin. Decimal string.
    #[serde(default)]
    pub tot_cross_init_mrgn: String,
    /// Total cross maintenance margin. Decimal string.
    #[serde(default)]
    pub tot_cross_maint_mrgn: String,
    /// Total unrealized PnL. Decimal string.
    #[serde(default)]
    pub tot_unrlzd_pnl: String,
    /// Realized PnL. Decimal string.
    #[serde(default)]
    pub rlzd_pnl: String,
    /// Margin health ratio. Decimal string.
    #[serde(default)]
    pub mrgn_health: String,
    /// Total isolated order reserve. Decimal string.
    #[serde(default)]
    pub tot_iso_ord_rsrv: String,
    /// Whether in cross liquidation.
    #[serde(default)]
    pub in_cross_liq: bool,
    /// Whether there is a pending withdrawal.
    #[serde(default)]
    pub has_pnd_wdraw: bool,
    /// Whether there is a pending stake vault request.
    #[serde(default)]
    pub has_pnd_stake: bool,
    /// Whether there is a pending unstake vault request.
    #[serde(default)]
    pub has_pnd_unstake: bool,
    /// Collateral assets breakdown.
    #[serde(default)]
    pub coll_assets: Vec<CollateralAssetView>,
    /// All open positions.
    #[serde(default)]
    pub pos: Vec<PositionView>,
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

    /// Decode `data` as a list of [`PositionView`].
    ///
    /// The two wire shapes differ: a **snapshot** delivers all positions as
    /// a JSON array (`MarshalProtoSlice`), but a live **update** delivers a
    /// *single* position object (`MarshalProto`) — see
    /// `services/pulse/channel/channel_position.go`. We accept either and
    /// always return a `Vec` so callers don't have to branch on `kind`.
    pub fn as_positions(&self) -> Result<Vec<PositionView>> {
        if self.channel != ChannelName::Position {
            return Err(Error::Decode(serde::de::Error::custom(format!(
                "expected channel {:?}, got {:?}",
                ChannelName::Position,
                self.channel
            ))));
        }
        // Borrow-deserialize from `&Value` (which implements `Deserializer`)
        // instead of `from_value(self.data.clone())` — avoids cloning the
        // whole JSON tree, which matters for large position snapshots.
        match &self.data {
            // Snapshot: array of positions.
            serde_json::Value::Array(_) => {
                Vec::<PositionView>::deserialize(&self.data).map_err(Error::from)
            }
            // Defensive: empty/absent payload → no positions.
            serde_json::Value::Null => Ok(Vec::new()),
            // Update: a single position object.
            _ => {
                let one = PositionView::deserialize(&self.data).map_err(Error::from)?;
                Ok(vec![one])
            }
        }
    }

    /// Decode `data` as a [`PortfolioView`]. Both snapshot and update are
    /// a single portfolio object.
    pub fn as_portfolio(&self) -> Result<PortfolioView> {
        parse_view(self, ChannelName::Portfolio)
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
    fn position_view_roundtrip() {
        let u = fake_update(
            ChannelName::Position,
            json!([{
                "mkt_idx": 1,
                "mkt_id": "BTC-PERP",
                "net_sz": "0.5",
                "avg_entry_px": "43000.00",
                "quote_bal": "21500.00",
                "mark_px": "43100.00",
                "idx_px": "43050.00",
                "mrgn_mode": "MARGIN_MODE_CROSS",
                "lev": "10",
                "mrgn_bal": "2150.00",
                "init_mrgn_req": "215.00",
                "maint_mrgn_req": "107.50",
                "liq_px": "40000.00",
                "unrlzd_pnl": "50.00",
                "tot_fund_paid": "1.23",
                "iso_usdc_bal": "0",
                "free_iso_usdc_bal": "0",
                "in_iso_liq": false,
                "mrgn_ratio": "0.05"
            }]),
        );
        let positions = u.as_positions().unwrap();
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].mkt_id, "BTC-PERP");
        assert_eq!(positions[0].net_sz, "0.5");
        assert_eq!(positions[0].mrgn_mode, "MARGIN_MODE_CROSS");
    }

    #[test]
    fn position_update_single_object_decodes() {
        // A live position UPDATE arrives as a single object, not an array
        // (server uses MarshalProto, not MarshalProtoSlice). Regression
        // test for the silent-decode-failure bug.
        let u = WsUpdate {
            kind: WsUpdateKind::Update,
            channel: ChannelName::Position,
            gsn: 2,
            ts: 0,
            filter: "BTC-PERP".to_string(),
            data: json!({
                "mkt_idx": 1,
                "mkt_id": "BTC-PERP",
                "net_sz": "0.5",
                "avg_entry_px": "43000.00",
                "quote_bal": "21500.00",
                "mark_px": "43100.00",
                "idx_px": "43050.00",
                "mrgn_mode": "MARGIN_MODE_CROSS",
                "lev": "10",
                "mrgn_bal": "2150.00",
                "init_mrgn_req": "215.00",
                "maint_mrgn_req": "107.50",
                "liq_px": "40000.00",
                "unrlzd_pnl": "50.00",
                "tot_fund_paid": "1.23",
                "iso_usdc_bal": "0",
                "free_iso_usdc_bal": "0",
                "in_iso_liq": false,
                "mrgn_ratio": "0.05"
            }),
        };
        let positions = u.as_positions().unwrap();
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].mkt_id, "BTC-PERP");
    }

    #[test]
    fn portfolio_view_roundtrip() {
        let u = fake_update(
            ChannelName::Portfolio,
            json!({
                "coll_mode": "COLLATERAL_MODE_USDC",
                "tot_coll_val": "10000.00",
                "coll_mrgn_bal": "5000.00",
                "cross_mrgn_bal": "5000.00",
                "cross_mrgn_ratio": "0.10",
                "cross_mrgn_usg": "50.00",
                "cross_acct_lev": "2.0",
                "free_coll": "5000.00",
                "tot_acct_val": "10500.00",
                "tot_cross_ntnl": "10000.00",
                "tot_cross_init_mrgn": "500.00",
                "tot_cross_maint_mrgn": "250.00",
                "tot_unrlzd_pnl": "500.00",
                "rlzd_pnl": "100.00",
                "mrgn_health": "95.00",
                "tot_iso_ord_rsrv": "0",
                "in_cross_liq": false,
                "has_pnd_wdraw": false,
                "has_pnd_stake": false,
                "has_pnd_unstake": false,
                "coll_assets": [
                    {"asset": "USDC", "addr": "0xabc", "bal": "10000", "wdrawable_amt": "5000", "mkt_val_usd": "10000", "coll_val_usd": "10000", "coll_val_comp": "100"}
                ],
                "pos": []
            }),
        );
        let portfolio = u.as_portfolio().unwrap();
        assert_eq!(portfolio.coll_mode, "COLLATERAL_MODE_USDC");
        assert_eq!(portfolio.tot_coll_val, "10000.00");
        assert_eq!(portfolio.coll_assets.len(), 1);
        assert_eq!(portfolio.coll_assets[0].asset, "USDC");
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
