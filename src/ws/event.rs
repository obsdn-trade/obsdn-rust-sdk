//! Server → client wire types.

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
/// dispatch into `Event` / per-subscription `Update` before exposing
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
    /// Server emits `ts` as a JSON-string-encoded nanosecond timestamp.
    /// Parsed via `from_str`.
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
///
/// Marked `#[non_exhaustive]`: new kinds may be added in future releases, so
/// downstream `match` arms must include a `_` fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UpdateKind {
    /// Initial state - replace any cached state for this filter.
    Snapshot,
    /// Incremental diff against the prior snapshot/update.
    Update,
}

/// Snapshot or update message routed to a channel subscriber.
///
/// `data` is a raw `serde_json::Value`. Use the typed helpers on [`Update`]
/// (`as_book`, `as_oracle`, ...) or `serde_json::from_value` directly.
#[derive(Debug, Clone)]
pub struct Update {
    /// Snapshot or incremental update.
    pub kind: UpdateKind,
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

/// Per-subscription event delivered by the managed [`crate::ws::Session`].
///
/// Surfaces both the data plane (snapshot/update frames) and the lifecycle
/// plane (reconnect, auth failure) in a single stream.
///
/// Variants:
/// - [`Event::Update`]: snapshot or update frame. Inspect `update.kind` to
///   discriminate - both flow through this variant.
/// - [`Event::Lagged`]: the consumer fell behind, the per-subscription
///   buffer filled, and the subscription was dropped. This is the last
///   item the stream yields before it ends. Resubscribe to resume from a
///   fresh snapshot.
/// - [`Event::Reconnected`]: the socket dropped and the supervisor
///   re-attached with auth + subs replayed. Emitted once per reconnect.
/// - [`Event::Unauthorized`]: auth replay failed (e.g. revoked key).
///   Private subscriptions stop delivering; public subs continue.
///
/// Marked `#[non_exhaustive]`: new lifecycle variants may be added in future
/// releases, so downstream `match` arms must include a `_` fallback.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Event {
    /// Normal data frame (snapshot or update).
    Update(Update),
    /// The consumer could not keep up: the subscription's bounded buffer
    /// filled and the subscription was dropped. Delivered on a best-effort
    /// basis as the final item before the stream ends. If the buffer was
    /// fully saturated the stream may simply end without this marker, so do
    /// not rely on it as the sole liveness signal. Resubscribe to resync.
    Lagged {
        /// Channel of the dropped subscription.
        channel: ChannelName,
        /// Filter of the dropped subscription (`""` for filter-less channels).
        filter: String,
    },
    /// Underlying connection re-attached. Subs replayed.
    Reconnected,
    /// Auth replay rejected by the server. Carries the server's error
    /// message for diagnostics.
    Unauthorized(String),
}
