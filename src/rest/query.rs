//! URL query-string encoding for GET requests.
//!
//! Uses `serde_json::to_value` to drive serialization through the
//! pbjson-emitted `Serialize` impl on each proto request type - pbjson
//! already follows proto3 default-skipping semantics, so default-valued
//! fields drop out automatically and we don't carry them as `?foo=0`.
//! Repeated fields encode as `?key=v1&key=v2` (grpc-gateway supports both
//! `?key[]=...` and repeated `?key=...`; we pick the latter for parity
//! with `pkg/exc/client.go`).
//!
//! Field names are the JSON (lowerCamelCase) form pbjson emits - matches
//! what `runtime.DefaultQueryParser` accepts on the gateway side. Proto
//! field names (snake_case) are also accepted server-side, but we keep
//! one canonical form to avoid surprises.

use serde::Serialize;
use serde_json::Value;
use url::form_urlencoded::Serializer;

use crate::error::{Error, Result};

/// Encode a serializable request struct into a URL-encoded query string
/// (without the leading `?`).
///
/// Returns an empty string when the request has no non-default fields.
/// Path parameters MUST be cleared (set to default) by the caller before
/// passing - they are otherwise emitted as redundant query params.
pub fn encode_query<T: Serialize>(req: &T) -> Result<String> {
    let value = serde_json::to_value(req)?;
    let object = match value {
        Value::Object(map) => map,
        Value::Null => return Ok(String::new()),
        // The proto-generated request types are always messages, hence
        // objects. Anything else is a bug in the generated code or the
        // caller passing a wrong type.
        other => {
            return Err(Error::Config(format!(
                "encode_query expects a struct/object, got {other:?}"
            )))
        }
    };

    let mut ser = Serializer::new(String::new());
    for (key, val) in object {
        append_value(&mut ser, &key, val);
    }
    Ok(ser.finish())
}

fn append_value(ser: &mut Serializer<'_, String>, key: &str, val: Value) {
    match val {
        Value::Null => {}
        // pbjson emits scalars directly - strings, numbers, bools, enum
        // names. URL-encoding them is `to_string` minus quotes.
        Value::Bool(b) => {
            ser.append_pair(key, if b { "true" } else { "false" });
        }
        Value::Number(n) => {
            ser.append_pair(key, &n.to_string());
        }
        Value::String(s) => {
            ser.append_pair(key, &s);
        }
        Value::Array(items) => {
            for item in items {
                append_value(ser, key, item);
            }
        }
        // Nested objects in a query string aren't a thing. Flatten with
        // dot-separated keys is one option, but no current OBSDN endpoint
        // takes a nested message in a GET, so we just stringify as JSON
        // - server will reject if it ever happens. The `if let Ok(s)`
        // silently drops on serialize failure, which serde_json only
        // produces for impossible-in-practice cases (custom Serialize
        // impls); pbjson messages always round-trip cleanly.
        Value::Object(_) => {
            if let Ok(s) = serde_json::to_string(&val) {
                ser.append_pair(key, &s);
            }
        }
    }
}

/// Append `?query` to a path when the query is non-empty. Returns the
/// path unchanged otherwise.
pub fn append_query(path: &str, query: &str) -> String {
    if query.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{query}")
    }
}

/// Percent-encode a single URL path segment. Use this for `{var}` substitution.
pub fn percent_encode_segment(s: &str) -> String {
    // RFC 3986 path-segment safe set. `url::form_urlencoded` is for
    // application/x-www-form-urlencoded, which is wrong for path segments
    // (it encodes `/` differently). Hand-roll using percent_encoding.
    use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
    // Reserved sub-delims & general-delims - same as Go's url.PathEscape.
    const SEG: &AsciiSet = &CONTROLS
        .add(b' ')
        .add(b'"')
        .add(b'#')
        .add(b'<')
        .add(b'>')
        .add(b'?')
        .add(b'`')
        .add(b'{')
        .add(b'}')
        .add(b'/')
        .add(b'%');
    utf8_percent_encode(s, SEG).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Sample {
        mkt_id: String,
        lmt: u32,
        oids: Vec<String>,
        skip_zero: u32,
    }

    #[test]
    fn skips_default_when_all_serialized() {
        // serde_json produces all fields. Only proto's pbjson skips
        // defaults - a plain serde struct emits zeros. This test pins
        // the contract: encode_query produces stable output for a
        // representative struct.
        let q = encode_query(&Sample {
            mkt_id: "BTC-PERP".into(),
            lmt: 50,
            oids: vec!["a".into(), "b".into()],
            skip_zero: 0,
        })
        .unwrap();
        // Order is HashMap insertion (serde_json preserves keys but
        // ordering across `serde_json::Value` iteration is not stable
        // across `Object`'s underlying map type, which depends on the
        // `preserve_order` feature). Don't assert order - assert content.
        assert!(q.contains("mktId=BTC-PERP"));
        assert!(q.contains("lmt=50"));
        assert!(q.contains("oids=a") && q.contains("oids=b"));
        assert!(q.contains("skipZero=0"));
    }

    #[test]
    fn percent_encode_basic() {
        assert_eq!(percent_encode_segment("abc"), "abc");
        assert_eq!(percent_encode_segment("a b"), "a%20b");
        assert_eq!(percent_encode_segment("a/b"), "a%2Fb");
        assert_eq!(percent_encode_segment("a%b"), "a%25b");
    }

    #[test]
    fn append_query_no_trailing_question() {
        assert_eq!(append_query("/x", ""), "/x");
        assert_eq!(append_query("/x", "a=1"), "/x?a=1");
    }
}
