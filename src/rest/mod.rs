//! REST client core. One [`RestClient`] is shared (Arc) across all
//! per-service handles (`Orders`, `Markets`, ...).

pub mod account;
pub mod asset;
pub mod auth;
pub(crate) mod auth_layer;
pub mod chain;
pub mod general;
pub mod markets;
pub mod orders;
pub mod portfolio;
pub mod price;
pub mod query;
pub mod subaccount;
pub mod vault;

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use reqwest::header::{HeaderValue, ACCEPT, CONTENT_TYPE, USER_AGENT};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::Serialize;
use url::Url;

use crate::auth::HmacSigner;
use crate::error::{Error, Result, WireError};

/// Default User-Agent. Including the crate version makes server-side
/// debugging easier when SDK consumers report problems.
const DEFAULT_USER_AGENT: &str = concat!("obsdn-sdk/", env!("CARGO_PKG_VERSION"));

/// AuthMode requirement marker for the internal request path. `Required` adds
/// the HMAC headers and errors with [`crate::Error::Auth`] when no signer
/// is configured. `Optional` adds them when a signer is configured. `None`
/// never adds them, even with a signer (used for public endpoints like
/// `GetMarkets`).
#[derive(Debug, Clone, Copy)]
pub(crate) enum AuthMode {
    /// Authenticated endpoint - fail with `Error::Auth` if no signer.
    Required,
    /// Add headers if a signer is present, otherwise pass through.
    Optional,
    /// Public endpoint - never add HMAC headers.
    None,
}

/// Successful responses are wrapped in `{"data": ..., "request_id": ...}`; we unwrap to `T`.
#[derive(serde::Deserialize)]
struct DataEnvelope<T> {
    data: T,
    #[serde(default)]
    #[allow(dead_code)]
    request_id: Option<String>,
}

/// Shared HTTP client + base URL + (optional) signer. Cheap to clone via
/// `Arc<RestClient>` from the public [`crate::Client`].
#[derive(Debug, Clone)]
pub struct RestClient {
    http: reqwest::Client,
    base: Url,
    signer: Option<HmacSigner>,
}

impl RestClient {
    pub(crate) fn new(
        base: Url,
        signer: Option<HmacSigner>,
        timeout: Duration,
        user_agent: Option<String>,
        danger_accept_invalid_certs: bool,
    ) -> Result<Self> {
        let ua = user_agent.unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());
        let ua = HeaderValue::from_str(&ua)
            .map_err(|e| Error::Config(format!("invalid user agent: {e}")))?;
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(USER_AGENT, ua);
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(timeout)
            .danger_accept_invalid_certs(danger_accept_invalid_certs)
            .build()
            .map_err(Error::from)?;
        Ok(Self { http, base, signer })
    }

    /// Issue a request and decode the gateway's `{"data": T, "request_id": ...}`
    /// envelope. `path` is appended to the base URL exactly - pass an
    /// already-encoded path with leading slash. `body` is `Some(serializable)`
    /// to send a JSON body, or `None` for empty.
    pub(crate) async fn request<TReq, TResp>(
        self: &Arc<Self>,
        method: Method,
        path: &str,
        body: Option<&TReq>,
        auth: AuthMode,
    ) -> Result<TResp>
    where
        TReq: Serialize + ?Sized,
        TResp: DeserializeOwned,
    {
        let body_bytes: Bytes = match body {
            Some(v) => Bytes::from(serde_json::to_vec(v)?),
            None => Bytes::new(),
        };
        let raw = self
            .send_raw(method, path, body_bytes, body.is_some(), auth)
            .await?;
        let envelope: DataEnvelope<TResp> = serde_json::from_slice(&raw)?;
        Ok(envelope.data)
    }

    /// GET helper. Body is always empty.
    pub(crate) async fn get<TResp>(self: &Arc<Self>, path: &str, auth: AuthMode) -> Result<TResp>
    where
        TResp: DeserializeOwned,
    {
        self.request::<(), TResp>(Method::GET, path, None, auth)
            .await
    }

    /// GET with a request struct flattened to query string. The struct's
    /// path-param fields (if any) MUST be cleared before passing - they
    /// are otherwise echoed as redundant query params.
    pub(crate) async fn get_with_query<TReq, TResp>(
        self: &Arc<Self>,
        path: &str,
        req: &TReq,
        auth: AuthMode,
    ) -> Result<TResp>
    where
        TReq: Serialize,
        TResp: DeserializeOwned,
    {
        let q = query::encode_query(req)?;
        let full = query::append_query(path, &q);
        self.get(&full, auth).await
    }

    /// POST with JSON body.
    pub(crate) async fn post<TReq, TResp>(
        self: &Arc<Self>,
        path: &str,
        req: &TReq,
        auth: AuthMode,
    ) -> Result<TResp>
    where
        TReq: Serialize + ?Sized,
        TResp: DeserializeOwned,
    {
        self.request(Method::POST, path, Some(req), auth).await
    }

    /// DELETE with JSON body.
    pub(crate) async fn delete_with_body<TReq, TResp>(
        self: &Arc<Self>,
        path: &str,
        req: &TReq,
        auth: AuthMode,
    ) -> Result<TResp>
    where
        TReq: Serialize + ?Sized,
        TResp: DeserializeOwned,
    {
        self.request(Method::DELETE, path, Some(req), auth).await
    }

    /// DELETE without body.
    pub(crate) async fn delete<TResp>(self: &Arc<Self>, path: &str, auth: AuthMode) -> Result<TResp>
    where
        TResp: DeserializeOwned,
    {
        self.request::<(), TResp>(Method::DELETE, path, None, auth)
            .await
    }

    /// DELETE with the request struct serialized as a query string (filters
    /// passed as query params rather than a body).
    pub(crate) async fn delete_with_query<TReq, TResp>(
        self: &Arc<Self>,
        path: &str,
        req: &TReq,
        auth: AuthMode,
    ) -> Result<TResp>
    where
        TReq: Serialize,
        TResp: DeserializeOwned,
    {
        let q = query::encode_query(req)?;
        let full = query::append_query(path, &q);
        self.delete(&full, auth).await
    }

    /// Lower-level send: builds URL, signs (if needed), executes, decodes
    /// errors. Returns the raw success body for the caller to unwrap.
    async fn send_raw(
        &self,
        method: Method,
        path: &str,
        body: Bytes,
        has_body: bool,
        auth: AuthMode,
    ) -> Result<Bytes> {
        // Direct base-URL + path concatenation (`Url::join` would drop the last
        // path segment when joining onto a base without a trailing slash).
        // Caller passes a full path with leading slash (e.g., `"/orders"`).
        let raw_url = format!("{}{}", self.base.as_str().trim_end_matches('/'), path);
        let url = Url::parse(&raw_url)
            .map_err(|e| Error::Config(format!("invalid url {raw_url}: {e}")))?;

        // SECURITY: the HMAC prehash covers the URL path only - not the query
        // string or host. This matches the gateway's verification today (the
        // staging e2e suite's authenticated GET-with-query calls pass). Do NOT
        // start signing the query here unilaterally: the gateway verifies
        // path-only, so adding the query would break every authenticated
        // GET/DELETE-with-query until the server is updated in lockstep. Change
        // both sides together. `Url::path` is the percent-encoded path the
        // server sees.
        let sign_path = url.path().to_string();

        let mut builder = self.http.request(method.clone(), url);
        if has_body {
            builder = builder
                .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                .body(body.clone());
        }

        builder = match auth {
            AuthMode::Required => match self.signer.as_ref() {
                Some(s) => {
                    auth_layer::apply_auth(builder, s, method.as_str(), &sign_path, body.as_ref())?
                }
                None => {
                    return Err(Error::Auth(
                        "endpoint requires authentication but no api_key was configured".into(),
                    ))
                }
            },
            AuthMode::Optional => match self.signer.as_ref() {
                Some(s) => {
                    auth_layer::apply_auth(builder, s, method.as_str(), &sign_path, body.as_ref())?
                }
                None => builder,
            },
            AuthMode::None => builder,
        };

        let resp = builder.send().await?;
        let status = resp.status();
        let body_bytes = resp.bytes().await?;
        if status.is_success() {
            return Ok(body_bytes);
        }
        Err(decode_error(status, body_bytes))
    }
}

/// Decode a non-2xx body into [`Error`]. Falls back to raw text if the
/// structured error shape can't be parsed.
fn decode_error(status: StatusCode, body: Bytes) -> Error {
    if let Ok(parsed) = serde_json::from_slice::<WireError>(&body) {
        return Error::Api {
            status: status.as_u16(),
            code: parsed.error.code,
            message: parsed.error.message,
            ref_code: parsed.error.ref_code,
            request_id: parsed.request_id,
        };
    }
    // Cap the surfaced body so a pathological error page (e.g. a multi-MB WAF
    // HTML response) doesn't bloat the error string. Truncate the raw bytes
    // before decoding so the full payload isn't decoded just to be capped;
    // `from_utf8_lossy` repairs a byte-boundary split mid-character.
    const MAX_BODY: usize = 4096;
    let body = if body.len() > MAX_BODY {
        format!(
            "{}… (truncated)",
            String::from_utf8_lossy(&body[..MAX_BODY])
        )
    } else {
        String::from_utf8_lossy(&body).into_owned()
    };
    Error::UnparsedBody {
        status: status.as_u16(),
        body,
    }
}

/// Wall-clock nanoseconds since the Unix epoch. Default EIP-712 nonce used by
/// the one-call signing helpers (`Orders::place_limit`, `Account::transfer` /
/// `withdraw`).
pub(crate) fn now_unix_nanos() -> Result<u64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| nanos_to_nonce(d.as_nanos()))
        // Fail closed: signing with a zero nonce would risk EIP-712 replay
        // and cross-request nonce collisions.
        .map_err(|_| {
            Error::Sign("system clock is before the Unix epoch; cannot generate nonce".into())
        })
}

/// Reduce nanoseconds-since-epoch to a `u64` nonce. Truncates (wraps ~every 584
/// years) rather than saturating: wrapping keeps the nonce unique within each
/// window, whereas saturating to `u64::MAX` would make every nonce identical
/// past year ~2554 and break replay protection.
fn nanos_to_nonce(nanos: u128) -> u64 {
    nanos as u64
}

#[cfg(test)]
mod tests {
    use super::nanos_to_nonce;

    #[test]
    fn nonce_wraps_instead_of_saturating() {
        // Below u64::MAX: identity.
        assert_eq!(
            nanos_to_nonce(1_700_000_000_000_000_000),
            1_700_000_000_000_000_000
        );
        // At/above u64::MAX it must WRAP, not pin to u64::MAX (which would
        // collide every subsequent nonce and break replay protection).
        assert_eq!(nanos_to_nonce(u64::MAX as u128), u64::MAX);
        assert_eq!(nanos_to_nonce(u64::MAX as u128 + 1), 0);
        assert_eq!(nanos_to_nonce(u64::MAX as u128 + 2), 1);
        // Two distinct post-2554 instants stay distinct (the property that a
        // saturating cast would destroy).
        assert_ne!(
            nanos_to_nonce(u64::MAX as u128 + 100),
            nanos_to_nonce(u64::MAX as u128 + 200)
        );
    }
}
