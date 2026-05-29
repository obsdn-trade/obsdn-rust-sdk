//! Unit coverage for the typed `Market` accessors.

use obsdn_sdk::types::v1::Market;

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
