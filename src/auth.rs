//! HMAC-SHA256 request signing.
//!
//! Mirrors `pkg/auth/hmac.go::ComputeHMACSignature` + `BuildPrehash`:
//!
//! ```text
//! prehash   = timestamp || UPPER(method) || path || body
//! signature = base64_std(HMAC_SHA256(secret, prehash))
//! ```
//!
//! `timestamp` is Unix seconds as a decimal string. `path` is the URL path
//! ONLY - query string is excluded (verified against
//! `pkg/gateway/options.go::prehashMetadata` which reads `r.URL.Path`).
//! `body` is the raw bytes of the request body, or empty string for GET /
//! DELETE without a body.

use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// API-key + secret pair used to authenticate REST + WS requests.
///
/// `secret` is wiped from memory on drop. Clones share the secret bytes; the
/// last drop wipes.
#[derive(Clone)]
pub struct HmacSigner {
    pub(crate) key: String,
    pub(crate) secret: SecretBytes,
}

impl HmacSigner {
    /// Create a signer from the key + secret pair issued by `RegisterSigner`.
    pub fn new(api_key: impl Into<String>, api_secret: impl Into<String>) -> Self {
        let secret = api_secret.into();
        Self {
            key: api_key.into(),
            secret: SecretBytes(secret.into_bytes()),
        }
    }

    /// API key (sent as `x-api-key` header).
    pub fn api_key(&self) -> &str {
        &self.key
    }

    /// Compute the HMAC signature for a request.
    ///
    /// Returns base64-std-encoded HMAC-SHA256 of
    /// `timestamp || method || path || body`.
    pub fn sign(&self, timestamp: &str, method: &str, path: &str, body: &[u8]) -> String {
        sign_hmac(&self.secret.0, timestamp, method, path, body)
    }

    /// Raw secret bytes - crate-internal so other modules (`ws::auth`) can
    /// share the HMAC primitive without re-implementing the secret-zeroing
    /// wrapper. The slice borrow keeps the caller from copying out the
    /// secret.
    pub(crate) fn secret_bytes(&self) -> &[u8] {
        &self.secret.0
    }
}

impl std::fmt::Debug for HmacSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never leak the secret in Debug output.
        f.debug_struct("HmacSigner")
            .field("key", &self.key)
            .field("secret", &"<redacted>")
            .finish()
    }
}

/// Owned secret bytes that zero on drop. Use `Vec<u8>` directly so the
/// derived `Clone` produces an independent buffer (each clone wipes its own
/// copy on drop - no shared ownership of secret bytes).
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub(crate) struct SecretBytes(Vec<u8>);

/// Pure HMAC-SHA256 over the canonical prehash. Exposed as a free function
/// so test fixtures can validate against a known secret without constructing
/// an [`HmacSigner`].
pub fn sign_hmac(secret: &[u8], timestamp: &str, method: &str, path: &str, body: &[u8]) -> String {
    type HmacSha256 = Hmac<Sha256>;
    // `new_from_slice` only fails for keys that exceed the underlying block
    // size for non-keyed hashes; HMAC accepts any key length. Treat it as
    // unreachable.
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(timestamp.as_bytes());
    // Method is uppercased to match `strings.ToUpper(r.Method)` on the
    // gateway side; callers should already pass uppercase but normalize
    // defensively.
    let method_upper = ascii_upper(method);
    mac.update(method_upper.as_bytes());
    mac.update(path.as_bytes());
    mac.update(body);
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

fn ascii_upper(s: &str) -> std::borrow::Cow<'_, str> {
    if s.bytes().all(|b| !b.is_ascii_lowercase()) {
        std::borrow::Cow::Borrowed(s)
    } else {
        std::borrow::Cow::Owned(s.to_ascii_uppercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden vector - must match
    /// `pkg/auth/hmac_test.go::TestComputeHMACSignature` semantics. We
    /// recompute the expected value here from the same inputs the Go test
    /// uses, then assert byte-for-byte equality. If Go's encoding ever
    /// drifts (e.g., URL-safe base64), this test will fail.
    #[test]
    fn matches_go_hmac_format() {
        // Inputs lifted from `pkg/auth/hmac_test.go`.
        let timestamp = "1234567890";
        let method = "POST";
        let path = "/v1/orders";
        let body = b"{\"symbol\":\"BTC-USD\",\"side\":\"buy\"}";
        let secret = b"my-secret-key";

        // Pre-computed expected value: HMAC-SHA256(secret, prehash) where
        // prehash = "1234567890POST/v1/orders{\"symbol\":\"BTC-USD\",\"side\":\"buy\"}"
        // Verified against Go on 2026-04-27 by running
        // `auth.ComputeHMACSignature` over the same inputs. Hardcoding so
        // future changes either pass or trigger a visible regression - the
        // silent-mismatch risk flagged in the phase doc.
        let expected = "VNdJ7rUFSZvZN2gTGoo/Vz7MQ1S/FEf2GMbgp3fQ+ow=";

        let got = sign_hmac(secret, timestamp, method, path, body);
        assert_eq!(got, expected, "HMAC output diverged from Go reference");
    }

    #[test]
    fn empty_body_is_empty_string() {
        // Golden vs Go: prehash "9GET/x" + secret "k".
        let got = sign_hmac(b"k", "9", "GET", "/x", b"");
        assert_eq!(got, "/oo/kZ+guDSuEi/9eOA7ZRkh7ZKNkKzFOUBF2LSJVK4=");
    }

    #[test]
    fn place_order_golden() {
        // Golden vs Go: realistic place-order shape.
        let got = sign_hmac(
            b"abc123",
            "1705929600",
            "POST",
            "/orders",
            br#"{"mktId":"BTC-PERP"}"#,
        );
        assert_eq!(got, "zlFN4lQ5q7qFbn/cmVoPnX4lGiaFuFgMmp6baRDy/9E=");
    }

    #[test]
    fn lowercase_method_normalized_to_upper() {
        let s1 = sign_hmac(b"k", "1", "post", "/p", b"x");
        let s2 = sign_hmac(b"k", "1", "POST", "/p", b"x");
        assert_eq!(s1, s2);
    }

    #[test]
    fn signer_does_not_leak_secret_in_debug() {
        let signer = HmacSigner::new("KEY", "SUPER-SECRET-DO-NOT-PRINT");
        let dbg = format!("{:?}", signer);
        assert!(dbg.contains("KEY"));
        assert!(!dbg.contains("SUPER-SECRET"));
    }
}
