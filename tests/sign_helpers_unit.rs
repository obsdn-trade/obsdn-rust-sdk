//! Unit coverage for the public signing helpers and domain constants.

use obsdn_sdk::sign::{default_eip712_domain, scale_f64, OrderSide as SignSide};
use obsdn_sdk::{Env, Side};

#[test]
fn scale_f64_matches_fixed_point() {
    assert_eq!(scale_f64(0.0).unwrap(), 0);
    assert_eq!(scale_f64(1.0).unwrap(), 1_000_000_000_000_000_000);
    assert_eq!(scale_f64(1.5).unwrap(), 1_500_000_000_000_000_000);
}

#[test]
fn scale_f64_rejects_non_finite_and_negative() {
    assert!(scale_f64(-1.0).is_err());
    assert!(scale_f64(f64::NAN).is_err());
    assert!(scale_f64(f64::INFINITY).is_err());
}

#[test]
fn production_domain_constants() {
    let d = default_eip712_domain(&Env::Production).expect("production domain");
    assert_eq!(d.name.as_deref(), Some("Obsidian"));
    assert_eq!(d.version.as_deref(), Some("1"));
    assert_eq!(d.chain_id.unwrap().to_string(), "143");
    assert_eq!(
        d.verifying_contract.unwrap().to_string().to_lowercase(),
        "0x90c3747cd4e6bc6fbebb1b3c54d99737590ebe45"
    );
}

#[test]
fn staging_domain_constants() {
    let d = default_eip712_domain(&Env::Staging).expect("staging domain");
    assert_eq!(d.chain_id.unwrap().to_string(), "10143");
    assert_eq!(
        d.verifying_contract.unwrap().to_string().to_lowercase(),
        "0xb95ae40b700fdbb0906b8dc2aebbdd94848325df"
    );
}

#[test]
fn order_side_conversion_unifies_the_two_enums() {
    assert!(matches!(SignSide::try_from(Side::Buy), Ok(SignSide::Buy)));
    assert!(matches!(SignSide::try_from(Side::Sell), Ok(SignSide::Sell)));
    assert!(SignSide::try_from(Side::Unspecified).is_err());
}
