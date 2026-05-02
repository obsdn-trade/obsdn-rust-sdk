//! SDK error types.
//!
//! `Error` is the top-level result type. `Api` wraps the JSON envelope the
//! grpc-gateway emits on non-2xx (`pkg/gateway/response.go::ErrorResponse`):
//! `{"error":{"code":"...","message":"..."},"request_id":"..."}` — `code`
//! is the gRPC status string (e.g., `"InvalidArgument"`, `"Unauthenticated"`).

use thiserror::Error;

/// SDK-wide result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by the SDK surface.
#[derive(Debug, Error)]
pub enum Error {
    /// Network / TLS / I/O failure before a response was parsed.
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),

    /// Server returned non-2xx with a parsed error envelope.
    #[error("api error {status} {code}: {message}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// gRPC status code as string (e.g., `"InvalidArgument"`).
        code: String,
        /// Human-readable message from the server.
        message: String,
        /// Server-assigned request id (echoed via `X-Request-Id`), if any.
        request_id: Option<String>,
    },

    /// Server returned non-2xx but the body did not match `ErrorResponse`.
    /// Surface the raw body so callers can debug new shapes without us
    /// guessing.
    #[error("api error {status}: unparseable body: {body}")]
    UnparseableError {
        /// HTTP status code.
        status: u16,
        /// Raw response body, truncated by the caller if huge.
        body: String,
    },

    /// Auth misconfiguration (e.g., signer required by endpoint but missing).
    #[error("auth error: {0}")]
    Auth(String),

    /// EIP-712 signing failure (Phase 4+).
    #[error("sign error: {0}")]
    Sign(String),

    /// Response decode failure (JSON shape mismatch).
    #[error("decode error: {0}")]
    Decode(#[from] serde_json::Error),

    /// Builder / config validation failure.
    #[error("config error: {0}")]
    Config(String),

    /// WebSocket-specific failure (handshake, oversize frame, server error
    /// response, lost connection, ...). The thin client surfaces server
    /// `error` frames via this variant — the message is the server's raw
    /// `message` field.
    #[error("websocket error: {0}")]
    Ws(String),
}

/// JSON shape used by `pkg/gateway/response.go::ErrorResponse`. Internal
/// helper — surfaced through `Error::Api`.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct WireError {
    pub error: WireErrorDetail,
    #[serde(default)]
    pub request_id: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct WireErrorDetail {
    pub code: String,
    pub message: String,
}
