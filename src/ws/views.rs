//! Typed views over [`super::Update::data`].
//!
//! Pulse emits `data` as channel-specific JSON. The managed client keeps
//! `data` as `serde_json::Value` to stay schema-agnostic; this module adds
//! opt-in typed deserializers for the channels callers consume in tight loops.
//!
//! All numeric fields stay as `String` - pulse encodes prices/sizes as
//! decimal strings (no f64 round-trip risk) and the EIP-712 signing
//! pipeline already handles the `String → u128` scaling. Use
//! [`crate::sign::scale_decimal_str`] when you need the integer form.
//! Unknown fields are ignored for forward-compatibility.

use serde::Deserialize;

use crate::error::{Error, Result};

use super::event::Update;
use super::ChannelName;

/// Read `update.data` into a typed view, validating the channel matches.
fn parse_view<T: for<'de> Deserialize<'de>>(update: &Update, expected: ChannelName) -> Result<T> {
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
/// `update.kind` - snapshots replace state, updates patch it (`size = "0"`
/// removes the level). See `docs/api/ws-integration.md#order-book-maintenance`
/// for the maintenance algorithm.
#[derive(Debug, Clone, Deserialize)]
pub struct Book {
    /// `[price, size]` rows. Snapshot: descending by price.
    #[serde(default)]
    pub bids: Vec<[String; 2]>,
    /// `[price, size]` rows. Snapshot: ascending by price.
    #[serde(default)]
    pub asks: Vec<[String; 2]>,
    /// CRC32-IEEE of the full book state - validate local book after applying diffs.
    #[serde(default)]
    pub checksum: u32,
}

/// Best bid/ask frame (`ticker` channel).
#[derive(Debug, Clone, Deserialize)]
pub struct Ticker {
    /// Best bid level.
    pub bid: TickerLevel,
    /// Best ask level.
    pub ask: TickerLevel,
}

/// One side of a [`Ticker`] (best bid OR best ask).
#[derive(Debug, Clone, Deserialize)]
pub struct TickerLevel {
    /// Price as decimal string.
    #[serde(rename = "px")]
    pub price: String,
    /// Size at that price as decimal string.
    #[serde(rename = "sz")]
    pub size: String,
}

/// Oracle price frame (`oracle` channel).
#[derive(Debug, Clone, Deserialize)]
pub struct Oracle {
    /// Asset symbol (`"BTC"`, `"ETH"`, ...).
    pub asset: String,
    /// Mark price - used for margin/liquidation. Decimal string.
    #[serde(rename = "mark_px")]
    pub mark_price: String,
    /// Index price - spot reference. Decimal string.
    #[serde(rename = "idx_px")]
    pub index_price: String,
    /// Mark price timestamp (nanoseconds, JSON string).
    #[serde(rename = "mark_px_ts")]
    pub mark_price_ts: String,
    /// Index price timestamp (nanoseconds, JSON string).
    #[serde(rename = "idx_px_ts")]
    pub index_price_ts: String,
}

/// Public trade execution (`trade` channel - update only, no snapshot).
///
/// Note: trade timestamp is at the frame level - use `Update.ts`, not a
/// field in this struct.
#[derive(Debug, Clone, Deserialize)]
pub struct Trade {
    /// Unique trade id.
    pub id: String,
    /// Maker side string (`"ORDER_SIDE_BUY"` / `"ORDER_SIDE_SELL"`).
    #[serde(rename = "mkr_sd")]
    pub maker_side: String,
    /// Execution price as decimal string.
    #[serde(rename = "px")]
    pub price: String,
    /// Trade size in base asset as decimal string.
    #[serde(rename = "sz")]
    pub size: String,
    /// Trade size in quote asset as decimal string.
    #[serde(rename = "quote_sz")]
    pub quote_size: String,
}

/// User position frame (`position` channel).
///
/// Snapshot is the full position set (JSON array); updates wrap the changed
/// position in a single object.
#[derive(Debug, Clone, Deserialize)]
pub struct Position {
    /// Market index (uint32).
    #[serde(default)]
    #[serde(rename = "mkt_idx")]
    pub market_index: u32,
    /// Market symbol (e.g. `"BTC-PERP"`).
    #[serde(default)]
    #[serde(rename = "mkt_id")]
    pub market_id: String,
    /// Net position size (positive = long, negative = short). Decimal string.
    #[serde(default)]
    #[serde(rename = "net_sz")]
    pub net_size: String,
    /// Average entry price. Decimal string.
    #[serde(default)]
    #[serde(rename = "avg_entry_px")]
    pub avg_entry_price: String,
    /// Quote balance. Decimal string.
    #[serde(default)]
    #[serde(rename = "quote_bal")]
    pub quote_balance: String,
    /// Current mark price. Decimal string.
    #[serde(default)]
    #[serde(rename = "mark_px")]
    pub mark_price: String,
    /// Current index price. Decimal string.
    #[serde(default)]
    #[serde(rename = "idx_px")]
    pub index_price: String,
    /// Margin mode enum string (`"MARGIN_MODE_CROSS"` / `"MARGIN_MODE_ISOLATED"`).
    #[serde(default)]
    #[serde(rename = "mrgn_mode")]
    pub margin_mode: String,
    /// Position leverage. Decimal string.
    #[serde(default)]
    #[serde(rename = "lev")]
    pub leverage: String,
    /// Margin balance for this position. Decimal string.
    #[serde(default)]
    #[serde(rename = "mrgn_bal")]
    pub margin_balance: String,
    /// Initial margin requirement. Decimal string.
    #[serde(default)]
    #[serde(rename = "init_mrgn_req")]
    pub initial_margin_req: String,
    /// Maintenance margin requirement. Decimal string.
    #[serde(default)]
    #[serde(rename = "maint_mrgn_req")]
    pub maintenance_margin_req: String,
    /// Estimated liquidation price. Decimal string.
    #[serde(default)]
    #[serde(rename = "liq_px")]
    pub liquidation_price: String,
    /// Unrealized PnL. Decimal string.
    #[serde(default)]
    #[serde(rename = "unrlzd_pnl")]
    pub unrealized_pnl: String,
    /// Cumulative funding fee (positive = paid, negative = received). Decimal string.
    #[serde(default)]
    #[serde(rename = "fund_fee")]
    pub funding_fee: String,
    /// Isolated USDC balance. Decimal string.
    #[serde(default)]
    #[serde(rename = "iso_usdc_bal")]
    pub isolated_usdc_balance: String,
    /// Free isolated USDC balance. Decimal string.
    #[serde(default)]
    #[serde(rename = "free_iso_usdc_bal")]
    pub free_isolated_usdc_balance: String,
    /// Whether position is in isolated liquidation.
    #[serde(default)]
    #[serde(rename = "in_iso_liq")]
    pub in_isolated_liquidation: bool,
    /// Margin ratio (maintenance margin / margin balance). Decimal string.
    #[serde(default)]
    #[serde(rename = "mrgn_ratio")]
    pub margin_ratio: String,
}

/// Collateral asset within a [`Portfolio`].
#[derive(Debug, Clone, Deserialize)]
pub struct CollateralAsset {
    /// Token symbol (e.g. `"USDC"`, `"ETH"`).
    #[serde(default)]
    pub asset: String,
    /// Token contract address.
    #[serde(default)]
    #[serde(rename = "addr")]
    pub address: String,
    /// Token balance. Decimal string.
    #[serde(default)]
    #[serde(rename = "bal")]
    pub balance: String,
    /// Withdrawable amount. Decimal string.
    #[serde(default)]
    #[serde(rename = "wdrawable_amt")]
    pub withdrawable_amount: String,
    /// Market value in USD (before haircut). Decimal string.
    #[serde(default)]
    #[serde(rename = "mkt_val_usd")]
    pub market_value_usd: String,
    /// Collateral value in USD (after haircut). Decimal string.
    #[serde(default)]
    #[serde(rename = "coll_val_usd")]
    pub collateral_value_usd: String,
    /// Percentage of total collateral. Decimal string.
    #[serde(default)]
    #[serde(rename = "coll_val_comp")]
    pub collateral_value_pct: String,
}

/// User portfolio frame (`portfolio` channel).
///
/// Both snapshot and update deliver a single portfolio object.
#[derive(Debug, Clone, Deserialize)]
pub struct Portfolio {
    /// Collateral mode enum string (`"COLLATERAL_MODE_USDC"` / `"COLLATERAL_MODE_MULTI"`).
    #[serde(default)]
    #[serde(rename = "coll_mode")]
    pub collateral_mode: String,
    /// Total collateral value in USD. Decimal string.
    #[serde(default)]
    #[serde(rename = "tot_coll_val")]
    pub total_collateral_value: String,
    /// Collateral margin balance. Decimal string.
    #[serde(default)]
    #[serde(rename = "coll_mrgn_bal")]
    pub collateral_margin_balance: String,
    /// Cross margin balance. Decimal string.
    #[serde(default)]
    #[serde(rename = "cross_mrgn_bal")]
    pub cross_margin_balance: String,
    /// Cross margin ratio. Decimal string.
    #[serde(default)]
    #[serde(rename = "cross_mrgn_ratio")]
    pub cross_margin_ratio: String,
    /// Cross margin usage percentage. Decimal string.
    #[serde(default)]
    #[serde(rename = "cross_mrgn_usg")]
    pub cross_margin_usage: String,
    /// Cross account leverage. Decimal string.
    #[serde(default)]
    #[serde(rename = "cross_acct_lev")]
    pub cross_account_leverage: String,
    /// Free collateral. Decimal string.
    #[serde(default)]
    #[serde(rename = "free_coll")]
    pub free_collateral: String,
    /// Total account value. Decimal string.
    #[serde(default)]
    #[serde(rename = "tot_acct_val")]
    pub total_account_value: String,
    /// Total cross notional value. Decimal string.
    #[serde(default)]
    #[serde(rename = "tot_cross_ntnl")]
    pub total_cross_notional: String,
    /// Total cross initial margin. Decimal string.
    #[serde(default)]
    #[serde(rename = "tot_cross_init_mrgn")]
    pub total_cross_initial_margin: String,
    /// Total cross maintenance margin. Decimal string.
    #[serde(default)]
    #[serde(rename = "tot_cross_maint_mrgn")]
    pub total_cross_maintenance_margin: String,
    /// Total unrealized PnL. Decimal string.
    #[serde(default)]
    #[serde(rename = "tot_unrlzd_pnl")]
    pub total_unrealized_pnl: String,
    /// Realized PnL. Decimal string.
    #[serde(default)]
    #[serde(rename = "rlzd_pnl")]
    pub realized_pnl: String,
    /// Margin health ratio. Decimal string.
    #[serde(default)]
    #[serde(rename = "mrgn_health")]
    pub margin_health: String,
    /// Total isolated order reserve. Decimal string.
    #[serde(default)]
    #[serde(rename = "tot_iso_ord_rsrv")]
    pub total_isolated_order_reserve: String,
    /// Whether in cross liquidation.
    #[serde(default)]
    #[serde(rename = "in_cross_liq")]
    pub in_cross_liquidation: bool,
    /// Whether there is a pending withdrawal.
    #[serde(default)]
    #[serde(rename = "has_pnd_wdraw")]
    pub has_pending_withdrawal: bool,
    /// Whether there is a pending stake vault request.
    #[serde(default)]
    #[serde(rename = "has_pnd_stake")]
    pub has_pending_stake: bool,
    /// Whether there is a pending unstake vault request.
    #[serde(default)]
    #[serde(rename = "has_pnd_unstake")]
    pub has_pending_unstake: bool,
    /// Collateral assets breakdown.
    #[serde(default)]
    #[serde(rename = "coll_assets")]
    pub collateral_assets: Vec<CollateralAsset>,
    /// All open positions.
    #[serde(default)]
    #[serde(rename = "pos")]
    pub positions: Vec<Position>,
}

/// User order frame (`order` channel - payload is an array of orders).
///
/// Snapshot is the open-orders set; updates wrap the changed orders in an
/// array. Most fields use string-encoded numbers and enum strings.
/// Unknown fields are ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct Order {
    /// Order id (UUID).
    pub oid: String,
    /// Market symbol (e.g. `"BTC-PERP"`).
    #[serde(rename = "mkt_id")]
    pub market_id: String,
    /// `"ORDER_SIDE_BUY"` / `"ORDER_SIDE_SELL"`.
    #[serde(rename = "sd")]
    pub side: String,
    /// `"ORDER_TYPE_LIMIT"` / `..._MARKET` / `..._STOP` / `..._TWAP`.
    #[serde(rename = "ot")]
    pub order_type: String,
    /// Order size (base asset, decimal string).
    #[serde(rename = "sz")]
    pub size: String,
    /// Limit price (quote asset, decimal string).
    #[serde(rename = "px")]
    pub price: String,
    /// Sender wallet (`0x...`).
    #[serde(rename = "sndr")]
    pub sender: String,
    /// Decimal string of u64.
    pub nonce: String,
    /// Self-trade prevention enum string.
    #[serde(rename = "stp")]
    pub self_trade_prevention: String,
    /// Post-only flag.
    #[serde(rename = "po")]
    pub post_only: bool,
    /// Time-in-force enum string.
    #[serde(rename = "tif")]
    pub time_in_force: String,
    /// Reduce-only flag.
    #[serde(rename = "ro")]
    pub reduce_only: bool,
    /// `"ORDER_STATUS_OPEN"` / `..._DONE` / etc.
    #[serde(rename = "st")]
    pub status: String,
    /// Completion reason (`"filled"`, `"canceled"`, ...). Empty until done.
    #[serde(default)]
    #[serde(rename = "done_rsn")]
    pub done_reason: String,
    /// Filled size so far.
    #[serde(default)]
    #[serde(rename = "filled_sz")]
    pub filled_size: String,
    /// Average fill price.
    #[serde(default)]
    #[serde(rename = "avg_px")]
    pub avg_price: String,
    /// Total fees paid (USD, decimal string).
    #[serde(default)]
    #[serde(rename = "tot_fees")]
    pub total_fees: String,
    /// Created-at nanoseconds (JSON string).
    #[serde(default)]
    #[serde(rename = "crt_ts")]
    pub created_at: String,
    /// Last-updated-at nanoseconds (JSON string).
    #[serde(default)]
    #[serde(rename = "upd_ts")]
    pub updated_at: String,
    /// Caller-assigned client id (empty if none).
    #[serde(default)]
    #[serde(rename = "cl_oid")]
    pub client_order_id: String,
    /// `true` once a cancel was requested.
    #[serde(default)]
    #[serde(rename = "cancel_req")]
    pub cancel_requested: bool,
}

/// Account notification frame (`notification` channel): deposit/withdrawal and
/// subaccount lifecycle alerts a market maker uses for collateral management.
///
/// The `payload` shape depends on `notification_type` (e.g. `"deposit.confirmed"`,
/// `"withdrawal.completed"`, `"subaccount.created"`), so it is left as raw
/// `serde_json::Value` for the caller to decode by type.
#[derive(Debug, Clone, Deserialize)]
pub struct Notification {
    /// Event kind, e.g. `"deposit.confirmed"`, `"withdrawal.failed"`,
    /// `"subaccount.created"`.
    pub notification_type: String,
    /// Server timestamp (nanoseconds, as a JSON string, matching the other
    /// timestamp fields like `mark_price_ts`). Empty if absent.
    #[serde(default)]
    pub timestamp: String,
    /// Type-specific payload (deposit/withdrawal/subaccount fields). Decode
    /// per `notification_type`. `Null` if absent.
    #[serde(default)]
    pub payload: serde_json::Value,
}

impl Update {
    /// Decode `data` as a [`Book`]. Returns `Error::Decode` on shape
    /// mismatch, or if the update is for a different channel.
    pub fn as_book(&self) -> Result<Book> {
        parse_view(self, ChannelName::Book)
    }

    /// Decode `data` as a [`Ticker`].
    pub fn as_ticker(&self) -> Result<Ticker> {
        parse_view(self, ChannelName::Ticker)
    }

    /// Decode `data` as an [`Oracle`].
    pub fn as_oracle(&self) -> Result<Oracle> {
        parse_view(self, ChannelName::Oracle)
    }

    /// Decode `data` as a [`Trade`].
    pub fn as_trade(&self) -> Result<Trade> {
        parse_view(self, ChannelName::Trade)
    }

    /// Decode `data` as a list of [`Order`]. Server wraps both
    /// snapshot and update payloads in a JSON array.
    pub fn as_orders(&self) -> Result<Vec<Order>> {
        parse_view(self, ChannelName::Order)
    }

    /// Decode `data` as a list of [`Position`].
    ///
    /// The two wire shapes differ: a **snapshot** delivers all positions as
    /// a JSON array, but a live **update** delivers a *single* position
    /// object. We accept either and always return a `Vec` so callers don't
    /// have to branch on `kind`.
    pub fn as_positions(&self) -> Result<Vec<Position>> {
        if self.channel != ChannelName::Position {
            return Err(Error::Decode(serde::de::Error::custom(format!(
                "expected channel {:?}, got {:?}",
                ChannelName::Position,
                self.channel
            ))));
        }
        match &self.data {
            // Snapshot: array of positions.
            serde_json::Value::Array(_) => {
                serde_json::from_value(self.data.clone()).map_err(Error::from)
            }
            // Defensive: empty/absent payload → no positions.
            serde_json::Value::Null => Ok(Vec::new()),
            // Update: a single position object.
            _ => {
                let one: Position = serde_json::from_value(self.data.clone())?;
                Ok(vec![one])
            }
        }
    }

    /// Decode `data` as a [`Portfolio`]. Both snapshot and update are
    /// a single portfolio object.
    pub fn as_portfolio(&self) -> Result<Portfolio> {
        parse_view(self, ChannelName::Portfolio)
    }

    /// Decode `data` as a [`Notification`] (deposit/withdrawal/subaccount
    /// alert). Returns `Error::Decode` on channel mismatch.
    pub fn as_notification(&self) -> Result<Notification> {
        parse_view(self, ChannelName::Notification)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ws::event::UpdateKind;
    use serde_json::json;

    fn fake_update(channel: ChannelName, data: serde_json::Value) -> Update {
        Update {
            kind: UpdateKind::Snapshot,
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
        assert_eq!(t.bid.price, "43250.00");
        assert_eq!(t.ask.size, "0.8");
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
        assert_eq!(positions[0].market_id, "BTC-PERP");
        assert_eq!(positions[0].net_size, "0.5");
        assert_eq!(positions[0].margin_mode, "MARGIN_MODE_CROSS");
    }

    #[test]
    fn position_update_single_object_decodes() {
        // A live position UPDATE arrives as a single object, not an array.
        // Regression test for the silent-decode-failure bug.
        let u = Update {
            kind: UpdateKind::Update,
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
        assert_eq!(positions[0].market_id, "BTC-PERP");
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
        assert_eq!(portfolio.collateral_mode, "COLLATERAL_MODE_USDC");
        assert_eq!(portfolio.total_collateral_value, "10000.00");
        assert_eq!(portfolio.collateral_assets.len(), 1);
        assert_eq!(portfolio.collateral_assets[0].asset, "USDC");
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
