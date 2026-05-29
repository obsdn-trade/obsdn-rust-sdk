//! Codegen smoke tests: confirm the generated types deserialize representative
//! REST payloads and re-serialize back to structurally equal JSON.
//!
//! The asymmetry to defend against: the JSON serializer intentionally omits
//! fields holding default values (proto3 semantics). A weak round-trip test
//! (`parse → serialize → parse → assert eq`) cannot detect a silent field
//! drop on parse: the dropped field becomes the type default on parse-1,
//! gets omitted on serialize, and re-parses to the same default on parse-2.
//! Equality holds spuriously.
//!
//! Defense:
//!   1. Fixtures populate every field with a NON-DEFAULT value, so any
//!      field missing from the re-serialized JSON indicates a drop.
//!   2. We compare the raw fixture against `serialize(parse(raw))` as
//!      canonical JSON (recursively sorted keys). All input keys must
//!      survive the round trip.
//!   3. We additionally check parse-then-serialize is idempotent
//!      (`parse_a == parse_b`).

use std::path::PathBuf;

use obsdn_sdk::types::v1::{Order, PlaceOrderRequest};
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn load_fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/json")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read fixture {}: {}", path.display(), e))
}

/// Recursively sort all object keys so two structurally-equal JSON values
/// compare as identical regardless of key ordering. Necessary because
/// `serde_json` preserves insertion order but the serializer and a
/// hand-written fixture won't agree on field declaration order.
fn canonicalize(v: Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().map(|(k, v)| (k, canonicalize(v))).collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().collect())
        }
        Value::Array(arr) => Value::Array(arr.into_iter().map(canonicalize).collect()),
        other => other,
    }
}

/// Strong round-trip:
///   1. raw fixture parses to T (no error).
///   2. serialize(parse(raw)) parses to a Value that, when canonicalized,
///      equals the canonicalized raw fixture. This proves no field was
///      silently dropped on parse, given an all-non-default fixture.
///   3. parse-then-serialize is idempotent under PartialEq.
fn assert_round_trip<T>(fixture: &str)
where
    T: Serialize + for<'de> Deserialize<'de> + std::fmt::Debug + PartialEq,
{
    let raw = load_fixture(fixture);
    let raw_value: Value = serde_json::from_str(&raw).expect("fixture must be valid JSON");

    let parsed_once: T = serde_json::from_str(&raw).expect("first parse from fixture must succeed");
    let serialized =
        serde_json::to_string(&parsed_once).expect("serialize parsed value must succeed");
    let serialized_value: Value =
        serde_json::from_str(&serialized).expect("serialized form must be valid JSON");

    assert_eq!(
        canonicalize(raw_value),
        canonicalize(serialized_value),
        "round-trip dropped or altered fields for {fixture}\nserialized form: {serialized}"
    );

    let parsed_twice: T =
        serde_json::from_str(&serialized).expect("re-parse of serialized value must succeed");
    assert_eq!(
        parsed_once, parsed_twice,
        "parse(serialize(parse(raw))) was not idempotent for {fixture}"
    );
}

#[test]
fn place_order_request_round_trips() {
    assert_round_trip::<PlaceOrderRequest>("place_order_request.json");
}

#[test]
fn order_round_trips() {
    assert_round_trip::<Order>("order.json");
}

/// Confirms enum-as-string JSON behavior: the deserializer must accept both
/// the SCREAMING_SNAKE form ("ORDER_SIDE_BUY") and the integer form (1).
#[test]
fn enum_accepts_both_string_and_int() {
    let as_string = r#"{"mktId":"BTC-PERP","sd":"ORDER_SIDE_BUY","ot":"ORDER_TYPE_MARKET","sz":1.0,"px":0,"tif":"TIME_IN_FORCE_GTC","po":false,"ro":false,"stp":"SELF_TRADE_PREVENTION_CANCEL_TAKER","clOid":"x","nonce":"1","sig":""}"#;
    let as_int = r#"{"mktId":"BTC-PERP","sd":1,"ot":2,"sz":1.0,"px":0,"tif":1,"po":false,"ro":false,"stp":1,"clOid":"x","nonce":"1","sig":""}"#;

    let from_string: PlaceOrderRequest =
        serde_json::from_str(as_string).expect("string-form enum must parse");
    let from_int: PlaceOrderRequest =
        serde_json::from_str(as_int).expect("int-form enum must parse");
    assert_eq!(from_string.sd, from_int.sd);
    assert_eq!(from_string.ot, from_int.ot);
    assert_eq!(from_string.tif, from_int.tif);
}

/// Negative control: prove the strengthened round-trip ACTUALLY catches
/// silent drops. We feed a fixture with a known field, simulate a "drop"
/// by parsing into a stripped-down struct via Value, and confirm the
/// canonical-equality assertion would fail.
#[test]
fn round_trip_assertion_detects_dropped_field() {
    let raw = load_fixture("place_order_request.json");
    let raw_value: Value = serde_json::from_str(&raw).unwrap();

    // Simulate a parser that silently drops `clOid`: produce a serialized
    // JSON missing that key. Canonicalized comparison MUST flag it.
    let mut tampered = raw_value.clone();
    tampered.as_object_mut().unwrap().remove("clOid");

    assert_ne!(
        canonicalize(raw_value),
        canonicalize(tampered),
        "canonical-equality check failed to detect a dropped field - \
         the round-trip assertion is providing false confidence"
    );
}
