//! Subscription channels.
//!
//! The lower-case wire names are the canonical identifiers - typed via
//! [`ChannelName`] for routing and [`Channel`] for the user-facing API
//! which carries the per-channel filter.

use serde::{Deserialize, Serialize};

/// Lower-case channel name as it appears on the wire (`"book"`, `"oracle"`,
/// ...). Used as the routing key for incoming snapshot/update frames.
///
/// Marked `#[non_exhaustive]`: new channels may be added in future releases,
/// so downstream `match` arms must include a `_` fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ChannelName {
    /// `oracle` - price feed per asset.
    Oracle,
    /// `trade` - public trade executions.
    Trade,
    /// `book` - order-book depth.
    Book,
    /// `ticker` - best bid/ask.
    Ticker,
    /// `order` - private user-order updates.
    Order,
    /// `position` - private user-position updates.
    Position,
    /// `portfolio` - private portfolio summary.
    Portfolio,
    /// `notification` - private deposit/withdraw alerts.
    Notification,
    /// `event` - debug stream of all sequenced events.
    Event,
}

impl ChannelName {
    /// Server requires authentication before subscribing to these.
    ///
    /// Exhaustive match (not `matches!`) so a future channel variant forces an
    /// explicit private/public classification at compile time rather than
    /// silently defaulting to public (which would allow an unauthenticated
    /// subscribe attempt on a private channel).
    pub fn is_private(self) -> bool {
        match self {
            ChannelName::Order
            | ChannelName::Position
            | ChannelName::Portfolio
            | ChannelName::Notification => true,
            ChannelName::Oracle
            | ChannelName::Trade
            | ChannelName::Book
            | ChannelName::Ticker
            | ChannelName::Event => false,
        }
    }

    /// Wire string (`"oracle"`, `"book"`, ...).
    pub fn as_str(self) -> &'static str {
        match self {
            ChannelName::Oracle => "oracle",
            ChannelName::Trade => "trade",
            ChannelName::Book => "book",
            ChannelName::Ticker => "ticker",
            ChannelName::Order => "order",
            ChannelName::Position => "position",
            ChannelName::Portfolio => "portfolio",
            ChannelName::Notification => "notification",
            ChannelName::Event => "event",
        }
    }
}

/// User-facing subscription request. Carries the channel name and its filter.
/// The SDK validates the filter shape before sending.
///
/// Marked `#[non_exhaustive]`: new channels may be added in future releases,
/// so downstream `match` arms must include a `_` fallback. Variants are still
/// constructible directly or via the helper constructors ([`Channel::book`],
/// [`Channel::order`], ...).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Channel {
    /// `oracle` - `asset` (e.g. `"BTC"`) is required.
    Oracle {
        /// Asset symbol filter (required).
        asset: String,
    },
    /// `trade` - `market` filter optional (omit for all-markets stream).
    Trade {
        /// Optional market filter (`Some("BTC-PERP")` or `None`).
        market: Option<String>,
    },
    /// `book` - `market` (e.g. `"BTC-PERP"`) is required.
    Book {
        /// Market symbol filter (required).
        market: String,
    },
    /// `ticker` - `market` is required.
    Ticker {
        /// Market symbol filter (required).
        market: String,
    },
    /// `order` - private. `market` filter optional.
    Order {
        /// Optional market filter - `None` streams all markets.
        market: Option<String>,
    },
    /// `position` - private. `market` filter optional.
    Position {
        /// Optional market filter.
        market: Option<String>,
    },
    /// `portfolio` - private. No filter accepted.
    Portfolio,
    /// `notification` - private. No filter accepted.
    Notification,
    /// `event` - debug only. Optional event-type filter.
    Event {
        /// Optional event-type filter (e.g. `"ORDER_PLACED"`).
        event: Option<String>,
    },
}

impl Channel {
    /// Subscribe to the order book for `market` (e.g. `"BTC-PERP"`).
    pub fn book(market: impl Into<String>) -> Self {
        Channel::Book {
            market: market.into(),
        }
    }

    /// Subscribe to best bid/ask for `market`.
    pub fn ticker(market: impl Into<String>) -> Self {
        Channel::Ticker {
            market: market.into(),
        }
    }

    /// Subscribe to the oracle price feed for `asset` (e.g. `"BTC"`).
    pub fn oracle(asset: impl Into<String>) -> Self {
        Channel::Oracle {
            asset: asset.into(),
        }
    }

    /// Subscribe to public trades. `None` streams every market.
    pub fn trade(market: Option<&str>) -> Self {
        Channel::Trade {
            market: market.map(str::to_string),
        }
    }

    /// Subscribe to your order updates (private). `None` streams every market.
    pub fn order(market: Option<&str>) -> Self {
        Channel::Order {
            market: market.map(str::to_string),
        }
    }

    /// Subscribe to your position updates (private). `None` streams every market.
    pub fn position(market: Option<&str>) -> Self {
        Channel::Position {
            market: market.map(str::to_string),
        }
    }

    /// Subscribe to the debug event stream. `None` streams every event type.
    pub fn event(event: Option<&str>) -> Self {
        Channel::Event {
            event: event.map(str::to_string),
        }
    }

    /// Routing-side name.
    pub fn name(&self) -> ChannelName {
        match self {
            Channel::Oracle { .. } => ChannelName::Oracle,
            Channel::Trade { .. } => ChannelName::Trade,
            Channel::Book { .. } => ChannelName::Book,
            Channel::Ticker { .. } => ChannelName::Ticker,
            Channel::Order { .. } => ChannelName::Order,
            Channel::Position { .. } => ChannelName::Position,
            Channel::Portfolio => ChannelName::Portfolio,
            Channel::Notification => ChannelName::Notification,
            Channel::Event { .. } => ChannelName::Event,
        }
    }

    /// Filter as the server sees it on incoming frames (`""` when absent).
    pub fn filter(&self) -> &str {
        match self {
            Channel::Oracle { asset } => asset.as_str(),
            Channel::Book { market } | Channel::Ticker { market } => market.as_str(),
            Channel::Trade { market }
            | Channel::Order { market }
            | Channel::Position { market } => market.as_deref().unwrap_or(""),
            Channel::Event { event } => event.as_deref().unwrap_or(""),
            Channel::Portfolio | Channel::Notification => "",
        }
    }

    /// Wire `params` object for the `sub`/`unsub` request. Returns `null`
    /// when no filter applies (server tolerates either `null` or an empty
    /// object for filter-less channels).
    pub(crate) fn wire_params(&self) -> serde_json::Value {
        use serde_json::json;
        match self {
            Channel::Oracle { asset } => json!({ "asset": asset }),
            Channel::Book { market } | Channel::Ticker { market } => json!({ "market": market }),
            Channel::Trade { market: Some(m) }
            | Channel::Order { market: Some(m) }
            | Channel::Position { market: Some(m) } => json!({ "market": m }),
            Channel::Trade { market: None }
            | Channel::Order { market: None }
            | Channel::Position { market: None } => serde_json::Value::Null,
            Channel::Event { event: Some(e) } => json!({ "event": e }),
            Channel::Event { event: None } => serde_json::Value::Null,
            Channel::Portfolio | Channel::Notification => serde_json::Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_name_serde_lowercase() {
        let s = serde_json::to_string(&ChannelName::Book).unwrap();
        assert_eq!(s, "\"book\"");
        let n: ChannelName = serde_json::from_str("\"oracle\"").unwrap();
        assert_eq!(n, ChannelName::Oracle);
    }

    #[test]
    fn private_classification_matches_server() {
        for n in [
            ChannelName::Order,
            ChannelName::Position,
            ChannelName::Portfolio,
            ChannelName::Notification,
        ] {
            assert!(n.is_private(), "{n:?} should be private");
        }
        for n in [
            ChannelName::Oracle,
            ChannelName::Trade,
            ChannelName::Book,
            ChannelName::Ticker,
            ChannelName::Event,
        ] {
            assert!(!n.is_private(), "{n:?} should be public");
        }
    }

    #[test]
    fn wire_params_book_market() {
        let p = Channel::Book {
            market: "BTC-PERP".into(),
        }
        .wire_params();
        assert_eq!(p, serde_json::json!({"market": "BTC-PERP"}));
    }

    #[test]
    fn wire_params_trade_no_filter_is_null() {
        let p = Channel::Trade { market: None }.wire_params();
        assert!(p.is_null());
    }
}
