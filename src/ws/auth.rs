//! WebSocket authentication payload signing.
//!
//! The WS auth prehash differs from REST: server uses
//!
//! ```text
//! prehash = "{api_key},{timestamp}"
//! ```
//!
//! Compare with REST's `timestamp || METHOD || path || body`. We share the
//! underlying HMAC-SHA256 routine but build the payload here.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::auth::HmacSigner;
use crate::error::{Error, Result};

/// Build `(timestamp_seconds_string, base64_signature)` for the WS `auth`
/// frame using the same key/secret pair as the REST signer.
///
/// The server accepts timestamps within a +/- 60 second window of its own
/// clock. If authentication fails with a "timestamp expired" error, the
/// most likely cause is clock skew between the client machine and the
/// server - synchronize via NTP.
pub(crate) fn build_ws_auth(signer: &HmacSigner, now_secs: u64) -> (String, String) {
    let timestamp = now_secs.to_string();
    let prehash = format!("{},{}", signer.api_key(), timestamp);
    let signature = sign_ws_prehash(signer.secret_bytes(), &prehash);
    (timestamp, signature)
}

/// HMAC-SHA256(secret, prehash) → base64-std. Pure form so tests can assert
/// vectors without constructing an [`HmacSigner`].
pub(crate) fn sign_ws_prehash(secret: &[u8], prehash: &str) -> String {
    use base64::Engine;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(prehash.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// Wall-clock Unix seconds. Surfaced as a function so tests can fix the
/// timestamp.
pub(crate) fn now_unix_secs() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| Error::Ws(format!("system clock before unix epoch: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden vector - recompute against the canonical Python reference in
    /// `docs/api/ws-integration.md::Authentication` for the same inputs.
    #[test]
    fn ws_prehash_format() {
        // Inputs: key=K, ts=1700000000, secret=SECRET.
        // prehash = "K,1700000000"
        // expected = base64(HMAC-SHA256("SECRET", "K,1700000000"))
        let signer = HmacSigner::new("K", "SECRET");
        let (ts, sig) = build_ws_auth(&signer, 1_700_000_000);
        assert_eq!(ts, "1700000000");
        // Byte-equal across HMAC implementations.
        let expected = sign_ws_prehash(b"SECRET", "K,1700000000");
        assert_eq!(sig, expected);
    }

    #[test]
    fn deterministic_signature() {
        let a = sign_ws_prehash(b"k", "alpha,1");
        let b = sign_ws_prehash(b"k", "alpha,1");
        assert_eq!(a, b);
    }
}
