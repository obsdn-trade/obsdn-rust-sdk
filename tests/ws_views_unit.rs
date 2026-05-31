//! Offline decode of every WS view from synthetic JSON.
//!
//! This is the wire-compatibility guard for the readable field names: the
//! structs use human names (`size`, `price`, `market_id`, ...) while the JSON
//! keys stay abbreviated (`sz`, `px`, `mkt_id`, ...) via `#[serde(rename)]`.
//! Asserting concrete values catches any missing rename (which would silently
//! deserialize to a default).

use obsdn_sdk::ws::{ChannelName, Update, UpdateKind};
use serde_json::json;

fn upd(channel: ChannelName, data: serde_json::Value) -> Update {
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
fn order_view_maps_every_wire_key() {
    let u = upd(
        ChannelName::Order,
        json!([{
            "oid": "o1", "mkt_id": "BTC-PERP", "sd": "ORDER_SIDE_BUY",
            "ot": "ORDER_TYPE_LIMIT", "sz": "1.5", "px": "100.25", "sndr": "0xabc",
            "nonce": "7", "stp": "SELF_TRADE_PREVENTION_UNSPECIFIED", "po": true,
            "tif": "TIME_IN_FORCE_GTC", "ro": false, "st": "ORDER_STATUS_OPEN",
            "done_rsn": "", "filled_sz": "0.5", "avg_px": "100.0", "tot_fees": "0.01",
            "crt_ts": "111", "upd_ts": "222", "cl_oid": "c1", "cancel_req": false
        }]),
    );
    let o = &u.as_orders().expect("orders decode")[0];
    assert_eq!(o.oid, "o1");
    assert_eq!(o.market_id, "BTC-PERP");
    assert_eq!(o.side, "ORDER_SIDE_BUY");
    assert_eq!(o.order_type, "ORDER_TYPE_LIMIT");
    assert_eq!(o.size, "1.5");
    assert_eq!(o.price, "100.25");
    assert_eq!(o.sender, "0xabc");
    assert_eq!(o.self_trade_prevention, "SELF_TRADE_PREVENTION_UNSPECIFIED");
    assert!(o.post_only);
    assert_eq!(o.time_in_force, "TIME_IN_FORCE_GTC");
    assert!(!o.reduce_only);
    assert_eq!(o.status, "ORDER_STATUS_OPEN");
    assert_eq!(o.filled_size, "0.5");
    assert_eq!(o.avg_price, "100.0");
    assert_eq!(o.total_fees, "0.01");
    assert_eq!(o.created_at, "111");
    assert_eq!(o.updated_at, "222");
    assert_eq!(o.client_order_id, "c1");
    assert!(!o.cancel_requested);
}

#[test]
fn position_view_maps_every_wire_key() {
    let u = upd(
        ChannelName::Position,
        json!([{
            "mkt_idx": 2, "mkt_id": "ETH-PERP", "net_sz": "-1.0", "avg_entry_px": "2000",
            "quote_bal": "100", "mark_px": "2010", "idx_px": "2005",
            "mrgn_mode": "MARGIN_MODE_CROSS", "lev": "5", "mrgn_bal": "400",
            "init_mrgn_req": "40", "maint_mrgn_req": "20", "liq_px": "2500",
            "unrlzd_pnl": "-10", "tot_fund_paid": "0.5", "iso_usdc_bal": "0",
            "free_iso_usdc_bal": "0", "in_iso_liq": false, "mrgn_ratio": "0.05"
        }]),
    );
    let p = &u.as_positions().expect("positions decode")[0];
    assert_eq!(p.market_index, 2);
    assert_eq!(p.market_id, "ETH-PERP");
    assert_eq!(p.net_size, "-1.0");
    assert_eq!(p.avg_entry_price, "2000");
    assert_eq!(p.quote_balance, "100");
    assert_eq!(p.mark_price, "2010");
    assert_eq!(p.index_price, "2005");
    assert_eq!(p.margin_mode, "MARGIN_MODE_CROSS");
    assert_eq!(p.leverage, "5");
    assert_eq!(p.margin_balance, "400");
    assert_eq!(p.initial_margin_req, "40");
    assert_eq!(p.maintenance_margin_req, "20");
    assert_eq!(p.liquidation_price, "2500");
    assert_eq!(p.unrealized_pnl, "-10");
    assert_eq!(p.total_funding_paid, "0.5");
    assert!(!p.in_isolated_liquidation);
    assert_eq!(p.margin_ratio, "0.05");
}

#[test]
fn portfolio_view_maps_every_wire_key() {
    let u = upd(
        ChannelName::Portfolio,
        json!({
            "coll_mode": "COLLATERAL_MODE_USDC", "tot_coll_val": "1000",
            "coll_mrgn_bal": "500", "cross_mrgn_bal": "500", "cross_mrgn_ratio": "0.1",
            "cross_mrgn_usg": "50", "cross_acct_lev": "2", "free_coll": "500",
            "tot_acct_val": "1050", "tot_cross_ntnl": "1000", "tot_cross_init_mrgn": "50",
            "tot_cross_maint_mrgn": "25", "tot_unrlzd_pnl": "50", "rlzd_pnl": "10",
            "mrgn_health": "95", "tot_iso_ord_rsrv": "0", "in_cross_liq": false,
            "has_pnd_wdraw": true, "has_pnd_stake": false, "has_pnd_unstake": false,
            "coll_assets": [{
                "asset": "USDC", "addr": "0xa", "bal": "1000", "wdrawable_amt": "500",
                "mkt_val_usd": "1000", "coll_val_usd": "1000", "coll_val_comp": "100"
            }],
            "pos": []
        }),
    );
    let pf = u.as_portfolio().expect("portfolio decode");
    assert_eq!(pf.collateral_mode, "COLLATERAL_MODE_USDC");
    assert_eq!(pf.total_collateral_value, "1000");
    assert_eq!(pf.collateral_margin_balance, "500");
    assert_eq!(pf.cross_margin_balance, "500");
    assert_eq!(pf.cross_margin_ratio, "0.1");
    assert_eq!(pf.cross_margin_usage, "50");
    assert_eq!(pf.cross_account_leverage, "2");
    assert_eq!(pf.free_collateral, "500");
    assert_eq!(pf.total_account_value, "1050");
    assert_eq!(pf.total_cross_notional, "1000");
    assert_eq!(pf.total_cross_initial_margin, "50");
    assert_eq!(pf.total_cross_maintenance_margin, "25");
    assert_eq!(pf.total_unrealized_pnl, "50");
    assert_eq!(pf.realized_pnl, "10");
    assert_eq!(pf.margin_health, "95");
    assert_eq!(pf.total_isolated_order_reserve, "0");
    assert!(pf.has_pending_withdrawal);
    assert!(!pf.has_pending_stake);
    assert!(!pf.has_pending_unstake);
    let ca = &pf.collateral_assets[0];
    assert_eq!(ca.address, "0xa");
    assert_eq!(ca.balance, "1000");
    assert_eq!(ca.withdrawable_amount, "500");
    assert_eq!(ca.market_value_usd, "1000");
    assert_eq!(ca.collateral_value_usd, "1000");
    assert_eq!(ca.collateral_value_pct, "100");
}

#[test]
fn book_ticker_oracle_trade_decode() {
    let b = upd(
        ChannelName::Book,
        json!({"bids": [["100", "1"]], "asks": [["101", "2"]], "checksum": 42}),
    )
    .as_book()
    .expect("book");
    assert_eq!(b.bids[0], ["100".to_string(), "1".to_string()]);
    assert_eq!(b.checksum, 42);

    let t = upd(
        ChannelName::Ticker,
        json!({"bid": {"px": "100", "sz": "1"}, "ask": {"px": "101", "sz": "2"}}),
    )
    .as_ticker()
    .expect("ticker");
    assert_eq!(t.bid.price, "100");
    assert_eq!(t.ask.size, "2");

    let o = upd(
        ChannelName::Oracle,
        json!({"asset": "BTC", "mark_px": "50000", "idx_px": "49999", "mark_px_ts": "1", "idx_px_ts": "2"}),
    )
    .as_oracle()
    .expect("oracle");
    assert_eq!(o.asset, "BTC");
    assert_eq!(o.mark_price, "50000");
    assert_eq!(o.index_price, "49999");
    assert_eq!(o.mark_price_ts, "1");
    assert_eq!(o.index_price_ts, "2");

    let tr = upd(
        ChannelName::Trade,
        json!({"id": "t1", "mkr_sd": "ORDER_SIDE_BUY", "px": "100", "sz": "0.5", "quote_sz": "50"}),
    )
    .as_trade()
    .expect("trade");
    assert_eq!(tr.id, "t1");
    assert_eq!(tr.maker_side, "ORDER_SIDE_BUY");
    assert_eq!(tr.price, "100");
    assert_eq!(tr.size, "0.5");
    assert_eq!(tr.quote_size, "50");
}

#[test]
fn notification_view_decodes_type_and_payload() {
    let u = upd(
        ChannelName::Notification,
        json!({
            "notification_type": "deposit.confirmed",
            "timestamp": 1700000000,
            "payload": { "asset": "USDC", "amount": "1000", "tx_hash": "0xabc" }
        }),
    );
    let n = u.as_notification().expect("notification decode");
    assert_eq!(n.notification_type, "deposit.confirmed");
    assert_eq!(n.timestamp, 1700000000);
    assert_eq!(n.payload["asset"], "USDC");
    assert_eq!(n.payload["amount"], "1000");
}

#[test]
fn notification_view_tolerates_missing_optional_fields() {
    // timestamp/payload are optional on the wire.
    let u = upd(
        ChannelName::Notification,
        json!({ "notification_type": "withdrawal.failed" }),
    );
    let n = u.as_notification().expect("decode");
    assert_eq!(n.notification_type, "withdrawal.failed");
    assert_eq!(n.timestamp, 0);
    assert!(n.payload.is_null());
}

#[test]
fn wrong_channel_is_rejected() {
    let u = upd(ChannelName::Book, json!({"bids": [], "asks": []}));
    assert!(u.as_ticker().is_err());
    assert!(u.as_notification().is_err());
}
