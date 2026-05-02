//! Client → server frame builders.
//!
//! Server cap: 512B per inbound frame. We enforce that here so a misuse
//! surfaces immediately rather than silently disconnecting.

use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

use crate::error::{Error, Result};

use super::channel::Channel;

/// Hard cap from `services/pulse/io/websocket.go` — server drops the
/// connection on overflow. Verified at build time so a payload that would
/// disconnect us errors before the send.
pub(crate) const MAX_CLIENT_FRAME_BYTES: usize = 512;

/// `{"op":"ping"}`
pub(crate) fn ping() -> Message {
    Message::Text(r#"{"op":"ping"}"#.to_string())
}

/// `{"op":"sub","channel":"book","params":{"market":"BTC-PERP"}}`
pub(crate) fn subscribe(channel: &Channel) -> Result<Message> {
    op_with_channel("sub", channel)
}

/// `{"op":"unsub","channel":"book","params":{"market":"BTC-PERP"}}`
pub(crate) fn unsubscribe(channel: &Channel) -> Result<Message> {
    op_with_channel("unsub", channel)
}

/// `{"op":"auth","params":{"key":"...","timestamp":"...","signature":"..."}}`
pub(crate) fn auth(key: &str, timestamp: &str, signature: &str) -> Result<Message> {
    let body = json!({
        "op": "auth",
        "params": {
            "key": key,
            "timestamp": timestamp,
            "signature": signature,
        }
    });
    encode(&body)
}

fn op_with_channel(op: &str, channel: &Channel) -> Result<Message> {
    let params = channel.wire_params();
    let body = if params.is_null() {
        json!({ "op": op, "channel": channel.name().as_str() })
    } else {
        json!({ "op": op, "channel": channel.name().as_str(), "params": params })
    };
    encode(&body)
}

fn encode(value: &serde_json::Value) -> Result<Message> {
    let s = serde_json::to_string(value)?;
    if s.len() > MAX_CLIENT_FRAME_BYTES {
        return Err(Error::Ws(format!(
            "client frame {} bytes exceeds server cap of {}",
            s.len(),
            MAX_CLIENT_FRAME_BYTES
        )));
    }
    Ok(Message::Text(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(m: Message) -> String {
        match m {
            Message::Text(s) => s,
            _ => panic!("expected text frame"),
        }
    }

    #[test]
    fn ping_payload() {
        assert_eq!(text(ping()), r#"{"op":"ping"}"#);
    }

    #[test]
    fn subscribe_book() {
        let m = subscribe(&Channel::Book {
            market: "BTC-PERP".into(),
        })
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&text(m)).unwrap();
        assert_eq!(v["op"], "sub");
        assert_eq!(v["channel"], "book");
        assert_eq!(v["params"]["market"], "BTC-PERP");
    }

    #[test]
    fn subscribe_no_filter_omits_params() {
        let m = subscribe(&Channel::Portfolio).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text(m)).unwrap();
        assert_eq!(v["op"], "sub");
        assert_eq!(v["channel"], "portfolio");
        assert!(v.get("params").is_none(), "params should be omitted");
    }

    #[test]
    fn auth_frame_shape() {
        let m = auth("KEY", "1700000000", "SIG==").unwrap();
        let v: serde_json::Value = serde_json::from_str(&text(m)).unwrap();
        assert_eq!(v["op"], "auth");
        assert_eq!(v["params"]["key"], "KEY");
        assert_eq!(v["params"]["timestamp"], "1700000000");
        assert_eq!(v["params"]["signature"], "SIG==");
    }

    #[test]
    fn frame_size_guard_rejects_oversize() {
        // Build a giant filter to overflow the 512B cap.
        let huge = "x".repeat(1024);
        let err = subscribe(&Channel::Oracle { asset: huge }).expect_err("should reject oversize");
        assert!(format!("{err}").contains("exceeds server cap"));
    }
}
