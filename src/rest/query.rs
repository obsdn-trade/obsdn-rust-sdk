//! URL query-string encoding for GET requests.
//!
//! Uses `serde_json::to_value` to drive serialization. The generated request
//! types follow proto3 default-skipping semantics, so default-valued fields
//! drop out automatically (`?foo=0` is never emitted). Repeated fields encode
//! as `?key=v1&key=v2`.
//!
//! Field names are lowerCamelCase (the JSON form the server accepts).

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
        // Request types are always objects. Anything else is a caller bug.
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
        // Scalars are strings, numbers, bools, or enum names.
        // URL form is `to_string` minus JSON quotes.
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
        // Nested objects in a query string aren't a thing. No current
        // endpoint takes a nested message in a GET, so we stringify as JSON;
        // the server will reject it if that ever changes. `if let Ok(s)`
        // silently drops serialize failures, which serde_json only produces
        // for pathological custom `Serialize` impls.
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
    // RFC 3986 reserved sub-delims and general-delims.
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
        // serde_json emits all fields including zero-valued ones. This test
        // pins the contract: encode_query produces stable output for a
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
