//! HMAC header injection for REST requests.
//!
//! We do NOT use `reqwest_middleware` here. The middleware path forces the
//! request body to be re-read after the user's call returns, which means we
//! either lose streaming bodies or buffer twice. Since signing the body is
//! mandatory anyway, the [`crate::rest::RestClient`] hands raw body bytes
//! straight to [`apply_auth`] and we attach headers in one place.

use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::{header::HeaderName, RequestBuilder};

use crate::auth::HmacSigner;

/// `x-api-key` header - identifies the API key.
pub const HEADER_API_KEY: HeaderName = HeaderName::from_static("x-api-key");
/// `x-api-signature` header - base64 HMAC-SHA256 over the prehash.
pub const HEADER_API_SIGNATURE: HeaderName = HeaderName::from_static("x-api-signature");
/// `x-api-timestamp` header - unix-epoch seconds at signing time.
pub const HEADER_API_TIMESTAMP: HeaderName = HeaderName::from_static("x-api-timestamp");

/// Attach `x-api-key`, `x-api-signature`, `x-api-timestamp` to a request.
///
/// `path` MUST be the URL path only (no host, no query string). `body` is
/// the exact bytes that will be transmitted - pass `&[]` for empty.
pub fn apply_auth(
    builder: RequestBuilder,
    signer: &HmacSigner,
    method: &str,
    path: &str,
    body: &[u8],
) -> RequestBuilder {
    let timestamp = current_unix_seconds();
    let signature = signer.sign(&timestamp, method, path, body);
    builder
        .header(HEADER_API_KEY, signer.api_key())
        .header(HEADER_API_SIGNATURE, signature)
        .header(HEADER_API_TIMESTAMP, timestamp)
}

/// Unix-epoch seconds as a decimal string. Matches Go's
/// `strconv.FormatInt(time.Now().Unix(), 10)` semantics. Pre-epoch clocks
/// (which can't reach our servers anyway) emit `"0"` rather than panic.
fn current_unix_seconds() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
