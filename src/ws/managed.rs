//! Managed WebSocket client (Phase 6).
//!
//! Wraps the Phase 5 raw [`super::connection::WsConnection`] in an auto-
//! reconnecting supervisor. The user sees a single persistent
//! [`SubscriptionStream`] per channel; underlying socket churn is invisible
//! except for the [`super::event::WsEvent::Reconnected`] /
//! [`super::event::WsEvent::Gap`] / [`super::event::WsEvent::Unauthorized`]
//! markers the supervisor injects.
//!
//! ## Lifetime
//! - [`WsClient::new`] spawns the supervisor task immediately. It sits
//!   idle until the first [`WsClient::subscribe`] / [`WsClient::authenticate`]
//!   call expresses intent to use the connection.
//! - [`WsClient::shutdown`] cleanly closes the socket and stops the
//!   supervisor. After shutdown, all subscription streams end.
//! - Dropping every [`WsClient`] handle without calling `shutdown` also
//!   stops the supervisor (cmd_rx returns `None`).
//!
//! ## Reconnect semantics
//! - On socket drop the supervisor backs off exponentially (100ms → 30s),
//!   reconnects, replays auth (if previously called), replays every active
//!   subscription, then resumes pumping frames. A
//!   [`super::event::WsEvent::Reconnected`] marker is emitted to every
//!   sub stream once per reconnect.
//! - GSN trackers reset on reconnect — pulse rejoins at the current head,
//!   not a replay, so comparing GSNs across sessions would yield spurious
//!   gaps. Callers who need byte-perfect catch-up must resync via REST.
//! - Auth replay failures degrade to public-only mode and emit
//!   [`super::event::WsEvent::Unauthorized`] on every sub. Public subs keep
//!   working.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamMap;

use crate::auth::HmacSigner;
use crate::env::Env;
use crate::error::{Error, Result};

use super::channel::{Channel, ChannelName};
use super::connection::{Subscription, WsConnection};
use super::event::{WsEvent, WsUpdate};
use super::gsn::GsnTracker;

/// Per-subscription user-facing buffer. Bounded so a stalled consumer can
/// be detected and surfaced as a `Gap` rather than backpressuring the
/// supervisor (which would block ALL subs sharing the same socket).
const SUB_USER_BUFFER: usize = 256;
/// Outbound user→supervisor command channel depth.
const CMD_BUFFER: usize = 64;
/// Probe ping cadence — short enough to detect dead sockets quickly,
/// long enough not to add measurable load.
const PROBE_INTERVAL: Duration = Duration::from_secs(15);
/// Per-attempt timeout when calling [`WsConnection::ping`] for liveness.
/// Short — we'd rather declare dead and reconnect than hang.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Routing key for the registry: `(ChannelName, filter_string)`. Filter is
/// `""` for filter-less channels (`portfolio`, `notification`).
type SubKey = (ChannelName, String);

/// Public managed WebSocket client. Cheap to clone — backed by an
/// `Arc<Handle>`; cloning shares the supervisor task.
#[derive(Clone)]
pub struct WsClient {
    inner: Arc<Handle>,
}

struct Handle {
    cmd_tx: mpsc::Sender<SupCommand>,
}

impl WsClient {
    pub(crate) fn new(env: Env, hmac: Option<HmacSigner>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel(CMD_BUFFER);
        let supervisor = Supervisor {
            env,
            hmac,
            cmd_rx,
            subs: HashMap::new(),
            auth_active: false,
            auth_address: None,
            pending_auth_acks: Vec::new(),
            shutdown: false,
        };
        tokio::spawn(supervisor.run());
        Self {
            inner: Arc::new(Handle { cmd_tx }),
        }
    }

    /// Subscribe to a channel. Returns a stream of [`WsEvent`]s that
    /// survives reconnects — internal connection swaps surface as
    /// [`WsEvent::Reconnected`] markers and any GSN gap as
    /// [`WsEvent::Gap`].
    ///
    /// Subscribing while the supervisor is mid-reconnect is safe: the
    /// channel is registered immediately, the user receiver is returned,
    /// and the actual server-side subscribe happens once the next socket
    /// is up.
    pub async fn subscribe(&self, channel: Channel) -> Result<SubscriptionStream> {
        let (ack_tx, ack_rx) = oneshot::channel();
        self.send_cmd(SupCommand::Subscribe {
            channel,
            ack: ack_tx,
        })
        .await?;
        let rx = ack_rx
            .await
            .map_err(|_| Error::Ws("supervisor dropped subscribe ack".into()))??;
        Ok(SubscriptionStream {
            inner: ReceiverStream::new(rx),
        })
    }

    /// Unsubscribe and drop the registry entry. The previously returned
    /// stream will end after any in-flight events drain.
    ///
    /// Errors only if the supervisor task is gone. A best-effort `unsub`
    /// frame is sent to the server when the connection is up; otherwise
    /// the registration is cleared locally and never replayed.
    pub async fn unsubscribe(&self, channel: Channel) -> Result<()> {
        let (ack_tx, ack_rx) = oneshot::channel();
        self.send_cmd(SupCommand::Unsubscribe {
            channel,
            ack: ack_tx,
        })
        .await?;
        ack_rx
            .await
            .map_err(|_| Error::Ws("supervisor dropped unsubscribe ack".into()))?
    }

    /// Authenticate the connection. Required before subscribing to any
    /// private channel (`order`, `position`, `portfolio`, `notification`).
    /// The credential intent is sticky — supervisor replays it on every
    /// reconnect. Returns the wallet address the server resolved.
    pub async fn authenticate(&self) -> Result<String> {
        let (ack_tx, ack_rx) = oneshot::channel();
        self.send_cmd(SupCommand::Authenticate { ack: ack_tx })
            .await?;
        ack_rx
            .await
            .map_err(|_| Error::Ws("supervisor dropped auth ack".into()))?
    }

    /// Stop the supervisor and close the socket. After this returns, every
    /// subscription stream ends and subsequent calls on this handle (or
    /// any clone) error with `connection task is gone`.
    pub async fn shutdown(self) -> Result<()> {
        let (ack_tx, ack_rx) = oneshot::channel();
        self.send_cmd(SupCommand::Shutdown { ack: ack_tx }).await?;
        ack_rx
            .await
            .map_err(|_| Error::Ws("supervisor dropped shutdown ack".into()))?;
        Ok(())
    }

    async fn send_cmd(&self, cmd: SupCommand) -> Result<()> {
        self.inner
            .cmd_tx
            .send(cmd)
            .await
            .map_err(|_| Error::Ws("supervisor task is gone".into()))
    }
}

/// Stream of [`WsEvent`]s for a single subscription. Drop to free the
/// receiver — the supervisor notices the closed channel and unsubscribes
/// server-side. Holding the stream across disconnects is safe; the
/// supervisor pumps fresh frames into it after every reconnect.
pub struct SubscriptionStream {
    inner: ReceiverStream<WsEvent>,
}

impl futures_util::Stream for SubscriptionStream {
    type Item = WsEvent;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.inner).poll_next(cx)
    }
}

/* ──── supervisor ──────────────────────────────────────────────────── */

enum SupCommand {
    Subscribe {
        channel: Channel,
        ack: oneshot::Sender<Result<mpsc::Receiver<WsEvent>>>,
    },
    Unsubscribe {
        channel: Channel,
        ack: oneshot::Sender<Result<()>>,
    },
    Authenticate {
        ack: oneshot::Sender<Result<String>>,
    },
    Shutdown {
        ack: oneshot::Sender<()>,
    },
}

/// Outcome bag for the per-replay first-ack tracking. The supervisor
/// records which keys had their first ack fired *this* connect cycle so
/// `broadcast(Reconnected)` can skip them — a brand-new sub never
/// experienced a reconnect from its caller's perspective.
#[derive(Default)]
struct ReplayMarks {
    fresh: std::collections::HashSet<SubKey>,
}

struct SubSlot {
    channel: Channel,
    user_tx: mpsc::Sender<WsEvent>,
    gsn: GsnTracker,
    /// First-subscribe ack. Held while the supervisor is mid-(re)connect or
    /// awaiting the server `subscribed` reply. Fired once the server has
    /// confirmed the subscription; cleared after — subsequent reconnects
    /// re-attach the sub silently.
    pending: Option<PendingSub>,
}

struct PendingSub {
    ack: oneshot::Sender<Result<mpsc::Receiver<WsEvent>>>,
    user_rx: mpsc::Receiver<WsEvent>,
}

struct Supervisor {
    env: Env,
    hmac: Option<HmacSigner>,
    cmd_rx: mpsc::Receiver<SupCommand>,
    /// Persistent registry — survives reconnects.
    subs: HashMap<SubKey, SubSlot>,
    /// User has called `authenticate()` at least once → replay on every
    /// reconnect.
    auth_active: bool,
    /// Cached server-resolved wallet address (most recent successful auth).
    auth_address: Option<String>,
    /// Pending caller(s) of `authenticate()` invoked while disconnected.
    /// Fired with the resolved address once the next connect+auth
    /// succeeds, or with the auth-replay error.
    pending_auth_acks: Vec<oneshot::Sender<Result<String>>>,
    shutdown: bool,
}

impl Supervisor {
    async fn run(mut self) {
        let mut backoff = Backoff::new();
        let mut first = true;
        loop {
            if self.shutdown {
                return;
            }
            // Idle until the user expresses intent. Keeps the socket
            // unopened on a freshly-built `Client::ws()` until something
            // actually needs it.
            if !self.has_intent() {
                match self.cmd_rx.recv().await {
                    Some(c) => self.handle_disconnected(c),
                    None => return, // every WsClient handle dropped
                }
                continue;
            }
            let conn = match self.connect_with_backoff(&mut backoff).await {
                ConnectOutcome::Connected(c) => c,
                ConnectOutcome::Stop => return,
            };
            // Reset trackers BEFORE any data flows — fresh session means
            // fresh GSN baseline.
            for slot in self.subs.values_mut() {
                slot.gsn.reset();
            }
            // Auth replay → if it fails, downgrade to public-only.
            if self.auth_active {
                match conn.authenticate().await {
                    Ok(addr) => {
                        self.auth_address = Some(addr.clone());
                        // Fire any callers blocked on `authenticate()` from
                        // the disconnected state.
                        for ack in self.pending_auth_acks.drain(..) {
                            let _ = ack.send(Ok(addr.clone()));
                        }
                    }
                    Err(e) => {
                        let detail = format!("{e}");
                        tracing::warn!(error = %detail, "auth replay failed");
                        self.auth_active = false;
                        self.auth_address = None;
                        // Pending callers see the failure too.
                        for ack in self.pending_auth_acks.drain(..) {
                            let _ = ack.send(Err(Error::Ws(detail.clone())));
                        }
                        let empty = ReplayMarks::default();
                        self.broadcast(WsEvent::Unauthorized(detail), &empty).await;
                    }
                }
            }
            // GC: cancel any pending sub whose caller has already dropped
            // their `subscribe().await` future. Without this, an orphaned
            // slot would block future `subscribe(same_channel)` calls
            // forever (the registry would still report "already
            // subscribed"). C2 in code-review.
            self.gc_orphan_pending();
            // Sub replay — server validation failures drop that sub; a
            // mid-replay connection death does NOT (we leave the registry
            // intact and let the outer loop reconnect, otherwise a single
            // flaky reconnect would permanently lose every still-pending
            // sub past the failure point).
            let mut streams: StreamMap<SubKey, Subscription> = StreamMap::new();
            let mut marks = ReplayMarks::default();
            // Set when we detect the freshly-acquired conn died mid-replay
            // (`conn.subscribe()` returns "connection task is gone"). We
            // bail out of the for-loop, skip the Reconnected broadcast for
            // this aborted cycle, and let the outer loop reconnect with
            // every un-replayed sub still in `self.subs`.
            let mut conn_died_during_replay = false;
            let keys: Vec<SubKey> = self.subs.keys().cloned().collect();
            for key in keys {
                let Some(slot) = self.subs.get(&key) else {
                    continue;
                };
                match conn.subscribe(slot.channel.clone()).await {
                    Ok(stream) => {
                        streams.insert(key.clone(), stream);
                        // First-time subscribe ack fires NOW — caller has
                        // been blocked on `subscribe().await` since the
                        // command was registered. Subsequent reconnects
                        // see `pending = None` and silently re-attach.
                        // If the caller dropped their future
                        // (ack.is_closed), GC the slot to prevent the
                        // C2 leak.
                        let mut needs_gc = false;
                        if let Some(slot) = self.subs.get_mut(&key) {
                            if let Some(p) = slot.pending.take() {
                                if p.ack.is_closed() {
                                    needs_gc = true;
                                } else {
                                    let _ = p.ack.send(Ok(p.user_rx));
                                    marks.fresh.insert(key.clone());
                                }
                            }
                        }
                        if needs_gc {
                            let ch = self.subs.remove(&key).map(|s| s.channel);
                            streams.remove(&key);
                            if let Some(c) = ch {
                                let _ = conn.unsubscribe(c).await;
                            }
                        }
                    }
                    Err(e) if is_conn_gone(&e) => {
                        // Conn died between connect_with_backoff returning
                        // and now. Leave the slot in the registry so the
                        // next reconnect re-tries it. Pending first-time
                        // subscribers stay parked — they're conceptually
                        // still "subscribing", not failed.
                        tracing::info!(
                            ?key,
                            error = %e,
                            "ws conn died mid-replay; will retry on next reconnect",
                        );
                        conn_died_during_replay = true;
                        break;
                    }
                    Err(e) => {
                        // Server-side rejection: auth-required private sub
                        // on a now-public connection, validation error,
                        // etc. Drop the sub from the registry — replaying
                        // it would just fail the same way.
                        let detail = format!("resubscribe failed: {e}");
                        tracing::warn!(?key, error = %detail, "resub failed; dropping sub");
                        if let Some(slot) = self.subs.remove(&key) {
                            if let Some(p) = slot.pending {
                                let _ = p.ack.send(Err(Error::Ws(detail)));
                            } else {
                                let _ = slot.user_tx.send(WsEvent::Unauthorized(detail)).await;
                            }
                        }
                    }
                }
            }
            if conn_died_during_replay {
                // Skip Reconnected broadcast (caller would see Reconnected
                // followed immediately by another Reconnected once the
                // *real* reconnect completes). Don't reset backoff — the
                // conn proved unstable. Loop back and reconnect.
                drop(conn);
                continue;
            }
            backoff.reset();
            if !first {
                // Skip subs that JUST got their first ack this cycle —
                // from the caller's POV they didn't experience a
                // reconnect, they just freshly subscribed.
                self.broadcast(WsEvent::Reconnected, &marks).await;
            }
            first = false;
            // Drive until the connection drops or shutdown is requested.
            match self.drive(&conn, &mut streams).await {
                DriveExit::Shutdown => {
                    conn.close().await;
                    return;
                }
                DriveExit::ConnDropped => {
                    // Loop and reconnect.
                    drop(conn);
                }
            }
        }
    }

    fn has_intent(&self) -> bool {
        !self.subs.is_empty() || self.auth_active
    }

    async fn connect_with_backoff(&mut self, backoff: &mut Backoff) -> ConnectOutcome {
        loop {
            if self.shutdown {
                return ConnectOutcome::Stop;
            }
            match WsConnection::connect_raw(self.env.ws_url(), self.hmac.clone()).await {
                Ok(conn) => return ConnectOutcome::Connected(conn),
                Err(e) => {
                    let delay = backoff.next();
                    tracing::warn!(
                        error = %e,
                        delay_ms = delay.as_millis() as u64,
                        "ws connect failed; backing off"
                    );
                    let sleep = tokio::time::sleep(delay);
                    tokio::pin!(sleep);
                    loop {
                        tokio::select! {
                            _ = &mut sleep => break,
                            cmd = self.cmd_rx.recv() => {
                                match cmd {
                                    Some(c) => self.handle_disconnected(c),
                                    None => return ConnectOutcome::Stop,
                                }
                                if self.shutdown {
                                    return ConnectOutcome::Stop;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Handle a command while no active connection exists. Subscribe and
    /// authenticate register intent that the connect-loop will fulfil;
    /// their acks fire only once the server confirms.
    fn handle_disconnected(&mut self, cmd: SupCommand) {
        match cmd {
            SupCommand::Subscribe { channel, ack } => {
                self.gc_orphan_pending();
                let key = (channel.name(), channel.filter().to_string());
                if let Some(existing) = self.subs.get(&key) {
                    let msg = if existing.pending.is_some() {
                        format!("subscription request in flight for {key:?}")
                    } else {
                        format!("already subscribed to {key:?}")
                    };
                    let _ = ack.send(Err(Error::Ws(msg)));
                    return;
                }
                let (user_tx, user_rx) = mpsc::channel(SUB_USER_BUFFER);
                self.subs.insert(
                    key,
                    SubSlot {
                        channel,
                        user_tx,
                        gsn: GsnTracker::new(),
                        pending: Some(PendingSub { ack, user_rx }),
                    },
                );
            }
            SupCommand::Unsubscribe { channel, ack } => {
                let key = (channel.name(), channel.filter().to_string());
                if let Some(slot) = self.subs.remove(&key) {
                    // If a sub was pending and now gets cancelled, fail its
                    // ack so the caller doesn't hang on the oneshot.
                    if let Some(p) = slot.pending {
                        let _ = p.ack.send(Err(Error::Ws("subscription cancelled".into())));
                    }
                }
                let _ = ack.send(Ok(()));
            }
            SupCommand::Authenticate { ack } => {
                if self.hmac.is_none() {
                    let _ = ack.send(Err(Error::Ws(
                        "authenticate requires api_key on the Client".into(),
                    )));
                    return;
                }
                // Record intent + park the ack. The connect-loop will fire
                // it (with the resolved address) the next time auth replay
                // succeeds. If the user shuts down or the supervisor exits
                // first, `fail_all_pending` returns an err so the caller
                // doesn't hang on the oneshot.
                self.auth_active = true;
                self.pending_auth_acks.push(ack);
            }
            SupCommand::Shutdown { ack } => {
                self.shutdown = true;
                self.fail_all_pending(Error::Ws("shutdown".into()));
                let _ = ack.send(());
            }
        }
    }

    /// Fire every still-pending ack (subscribe + authenticate) with
    /// `err`. Used on shutdown / supervisor exit so callers blocked in
    /// `subscribe().await` or `authenticate().await` don't hang forever.
    fn fail_all_pending(&mut self, err: Error) {
        let keys: Vec<SubKey> = self.subs.keys().cloned().collect();
        for key in keys {
            if let Some(slot) = self.subs.get_mut(&key) {
                if let Some(p) = slot.pending.take() {
                    let _ = p.ack.send(Err(Error::Ws(format!("{err}"))));
                    // No point keeping a sub the caller never received the
                    // receiver for — drop it.
                    self.subs.remove(&key);
                }
            }
        }
        for ack in self.pending_auth_acks.drain(..) {
            let _ = ack.send(Err(Error::Ws(format!("{err}"))));
        }
    }

    async fn drive(
        &mut self,
        conn: &WsConnection,
        streams: &mut StreamMap<SubKey, Subscription>,
    ) -> DriveExit {
        let mut probe = tokio::time::interval(PROBE_INTERVAL);
        // First tick fires immediately; skip it so the first probe lands
        // PROBE_INTERVAL after `drive` starts.
        probe.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        probe.tick().await;
        loop {
            tokio::select! {
                biased; // user commands first — they're rarer than data
                cmd = self.cmd_rx.recv() => {
                    match cmd {
                        Some(c) => {
                            if let Some(exit) = self.handle_connected(c, conn, streams).await {
                                return exit;
                            }
                        }
                        None => return DriveExit::Shutdown,
                    }
                }
                _ = conn.closed() => {
                    // Driver task exited (peer close, read/send error, or
                    // explicit close). Reconnect immediately — don't wait
                    // for the probe.
                    tracing::info!("ws driver exited; reconnecting");
                    return DriveExit::ConnDropped;
                }
                Some((key, update)) = streams.next() => {
                    if let Some(exit) = self.route_update(key, update, conn).await {
                        return exit;
                    }
                }
                _ = probe.tick() => {
                    // Bound the ping so a wedged socket doesn't lock us out
                    // of the reconnect path. `closed()` covers most drops;
                    // the probe is the safety net for half-open sockets
                    // (TCP keepalive hasn't fired yet, peer never sent
                    // close).
                    match tokio::time::timeout(PROBE_TIMEOUT, conn.ping()).await {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => {
                            tracing::info!(error = %e, "ws probe ping failed; reconnecting");
                            return DriveExit::ConnDropped;
                        }
                        Err(_) => {
                            tracing::info!("ws probe ping timeout; reconnecting");
                            return DriveExit::ConnDropped;
                        }
                    }
                }
            }
        }
    }

    async fn handle_connected(
        &mut self,
        cmd: SupCommand,
        conn: &WsConnection,
        streams: &mut StreamMap<SubKey, Subscription>,
    ) -> Option<DriveExit> {
        match cmd {
            SupCommand::Subscribe { channel, ack } => {
                self.gc_orphan_pending();
                let key = (channel.name(), channel.filter().to_string());
                if let Some(existing) = self.subs.get(&key) {
                    let msg = if existing.pending.is_some() {
                        format!("subscription request in flight for {key:?}")
                    } else {
                        format!("already subscribed to {key:?}")
                    };
                    let _ = ack.send(Err(Error::Ws(msg)));
                    return None;
                }
                match conn.subscribe(channel.clone()).await {
                    Ok(stream) => {
                        let (user_tx, user_rx) = mpsc::channel(SUB_USER_BUFFER);
                        self.subs.insert(
                            key.clone(),
                            SubSlot {
                                channel,
                                user_tx,
                                gsn: GsnTracker::new(),
                                pending: None,
                            },
                        );
                        streams.insert(key, stream);
                        let _ = ack.send(Ok(user_rx));
                    }
                    Err(e) => {
                        // Could be "connection task is gone" — treat as
                        // disconnect, register intent for next reconnect,
                        // hand the user a stream that begins on reconnect.
                        if is_conn_gone(&e) {
                            let (user_tx, user_rx) = mpsc::channel(SUB_USER_BUFFER);
                            self.subs.insert(
                                key,
                                SubSlot {
                                    channel,
                                    user_tx,
                                    gsn: GsnTracker::new(),
                                    pending: Some(PendingSub { ack, user_rx }),
                                },
                            );
                            return Some(DriveExit::ConnDropped);
                        }
                        let _ = ack.send(Err(e));
                    }
                }
            }
            SupCommand::Unsubscribe { channel, ack } => {
                let key = (channel.name(), channel.filter().to_string());
                streams.remove(&key);
                if let Some(slot) = self.subs.remove(&key) {
                    if let Some(p) = slot.pending {
                        let _ = p.ack.send(Err(Error::Ws("subscription cancelled".into())));
                    }
                }
                // Best-effort wire unsub — if the conn is dead we ignore
                // the error since the supervisor already cleared local
                // state.
                match conn.unsubscribe(channel).await {
                    Ok(()) => {
                        let _ = ack.send(Ok(()));
                    }
                    Err(e) if is_conn_gone(&e) => {
                        let _ = ack.send(Ok(()));
                        return Some(DriveExit::ConnDropped);
                    }
                    Err(e) => {
                        let _ = ack.send(Err(e));
                    }
                }
            }
            SupCommand::Authenticate { ack } => {
                if self.hmac.is_none() {
                    let _ = ack.send(Err(Error::Ws(
                        "authenticate requires api_key on the Client".into(),
                    )));
                    return None;
                }
                match conn.authenticate().await {
                    Ok(addr) => {
                        self.auth_active = true;
                        self.auth_address = Some(addr.clone());
                        let _ = ack.send(Ok(addr));
                    }
                    Err(e) if is_conn_gone(&e) => {
                        // Park the ack alongside the disconnected-state
                        // pending list — connect-loop fires it after the
                        // next successful auth replay.
                        self.auth_active = true;
                        self.pending_auth_acks.push(ack);
                        return Some(DriveExit::ConnDropped);
                    }
                    Err(e) => {
                        let _ = ack.send(Err(e));
                    }
                }
            }
            SupCommand::Shutdown { ack } => {
                self.shutdown = true;
                self.fail_all_pending(Error::Ws("shutdown".into()));
                let _ = ack.send(());
                return Some(DriveExit::Shutdown);
            }
        }
        None
    }

    async fn route_update(
        &mut self,
        key: SubKey,
        update: WsUpdate,
        conn: &WsConnection,
    ) -> Option<DriveExit> {
        let Some(slot) = self.subs.get_mut(&key) else {
            // Sub was unsubscribed locally but a stale frame arrived;
            // drop it.
            return None;
        };
        // GSN check first so the gap marker lands BEFORE the update that
        // tripped it — gives consumers a chance to react before they see
        // the post-gap data.
        if let Some(gap) = slot.gsn.observe(update.gsn) {
            // try_send avoids blocking the supervisor on a slow consumer.
            // If the buffer is full, treat the consumer as gone — sending
            // a gap into a buffer they're not reading is pointless.
            if slot
                .user_tx
                .try_send(WsEvent::Gap {
                    from: gap.from,
                    to: gap.to,
                })
                .is_err()
            {
                return self.drop_sub(&key, conn).await;
            }
        }
        // Update path also try_send: blocking await here would back up
        // the entire supervisor (cmd loop, other subs, conn.closed()
        // detection) on the slowest consumer. Phase doc Risk Assessment
        // mentions "drop-oldest policy" but tokio mpsc lacks that — we
        // drop the sub on Full and let the caller see the stream end.
        // They can resubscribe to start fresh.
        match slot.user_tx.try_send(WsEvent::Update(update)) {
            Ok(()) => None,
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!(?key, "ws subscriber buffer full; dropping sub");
                self.drop_sub(&key, conn).await
            }
            Err(mpsc::error::TrySendError::Closed(_)) => self.drop_sub(&key, conn).await,
        }
    }

    async fn drop_sub(&mut self, key: &SubKey, conn: &WsConnection) -> Option<DriveExit> {
        let slot = self.subs.remove(key)?;
        match conn.unsubscribe(slot.channel).await {
            Ok(()) => None,
            Err(e) if is_conn_gone(&e) => Some(DriveExit::ConnDropped),
            Err(e) => {
                tracing::warn!(?key, error = %e, "best-effort unsub failed");
                None
            }
        }
    }

    async fn broadcast(&mut self, event: WsEvent, skip: &ReplayMarks) {
        let keys: Vec<SubKey> = self.subs.keys().cloned().collect();
        for key in keys {
            // Skip subs that just got their first ack this cycle — from
            // the caller's POV this isn't a reconnect.
            if skip.fresh.contains(&key) {
                continue;
            }
            let Some(slot) = self.subs.get(&key) else {
                continue;
            };
            // Pending subs have never delivered a first event to the
            // caller; broadcasting lifecycle markers to a buffer the
            // caller hasn't received the receiver for is misleading.
            if slot.pending.is_some() {
                continue;
            }
            // try_send so a stalled consumer doesn't block the rest of the
            // broadcast. Reconnected/Unauthorized are informational; if
            // the consumer is too far behind to receive them, they'll see
            // it implicitly via the next Gap.
            if let Err(mpsc::error::TrySendError::Closed(_)) = slot.user_tx.try_send(event.clone())
            {
                self.subs.remove(&key);
            }
        }
    }

    /// Drop slots whose user-side handle is gone:
    /// - pending sub whose first-time caller dropped the
    ///   `subscribe().await` future (oneshot sender closed)
    /// - established sub whose caller dropped the `SubscriptionStream`
    ///   (mpsc receiver closed)
    ///
    /// Without this, a dropped handle pins the SubKey in the registry
    /// forever and blocks future `subscribe(same_channel)` calls.
    /// Called before every replay loop and before processing `Subscribe`
    /// commands so the registry doesn't accumulate ghosts. Server-side
    /// unsub is best-effort and happens on the next data frame for that
    /// channel via `route_update`'s try_send-Closed path; this GC only
    /// reclaims the local slot.
    fn gc_orphan_pending(&mut self) {
        let stale: Vec<SubKey> = self
            .subs
            .iter()
            .filter_map(|(k, s)| match &s.pending {
                Some(p) if p.ack.is_closed() => Some(k.clone()),
                None if s.user_tx.is_closed() => Some(k.clone()),
                _ => None,
            })
            .collect();
        for k in stale {
            self.subs.remove(&k);
        }
    }
}

/// Outcome of a connect attempt — connect either succeeded or the
/// supervisor was asked to stop while waiting.
enum ConnectOutcome {
    Connected(WsConnection),
    Stop,
}

#[derive(PartialEq, Eq)]
enum DriveExit {
    Shutdown,
    ConnDropped,
}

/// `WsConnection` ops return `Error::Ws("connection task is gone")` when
/// the underlying driver has exited. We distinguish that from real
/// protocol errors so we can transition into reconnect rather than
/// surfacing it to the user.
fn is_conn_gone(e: &Error) -> bool {
    match e {
        Error::Ws(s) => s.contains("connection task is gone") || s.contains("connection closed"),
        _ => false,
    }
}

/* ──── backoff ──────────────────────────────────────────────────────── */

/// Exponential backoff with bounded jitter. 100ms base doubles up to ~25s,
/// capped at 30s including jitter. Each [`Backoff`] holds its own xorshift
/// state seeded from a process-unique entropy source so a fleet of SDK
/// clients restarting against a recovering server doesn't synchronize on
/// identical jitter (which `subsec_nanos`-based jitter does — multiple
/// processes restarting in the same wall-clock millisecond would compute
/// the same offset).
struct Backoff {
    n: u32,
    /// xorshift64* state — must be non-zero (xorshift produces all zeros
    /// from a zero seed).
    rng: u64,
}

impl Backoff {
    const BASE_MS: u64 = 100;
    const CAP_MS: u64 = 30_000;
    const MAX_SHIFT: u32 = 8; // 100ms * 2^8 = 25.6s; one more puts us at cap

    fn new() -> Self {
        Self {
            n: 0,
            rng: seed_for_backoff(),
        }
    }

    fn next(&mut self) -> Duration {
        let shift = self.n.min(Self::MAX_SHIFT);
        let target = Self::BASE_MS
            .saturating_mul(1u64 << shift)
            .min(Self::CAP_MS);
        let jitter = self.next_jitter(target / 4 + 1);
        self.n = self.n.saturating_add(1);
        Duration::from_millis(target.saturating_add(jitter))
    }

    fn reset(&mut self) {
        self.n = 0;
    }

    fn next_jitter(&mut self, span: u64) -> u64 {
        // xorshift64 — passes basic statistical tests, trivial to embed,
        // good enough for de-synchronizing reconnects. Not crypto.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        if span == 0 {
            0
        } else {
            self.rng % span
        }
    }
}

/// Per-instance seed mixing wall-clock nanos with a stack address.
/// Two `Backoff::new()` calls in the same process get different seeds
/// (different stack frames); two processes started simultaneously
/// diverge via ASLR. Avoids pulling the `rand` crate.
fn seed_for_backoff() -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut h = DefaultHasher::new();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    h.write_u64(nanos);
    let local = 0u8;
    h.write_usize(&local as *const _ as usize);
    let s = h.finish();
    if s == 0 {
        1
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_caps() {
        let mut b = Backoff::new();
        let d1 = b.next();
        let d2 = b.next();
        // Allow for jitter — assert the floor.
        assert!(d1.as_millis() as u64 >= Backoff::BASE_MS);
        assert!(d2.as_millis() as u64 >= Backoff::BASE_MS * 2);
        for _ in 0..20 {
            let _ = b.next();
        }
        let dn = b.next();
        // After many doublings we must be at or near the cap (allow
        // jitter overshoot of up to 25%).
        let max_with_jitter = Backoff::CAP_MS + Backoff::CAP_MS / 4 + 1;
        assert!(dn.as_millis() as u64 <= max_with_jitter);
    }

    #[test]
    fn backoff_reset_returns_to_base() {
        let mut b = Backoff::new();
        for _ in 0..5 {
            b.next();
        }
        b.reset();
        let d = b.next();
        assert!((d.as_millis() as u64) < Backoff::BASE_MS * 4);
    }

    #[test]
    fn is_conn_gone_matches_known_strings() {
        assert!(is_conn_gone(&Error::Ws("connection task is gone".into())));
        assert!(is_conn_gone(&Error::Ws("connection closed".into())));
        assert!(!is_conn_gone(&Error::Ws(
            "server error: bad request".into()
        )));
        assert!(!is_conn_gone(&Error::Auth("nope".into())));
    }
}
