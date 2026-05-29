//! REST client core. One [`RestClient`] is shared (Arc) across all
//! per-service handles (`OrdersApi`, `MarketsApi`, ...).

pub mod account;
pub mod asset;
pub mod auth_api;
pub mod auth_layer;
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
const DEFAULT_USER_AGENT: &str = concat!("obsdn-sdk-rust/", env!("CARGO_PKG_VERSION"));

/// Auth requirement marker for the internal request path. `Required` adds
/// the HMAC headers and errors with [`crate::Error::Auth`] when no signer
/// is configured. `Optional` adds them when a signer is configured. `None`
/// never adds them, even with a signer (used for public endpoints like
/// `GetMarkets`).
#[derive(Debug, Clone, Copy)]
pub enum Auth {
    /// Authenticated endpoint - fail with `Error::Auth` if no signer.
    Required,
    /// Add headers if a signer is present, otherwise pass through.
    Optional,
    /// Public endpoint - never add HMAC headers.
    None,
}

/// Successful gateway responses are wrapped in
/// `{"data": ..., "request_id": "..."}` per
/// `pkg/gateway/response.go::forwardResponseWrapper`. We unwrap to `T`
/// before returning.
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
        auth: Auth,
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
    pub(crate) async fn get<TResp>(self: &Arc<Self>, path: &str, auth: Auth) -> Result<TResp>
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
        auth: Auth,
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
        auth: Auth,
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
        auth: Auth,
    ) -> Result<TResp>
    where
        TReq: Serialize + ?Sized,
        TResp: DeserializeOwned,
    {
        self.request(Method::DELETE, path, Some(req), auth).await
    }

    /// DELETE without body.
    pub(crate) async fn delete<TResp>(self: &Arc<Self>, path: &str, auth: Auth) -> Result<TResp>
    where
        TResp: DeserializeOwned,
    {
        self.request::<(), TResp>(Method::DELETE, path, None, auth)
            .await
    }

    /// DELETE with the request struct serialized as a query string. Used
    /// by `CancelAllOrders` (no `body: "*"` annotation).
    pub(crate) async fn delete_with_query<TReq, TResp>(
        self: &Arc<Self>,
        path: &str,
        req: &TReq,
        auth: Auth,
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
        auth: Auth,
    ) -> Result<Bytes> {
        // Direct concatenation mirrors `pkg/exc/client.go` (`baseURL + path`)
        // - `Url::join` resolves relative refs (it would drop the last
        // segment when joining `"orders"` onto a base without a trailing
        // slash), which is the wrong semantics for our flat REST surface.
        // Caller passes a full path with leading slash (e.g., `"/orders"`).
        let raw_url = format!("{}{}", self.base.as_str().trim_end_matches('/'), path);
        let url = Url::parse(&raw_url)
            .map_err(|e| Error::Config(format!("invalid url {raw_url}: {e}")))?;

        // Path used for HMAC signing must be the URL path component ONLY,
        // matching the gateway's `r.URL.Path` (no query, no host).
        // `Url::path` returns the percent-encoded path the server will see.
        let sign_path = url.path().to_string();

        let mut builder = self.http.request(method.clone(), url);
        if has_body {
            builder = builder
                .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                .body(body.clone());
        }

        builder = match auth {
            Auth::Required => match self.signer.as_ref() {
                Some(s) => {
                    auth_layer::apply_auth(builder, s, method.as_str(), &sign_path, body.as_ref())
                }
                None => {
                    return Err(Error::Auth(
                        "endpoint requires authentication but no api_key was configured".into(),
                    ))
                }
            },
            Auth::Optional => match self.signer.as_ref() {
                Some(s) => {
                    auth_layer::apply_auth(builder, s, method.as_str(), &sign_path, body.as_ref())
                }
                None => builder,
            },
            Auth::None => builder,
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

/// Decode a non-2xx body. Server returns `ErrorResponse`
/// (`pkg/gateway/response.go`); fall back to raw text if parsing fails so
/// we don't swallow new shapes.
fn decode_error(status: StatusCode, body: Bytes) -> Error {
    if let Ok(parsed) = serde_json::from_slice::<WireError>(&body) {
        return Error::Api {
            status: status.as_u16(),
            code: parsed.error.code,
            message: parsed.error.message,
            request_id: parsed.request_id,
        };
    }
    Error::UnparseableError {
        status: status.as_u16(),
        body: String::from_utf8_lossy(&body).into_owned(),
    }
}
