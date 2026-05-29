//! Per-client market metadata cache.
//!
//! `Market.idx` is needed at the EIP-712 hashing site for every order
//! (`OrderPayload.market_index`). Fetching `/markets` per order would burn
//! a roundtrip on the hot path; this module memoizes a snapshot of the
//! `GetMarketsResponse` and refreshes it lazily on TTL expiry or explicit
//! invalidation.
//!
//! Refresh is single-flight per client: concurrent callers awaiting the
//! same expired snapshot share one in-flight REST call. Failures are NOT
//! cached - a transient error doesn't poison the cache.
//!
//! TTL default 60s - markets are static (`idx`/`mkt_id` never change at
//! runtime); the TTL only protects against operator-driven market
//! adds/removes propagating to active SDK clients within a minute.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::error::{Error, Result};
use crate::rest::markets::Markets;
use crate::types::v1::Market;

/// Default lifetime of a cached snapshot before lazy refresh.
pub const DEFAULT_TTL: Duration = Duration::from_secs(60);

#[derive(Clone)]
struct Snapshot {
    /// Symbol (`mkt_id`) → Market. Cheap clone - `Market` itself is small
    /// strings.
    by_symbol: Arc<HashMap<String, Market>>,
    fetched_at: Instant,
}

/// Lazy, single-flight cache of the markets list.
///
/// Constructed internally by [`crate::Client`]; not part of the public
/// surface. The user-facing entry point is `Client::resolve_market(...)`.
pub(crate) struct MarketCache {
    rest: Markets,
    ttl: Duration,
    // Single mutex protects both the snapshot cell and the
    // refresh-in-progress flag - under TTL expiry we want exactly one
    // flight, and the cache is a cold path (a single ms of contention
    // dwarfs the network call we're avoiding).
    state: Mutex<Option<Snapshot>>,
}

impl MarketCache {
    pub(crate) fn new(rest: Markets) -> Self {
        Self {
            rest,
            ttl: DEFAULT_TTL,
            state: Mutex::new(None),
        }
    }

    /// Override the default TTL. Currently unused outside tests but kept
    /// crate-public for future tuning hooks.
    #[allow(dead_code)]
    pub(crate) fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Force a refresh on the next call. Use after seeing a "market not
    /// found" error against a known-good symbol.
    pub(crate) async fn invalidate(&self) {
        *self.state.lock().await = None;
    }

    /// Return a snapshot, refreshing if absent or stale.
    async fn snapshot(&self) -> Result<Snapshot> {
        let mut guard = self.state.lock().await;
        if let Some(snap) = guard.as_ref() {
            if snap.fetched_at.elapsed() < self.ttl {
                return Ok(snap.clone());
            }
        }
        // Fetch under the lock - single-flight by construction. Other
        // callers block on the mutex; once we publish the snapshot they
        // see it and skip the REST call.
        let resp = self.rest.list().await?;
        let mut by_symbol = HashMap::with_capacity(resp.mkts.len());
        for mkt in resp.mkts {
            by_symbol.insert(mkt.mkt_id.clone(), mkt);
        }
        let snap = Snapshot {
            by_symbol: Arc::new(by_symbol),
            fetched_at: Instant::now(),
        };
        *guard = Some(snap.clone());
        Ok(snap)
    }

    /// Look up a market by its `mkt_id` (e.g. `"BTC-PERP"`). Refreshes
    /// transparently on TTL expiry. Returns `Error::Config` if unknown
    /// after a fresh fetch - caller should treat this as a permanent
    /// configuration error, not a retryable failure.
    pub(crate) async fn resolve(&self, mkt_id: &str) -> Result<Market> {
        let snap = self.snapshot().await?;
        if let Some(m) = snap.by_symbol.get(mkt_id) {
            return Ok(m.clone());
        }
        // Miss + snapshot was fresh → market truly unknown. Don't burn a
        // second REST call; the operator either renamed the symbol or
        // the caller mistyped.
        Err(Error::Config(format!("unknown market: {mkt_id}")))
    }

    /// Parse `Market.idx` (a decimal string) into the `u16` shape required
    /// by the EIP-712 `Order.marketIndex` field.
    pub(crate) fn idx_as_u16(market: &Market) -> Result<u16> {
        market.idx.parse::<u16>().map_err(|e| {
            Error::Config(format!(
                "market {} has invalid idx {}: {e}",
                market.mkt_id, market.idx
            ))
        })
    }
}
