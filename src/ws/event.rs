//! Server → client wire types.
//!
//! Mirrors `services/pulse/channel/op.go` (`AuthResponse`,
//! `SubscriptionResponse`, `ErrorResponse`, `ChannelMessage`). The thin
//! client surfaces `data` as `serde_json::Value` so callers can deserialize
//! into per-channel typed structs they own (Phase 7 ergonomic wrappers will
//! provide ready-made views - keeping Phase 5 minimal).

use serde::Deserialize;

use super::channel::ChannelName;

/// `type` field on every server frame. Drives our routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum WireType {
    Welcome,
    Authenticated,
    Subscribed,
    Unsubscribed,
    Snapshot,
    Update,
    Error,
    Pong,
}

/// A single server frame, after JSON parse but before routing. Internal - we
/// dispatch into `WsEvent` / per-subscription `WsUpdate` before exposing
/// anything to callers.
#[derive(Debug, Deserialize)]
pub(crate) struct ServerFrame {
    #[serde(rename = "type")]
    pub kind: WireType,
    #[serde(default)]
    pub channel: Option<ChannelName>,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub filter: Option<String>,
    #[serde(default)]
    pub gsn: Option<u64>,
    /// Server emits `ts` as a JSON-string-encoded i64 nanosecond timestamp
    /// (`json:"ts,string"` on the Go side). Parse via `from_str`.
    #[serde(default, deserialize_with = "deserialize_opt_str_u64")]
    pub ts: Option<u64>,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

fn deserialize_opt_str_u64<'de, D>(d: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(d)?;
    opt.map(|s| s.parse::<u64>().map_err(serde::de::Error::custom))
        .transpose()
}

/// Whether a data frame is the initial state or a subsequent diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsUpdateKind {
    /// Initial state - replace any cached state for this filter.
    Snapshot,
    /// Incremental diff against the prior snapshot/update.
    Update,
}

/// Snapshot or update message routed to a channel subscriber.
///
/// Phase 5 keeps `data` as `serde_json::Value`. Callers can `serde_json::
/// from_value` it into a per-channel typed struct (see
/// `docs/api/ws-integration.md` for schemas).
#[derive(Debug, Clone)]
pub struct WsUpdate {
    /// Snapshot or incremental update.
    pub kind: WsUpdateKind,
    /// Routing channel (`book`, `oracle`, ...).
    pub channel: ChannelName,
    /// Server `gsn` - a global event watermark, monotonic but sparse per
    /// subscription (channels emit selectively). Not a dense sequence; do
    /// not infer dropped messages from gaps between consecutive frames.
    pub gsn: u64,
    /// Server-side timestamp in nanoseconds.
    pub ts: u64,
    /// Channel filter (`"BTC-PERP"`, `"BTC"`, ...). `""` for filter-less
    /// channels (`portfolio`, `notification`).
    pub filter: String,
    /// Raw channel payload - decode via [`crate::ws::views`] helpers
    /// (`as_book`, `as_oracle`, ...) or `serde_json::from_value`.
    pub data: serde_json::Value,
}

/// Per-subscription event delivered by the managed [`crate::ws::WsClient`].
///
/// The managed client surfaces both the data plane (snapshot/update frames
/// from the server) and the lifecycle plane (reconnect, auth failure) in a
/// single stream so callers don't have to juggle two channels.
///
/// Variants:
/// - [`WsEvent::Update`]: a snapshot or update frame. Inspect
///   `update.kind` to discriminate - both flow through this variant.
/// - [`WsEvent::Reconnected`]: the underlying socket dropped and the
///   supervisor re-attached. Subscriptions and authentication were
///   replayed automatically. Emitted once per reconnect.
/// - [`WsEvent::Unauthorized`]: an auth replay failed (e.g., revoked key).
///   Private subscriptions on this connection will not deliver further
///   updates until the caller re-authenticates. Public subscriptions
///   continue working.
#[derive(Debug, Clone)]
pub enum WsEvent {
    /// Normal data frame (snapshot or update).
    Update(WsUpdate),
    /// Underlying connection re-attached. Subs replayed.
    Reconnected,
    /// Auth replay rejected by the server. Carries the server's error
    /// message for diagnostics.
    Unauthorized(String),
}
