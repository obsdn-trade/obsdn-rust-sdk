//! Per-subscription Global Sequence Number tracking.
//!
//! Pulse stamps every snapshot/update with a `gsn` that is monotonically
//! increasing per (channel, filter). The supervisor compares each frame's
//! GSN against the last seen value to detect gaps — typically caused by
//! the server dropping our connection's outbox under backpressure
//! (`services/pulse/io/websocket.go::sendOnFull`) or by a reconnect that
//! rejoins at a later GSN.
//!
//! Reset semantics: on reconnect, the first frame after re-attach is NOT
//! compared against the prior session — we only emit `Reconnected` once
//! and start tracking fresh from the new GSN. Comparing across sessions
//! would produce spurious gaps every reconnect.

/// Half-open gap range `[from, to]` inclusive — the GSNs the server emitted
/// but we never saw. Caller is expected to resync via REST snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GsnGap {
    /// First missed GSN (inclusive).
    pub from: u64,
    /// Last missed GSN (inclusive).
    pub to: u64,
}

/// Tracks the highest GSN observed for a single subscription. Not thread-
/// safe — the supervisor task is the sole owner.
#[derive(Debug, Default)]
pub(crate) struct GsnTracker {
    last: u64,
}

impl GsnTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget the previous GSN so the next observation establishes a new
    /// baseline. Called after a successful reconnect.
    pub fn reset(&mut self) {
        self.last = 0;
    }

    /// Record a fresh GSN. Returns `Some(gap)` if the server skipped one or
    /// more numbers since the last observation.
    ///
    /// Zero `last` means "first frame this session" — bootstrap with the
    /// observed value, never report a gap.
    pub fn observe(&mut self, gsn: u64) -> Option<GsnGap> {
        // Pulse uses 1-based GSN, so 0 is reserved for "uninitialized" both
        // here and on the wire. A 0 frame is malformed; treat it as a
        // bootstrap.
        if gsn == 0 {
            return None;
        }
        let prev = self.last;
        self.last = gsn;
        if prev == 0 {
            return None;
        }
        // Ignore duplicates / out-of-order replay (some channels emit the
        // same GSN twice on resub if the snapshot rebuild races a live
        // update). Server enforces monotonicity over a session, so a lower
        // GSN here means we already accounted for it — treat as no-op.
        if gsn <= prev {
            self.last = prev;
            return None;
        }
        if gsn > prev + 1 {
            Some(GsnGap {
                from: prev + 1,
                to: gsn - 1,
            })
        } else {
            None
        }
    }

    /// Most recent GSN; primarily for debugging / introspection.
    #[cfg(test)]
    pub fn last(&self) -> u64 {
        self.last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_observation_no_gap() {
        let mut t = GsnTracker::new();
        assert_eq!(t.observe(42), None);
        assert_eq!(t.last(), 42);
    }

    #[test]
    fn sequential_no_gap() {
        let mut t = GsnTracker::new();
        t.observe(1);
        assert_eq!(t.observe(2), None);
        assert_eq!(t.observe(3), None);
    }

    #[test]
    fn skip_emits_gap() {
        let mut t = GsnTracker::new();
        t.observe(1);
        let gap = t.observe(5);
        assert_eq!(gap, Some(GsnGap { from: 2, to: 4 }));
        assert_eq!(t.last(), 5);
    }

    #[test]
    fn duplicate_gsn_no_gap_no_advance() {
        let mut t = GsnTracker::new();
        t.observe(10);
        assert_eq!(t.observe(10), None);
        assert_eq!(t.last(), 10);
    }

    #[test]
    fn out_of_order_lower_no_gap_no_advance() {
        let mut t = GsnTracker::new();
        t.observe(10);
        assert_eq!(t.observe(5), None);
        assert_eq!(t.last(), 10);
    }

    #[test]
    fn reset_clears_baseline() {
        let mut t = GsnTracker::new();
        t.observe(100);
        t.reset();
        // After reset, the next GSN bootstraps without reporting a gap
        // even though it's well beyond the previous baseline — this is
        // the post-reconnect semantics.
        assert_eq!(t.observe(500), None);
    }

    #[test]
    fn zero_gsn_ignored() {
        let mut t = GsnTracker::new();
        assert_eq!(t.observe(0), None);
        assert_eq!(t.last(), 0);
        // And subsequent real GSN still bootstraps.
        assert_eq!(t.observe(5), None);
    }
}
