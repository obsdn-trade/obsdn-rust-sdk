//! Unit coverage for the typed `Market` / `Order` accessors and
//! `GetMarketsResponse::markets`.

use obsdn_sdk::types::v1::{
    GetMarketsResponse, Market, Order, OrderSide, OrderStatus, OrderType, TimeInForce,
};

#[test]
fn typed_accessors_parse_decimal_strings() {
    let m = Market {
        mark_px: "50000.5".into(),
        idx_px: "49999.0".into(),
        last_px: "50001".into(),
        min_sz: "0.001".into(),
        base_incr: "0.0001".into(),
        price_incr: "0.01".into(),
        max_lev: "50".into(),
        ..Default::default()
    };
    assert_eq!(m.mark_price(), Some(50000.5));
    assert_eq!(m.index_price(), Some(49999.0));
    assert_eq!(m.last_price(), Some(50001.0));
    assert_eq!(m.min_size(), Some(0.001));
    assert_eq!(m.base_increment(), Some(0.0001));
    assert_eq!(m.price_increment(), Some(0.01));
    assert_eq!(m.max_leverage(), Some(50.0));
}

#[test]
fn accessors_return_none_on_unparseable() {
    let m = Market {
        mark_px: String::new(),
        idx_px: "n/a".into(),
        ..Default::default()
    };
    assert_eq!(m.mark_price(), None);
    assert_eq!(m.index_price(), None);
}

#[test]
fn order_typed_accessors_map_raw_i32_to_enums() {
    let o = Order {
        sd: OrderSide::Buy as i32,
        ot: OrderType::Limit as i32,
        tif: TimeInForce::Gtc as i32,
        st: OrderStatus::Open as i32,
        ..Default::default()
    };
    // Each accessor must read its own wire field (sd/ot/tif/st), not another.
    assert_eq!(o.side(), Some(OrderSide::Buy));
    assert_eq!(o.order_type(), Some(OrderType::Limit));
    assert_eq!(o.time_in_force(), Some(TimeInForce::Gtc));
    assert_eq!(o.status(), Some(OrderStatus::Open));
}

#[test]
fn order_accessors_return_none_on_unknown_discriminant() {
    // A wire value outside the known enum range (e.g. a newer server variant)
    // must surface as None, not a panic or a wrong variant.
    let o = Order {
        sd: 9999,
        ot: -1,
        ..Default::default()
    };
    assert_eq!(o.side(), None);
    assert_eq!(o.order_type(), None);
}

#[test]
fn markets_accessor_exposes_inner_slice() {
    let resp = GetMarketsResponse {
        mkts: vec![Market::default(), Market::default()],
    };
    assert_eq!(resp.markets().len(), 2);
}
