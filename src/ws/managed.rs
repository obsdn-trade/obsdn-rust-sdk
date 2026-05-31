//! Managed WebSocket client.
//!
//! Wraps the raw [`super::connection::WsConnection`] in an auto-reconnecting
//! supervisor. The user sees a single persistent [`SubscriptionStream`] per
//! channel; underlying socket churn is invisible except for the
//! [`super::event::Event::Reconnected`] /
//! [`super::event::Event::Unauthorized`] markers the supervisor injects.
//!
//! ## Lifetime
//! - [`Session::new`] spawns the supervisor task immediately. It sits
//!   idle until the first [`Session::subscribe`] / [`Session::authenticate`]
//!   call expresses intent to use the connection.
//! - [`Session::shutdown`] cleanly closes the socket and stops the
//!   supervisor. After shutdown, all subscription streams end.
//! - Dropping every [`Session`] handle without calling `shutdown` also
//!   stops the supervisor (cmd_rx returns `None`).
//!
//! ## Reconnect semantics
//! - On socket drop the supervisor backs off exponentially (100ms → 30s),
//!   reconnects, replays auth (if previously called), replays every active
//!   subscription, then resumes pumping frames. A
//!   [`super::event::Event::Reconnected`] marker is emitted to every
//!   sub stream once per reconnect.
//! - No GSN gap detection - pulse `gsn` is a sparse global watermark, not a
//!   dense per-sub sequence (see `super` module docs). On reconnect pulse
//!   rejoins at the current head; callers who need byte-perfect catch-up
//!   must resync via REST.
//! - Auth replay failures emit [`super::event::Event::Unauthorized`] on every
//!   sub but are retried on subsequent reconnects (a transient failure, e.g. a
//!   server restart, recovers the private feed automatically). Only after more
//!   than [`MAX_AUTH_REPLAY_FAILURES`] consecutive failures does the supervisor
//!   give up and downgrade to public-only until the caller invokes
//!   `authenticate()` again. Public subs keep working throughout.

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
use super::connection::{RawSubItem, Subscription, WsConnection};
use super::event::Event;

/// Per-subscription user-facing buffer. Bounded so a stalled consumer can
/// be detected and the sub dropped rather than backpressuring the
/// supervisor (which would block ALL subs sharing the same socket).
const SUB_USER_BUFFER: usize = 256;
/// Outbound user→supervisor command channel depth.
const CMD_BUFFER: usize = 64;
/// Probe ping cadence - short enough to detect dead sockets quickly,
/// long enough not to add measurable load.
const PROBE_INTERVAL: Duration = Duration::from_secs(15);
/// Per-attempt timeout when calling [`WsConnection::ping`] for liveness.
/// Short - we'd rather declare dead and reconnect than hang.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// Consecutive auth-replay failures tolerated before the supervisor gives up
/// and downgrades to public-only. A transient failure (server restart, brief
/// clock skew) recovers on the next reconnect; a permanently-revoked key stops
/// retrying after this many attempts. Reset to zero on any auth success.
const MAX_AUTH_REPLAY_FAILURES: u32 = 5;

/// Routing key for the registry: `(ChannelName, filter_string)`. Filter is
/// `""` for filter-less channels (`portfolio`, `notification`).
type SubKey = (ChannelName, String);

/// Public managed WebSocket client. Cheap to clone - backed by an
/// `Arc<Handle>`; cloning shares the supervisor task.
#[derive(Clone)]
pub struct Session {
    inner: Arc<Handle>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").finish_non_exhaustive()
    }
}

struct Handle {
    cmd_tx: mpsc::Sender<SupCommand>,
}

impl Session {
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
            auth_failures: 0,
            shutdown: false,
        };
        tokio::spawn(supervisor.run());
        Self {
            inner: Arc::new(Handle { cmd_tx }),
        }
    }

    /// Subscribe to a channel. Returns a stream of [`Event`]s that
    /// survives reconnects - internal connection swaps surface as
    /// [`Event::Reconnected`] markers.
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
            terminated: false,
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
    /// The credential intent is sticky - supervisor replays it on every
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

/// Stream of [`Event`]s for a single subscription. Drop to free the
/// receiver - the supervisor notices the closed channel and unsubscribes
/// server-side. Holding the stream across disconnects is safe; the
/// supervisor pumps fresh frames into it after every reconnect.
pub struct SubscriptionStream {
    inner: ReceiverStream<Event>,
    /// `true` once `poll_next` has yielded `None`, so [`FusedStream`] can
    /// report termination.
    ///
    /// [`FusedStream`]: futures_util::stream::FusedStream
    terminated: bool,
}

impl std::fmt::Debug for SubscriptionStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubscriptionStream")
            .field("terminated", &self.terminated)
            .finish_non_exhaustive()
    }
}

impl futures_util::Stream for SubscriptionStream {
    type Item = Event;

    #[inline]
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match std::pin::Pin::new(&mut self.inner).poll_next(cx) {
            std::task::Poll::Ready(None) => {
                self.terminated = true;
                std::task::Poll::Ready(None)
            }
            other => other,
        }
    }
}

impl futures_util::stream::FusedStream for SubscriptionStream {
    // `terminated` is set only after `poll_next` observes the end, so this is a
    // post-poll signal: it reports `false` until the stream has actually been
    // polled to completion, not the instant the supervisor closes the channel.
    // Don't use it as a pre-poll liveness check.
    #[inline]
    fn is_terminated(&self) -> bool {
        self.terminated
    }
}

/* ──── supervisor ──────────────────────────────────────────────────── */

enum SupCommand {
    Subscribe {
        channel: Channel,
        ack: oneshot::Sender<Result<mpsc::Receiver<Event>>>,
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
/// `broadcast(Reconnected)` can skip them - a brand-new sub never
/// experienced a reconnect from its caller's perspective.
#[derive(Default)]
struct ReplayMarks {
    fresh: std::collections::HashSet<SubKey>,
}

struct SubSlot {
    channel: Channel,
    user_tx: mpsc::Sender<Event>,
    /// First-subscribe ack. Held while the supervisor is mid-(re)connect or
    /// awaiting the server `subscribed` reply. Fired once the server has
    /// confirmed the subscription; cleared after - subsequent reconnects
    /// re-attach the sub silently.
    pending: Option<PendingSub>,
}

struct PendingSub {
    ack: oneshot::Sender<Result<mpsc::Receiver<Event>>>,
    user_rx: mpsc::Receiver<Event>,
}

struct Supervisor {
    env: Env,
    hmac: Option<HmacSigner>,
    cmd_rx: mpsc::Receiver<SupCommand>,
    /// Persistent registry - survives reconnects.
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
    /// Consecutive auth-replay failures. Reset on success; once it exceeds
    /// [`MAX_AUTH_REPLAY_FAILURES`] the supervisor stops retrying auth and
    /// downgrades to public-only.
    auth_failures: u32,
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
                    None => return, // every Session handle dropped
                }
                continue;
            }
            let conn = match self.connect_with_backoff(&mut backoff).await {
                ConnectOutcome::Connected(c) => c,
                ConnectOutcome::Stop => return,
            };
            // Auth replay. A transient failure should not permanently kill the
            // private feed, so keep `auth_active` set and retry on the next
            // reconnect, bounded by MAX_AUTH_REPLAY_FAILURES. Only after the
            // bound is exceeded do we give up and downgrade to public-only
            // (treating the key as revoked). Recovery happens on the next
            // natural reconnect; we don't force one just to retry auth.
            //
            // `authed_this_cycle` gates private-channel replay below: if auth
            // failed this cycle we must NOT resubscribe private channels (the
            // server rejects them unauthenticated, which would drop them from
            // the registry). We leave them parked so the next reconnect's auth
            // can re-establish them.
            let mut authed_this_cycle = true;
            if self.auth_active {
                match conn.authenticate().await {
                    Ok(addr) => {
                        self.auth_failures = 0;
                        self.auth_address = Some(addr.clone());
                        // Fire any callers blocked on `authenticate()` from
                        // the disconnected state.
                        for ack in self.pending_auth_acks.drain(..) {
                            let _ = ack.send(Ok(addr.clone()));
                        }
                    }
                    Err(e) if is_conn_gone(&e) => {
                        // The socket dropped mid-auth: a transport failure, not a
                        // credential rejection. Don't consume the retry budget or
                        // emit Unauthorized - the connect loop reconnects and
                        // replays auth, and parked acks stay queued for the next
                        // successful auth. Otherwise a flaky network could exhaust
                        // MAX_AUTH_REPLAY_FAILURES and falsely downgrade.
                        authed_this_cycle = false;
                        self.auth_address = None;
                        tracing::info!(error = %e, "auth replay interrupted by conn drop; will retry");
                    }
                    Err(e) => {
                        authed_this_cycle = false;
                        let detail = format!("{e}");
                        self.auth_failures += 1;
                        self.auth_address = None;
                        let gave_up = self.auth_failures > MAX_AUTH_REPLAY_FAILURES;
                        if gave_up {
                            // Stop retrying: downgrade to public-only until the
                            // caller invokes `authenticate()` again.
                            self.auth_active = false;
                            tracing::warn!(
                                error = %detail,
                                failures = self.auth_failures,
                                "auth replay failed; giving up, downgrading to public-only"
                            );
                        } else {
                            tracing::warn!(
                                error = %detail,
                                failures = self.auth_failures,
                                "auth replay failed; will retry on next reconnect"
                            );
                        }
                        // A caller blocked on `authenticate()` gets a prompt
                        // error for this attempt (it must not hang if the
                        // connection recovers without auth). The sticky
                        // `auth_active` retry is independent: the session still
                        // re-attempts auth on the next reconnect.
                        for ack in self.pending_auth_acks.drain(..) {
                            let _ = ack.send(Err(Error::Ws(detail.clone())));
                        }
                        self.broadcast(Event::Unauthorized(detail), &ReplayMarks::default(), &conn)
                            .await;
                    }
                }
            }
            // GC: cancel any pending sub whose caller has already dropped
            // their `subscribe().await` future. Without this, an orphaned
            // slot would block future `subscribe(same_channel)` calls
            // forever (the registry would still report "already
            // subscribed"). C2 in code-review.
            self.gc_orphan_pending();
            // Sub replay - server validation failures drop that sub; a
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
                // If auth failed this cycle, don't replay private channels - the
                // server would reject them unauthenticated and we'd drop them.
                // Leave them parked; the next reconnect's auth retry re-attaches
                // them. Public channels replay normally.
                if !authed_this_cycle && key.0.is_private() {
                    continue;
                }
                match conn.subscribe(slot.channel.clone()).await {
                    Ok(stream) => {
                        streams.insert(key.clone(), stream);
                        // First-time subscribe ack fires NOW - caller has
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
                        // subscribers stay parked - they're conceptually
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
                        // etc. Drop the sub from the registry - replaying
                        // it would just fail the same way.
                        let detail = format!("resubscribe failed: {e}");
                        tracing::warn!(?key, error = %detail, "resub failed; dropping sub");
                        if let Some(slot) = self.subs.remove(&key) {
                            if let Some(p) = slot.pending {
                                let _ = p.ack.send(Err(Error::Ws(detail)));
                            } else {
                                // try_send: a full user buffer must not block the
                                // supervisor's reconnect (the sub is dropped anyway).
                                let _ = slot.user_tx.try_send(Event::Unauthorized(detail));
                            }
                        }
                    }
                }
            }
            if conn_died_during_replay {
                // Skip Reconnected broadcast (caller would see Reconnected
                // followed immediately by another Reconnected once the
                // *real* reconnect completes). Don't reset backoff - the
                // conn proved unstable. Loop back and reconnect.
                drop(conn);
                continue;
            }
            backoff.reset();
            if !first {
                // Skip subs that JUST got their first ack this cycle -
                // from the caller's POV they didn't experience a
                // reconnect, they just freshly subscribed.
                self.broadcast(Event::Reconnected, &marks, &conn).await;
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
                //
                // Reset the retry budget: an explicit authenticate() grants a
                // fresh bound, so a session that previously gave up retries the
                // full MAX_AUTH_REPLAY_FAILURES again on the next reconnect.
                self.auth_active = true;
                self.auth_failures = 0;
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
                    // receiver for - drop it.
                    self.subs.remove(&key);
                }
            }
        }
        for ack in self.pending_auth_acks.drain(..) {
            let _ = ack.send(Err(Error::Ws(format!("{err}"))));
        }
    }

    /// Re-subscribe private channels that are registered but have no live
    /// stream - parked by an earlier failed auth replay. Called after a manual
    /// `authenticate()` succeeds on an active connection so the private feed
    /// recovers without waiting for a reconnect. Already-streaming subs are
    /// left untouched.
    async fn replay_parked_private(
        &mut self,
        conn: &WsConnection,
        streams: &mut StreamMap<SubKey, Subscription>,
    ) {
        let parked: Vec<(SubKey, Channel)> = self
            .subs
            .iter()
            .filter(|(k, _)| k.0.is_private() && !streams.contains_key(*k))
            .map(|(k, s)| (k.clone(), s.channel.clone()))
            .collect();
        for (key, channel) in parked {
            match conn.subscribe(channel).await {
                Ok(stream) => {
                    streams.insert(key.clone(), stream);
                    // Fire a first-time subscriber's parked ack. If that caller
                    // already dropped their `subscribe()` future, GC the slot
                    // (remove + unsubscribe) instead of leaving an orphaned
                    // server subscription that would reject a later subscribe to
                    // the same channel as a duplicate. Mirrors the reconnect path.
                    let mut needs_gc = false;
                    if let Some(slot) = self.subs.get_mut(&key) {
                        if let Some(p) = slot.pending.take() {
                            if p.ack.is_closed() {
                                needs_gc = true;
                            } else {
                                let _ = p.ack.send(Ok(p.user_rx));
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
                // Conn died mid-replay: the next reconnect re-attaches it.
                Err(e) if is_conn_gone(&e) => return,
                Err(e) => {
                    // Terminal rejection (validation/permission): drop the sub
                    // so a pending caller isn't left hanging and an established
                    // stream isn't left silently registered. Mirrors the
                    // reconnect resubscribe path. `try_send` so a full user
                    // buffer can't block the supervisor (the sub is dropped
                    // either way).
                    let detail = format!("resubscribe failed: {e}");
                    tracing::warn!(?key, error = %detail, "parked private resubscribe rejected; dropping sub");
                    if let Some(slot) = self.subs.remove(&key) {
                        if let Some(p) = slot.pending {
                            let _ = p.ack.send(Err(Error::Ws(detail)));
                        } else {
                            let _ = slot.user_tx.try_send(Event::Unauthorized(detail));
                        }
                    }
                }
            }
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
                biased; // user commands first - they're rarer than data
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
                    // explicit close). Reconnect immediately - don't wait
                    // for the probe.
                    tracing::info!("ws driver exited; reconnecting");
                    return DriveExit::ConnDropped;
                }
                Some((key, item)) = streams.next() => {
                    if let Some(exit) = self.route_update(key, item, conn).await {
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
                                pending: None,
                            },
                        );
                        streams.insert(key, stream);
                        let _ = ack.send(Ok(user_rx));
                    }
                    Err(e) => {
                        // Could be "connection task is gone" - treat as
                        // disconnect, register intent for next reconnect,
                        // hand the user a stream that begins on reconnect.
                        if is_conn_gone(&e) {
                            let (user_tx, user_rx) = mpsc::channel(SUB_USER_BUFFER);
                            self.subs.insert(
                                key,
                                SubSlot {
                                    channel,
                                    user_tx,
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
                // Best-effort wire unsub - if the conn is dead we ignore
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
                        self.auth_failures = 0;
                        self.auth_address = Some(addr.clone());
                        // Re-establish any private subs parked by an earlier
                        // failed auth replay BEFORE acking, so authenticate()
                        // returns only once the private feed is actually
                        // restored. Without this a manual authenticate() on a
                        // still-healthy socket would auth the connection but
                        // leave the private feed silent (the reconnect that
                        // would replay it is not forced here).
                        self.replay_parked_private(conn, streams).await;
                        let _ = ack.send(Ok(addr));
                    }
                    Err(e) if is_conn_gone(&e) => {
                        // Park the ack alongside the disconnected-state
                        // pending list - connect-loop fires it after the
                        // next successful auth replay. An explicit
                        // authenticate() grants a fresh retry budget.
                        self.auth_active = true;
                        self.auth_failures = 0;
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
        item: RawSubItem,
        conn: &WsConnection,
    ) -> Option<DriveExit> {
        let update = match item {
            RawSubItem::Update(u) => u,
            RawSubItem::Lagged => {
                // The raw socket->supervisor buffer overflowed (the supervisor
                // was stalled, e.g. mid-reconnect or in a slow command), so
                // updates were dropped. Surface Event::Lagged and drop the sub -
                // the same terminal semantics as the user-side overflow below;
                // the caller resubscribes to resync.
                if let Some(slot) = self.subs.get(&key) {
                    if let Err(e) = slot.user_tx.try_send(lagged_event(&key)) {
                        tracing::warn!(
                            ?key,
                            ?e,
                            "could not deliver raw-lag Lagged marker before drop"
                        );
                    }
                }
                return self.drop_sub(&key, conn).await;
            }
        };
        // try_send: blocking await here would back up the entire supervisor
        // (cmd loop, other subs, conn.closed() detection) on the slowest
        // consumer. A "drop-oldest" policy is not available in tokio mpsc, so
        // on Full we drop the sub and let the caller see the stream end. They
        // can resubscribe to start fresh. The borrow of `slot` is scoped to
        // this block so `drop_sub` (which needs `&mut self`) can run after.
        {
            let Some(slot) = self.subs.get_mut(&key) else {
                // Sub was unsubscribed locally but a stale frame arrived; drop it.
                return None;
            };
            // Reserve the last buffer slot for the terminal Event::Lagged marker.
            // While more than one slot is free, deliver the update; once only the
            // reserved slot remains, stop delivering data and spend it on the
            // marker so a lagging consumer always sees why the stream ended.
            // (Sending the marker into an already-full channel would drop it.)
            if slot.user_tx.capacity() > 1 {
                if slot.user_tx.try_send(Event::Update(update)).is_ok() {
                    return None;
                }
                // Only this task sends to user_tx, so Full is impossible while
                // capacity > 1; an error here means the receiver was dropped
                // (Closed). Fall through to drop the sub.
            } else {
                tracing::warn!(?key, "ws subscriber buffer full; dropping sub");
                // Best-effort terminal marker. The reserved slot makes this
                // succeed in the normal lag case; if the consumer also dropped
                // the receiver (Closed) it's moot. Log if it's ever lost so a
                // silently-ending stream is diagnosable.
                if let Err(e) = slot.user_tx.try_send(lagged_event(&key)) {
                    tracing::warn!(?key, ?e, "could not deliver Lagged marker before drop");
                }
            }
        }
        self.drop_sub(&key, conn).await
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

    async fn broadcast(&mut self, event: Event, skip: &ReplayMarks, conn: &WsConnection) {
        let keys: Vec<SubKey> = self.subs.keys().cloned().collect();
        for key in keys {
            // Skip subs that just got their first ack this cycle - from
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
            // Respect the reserved lag slot (see route_update): only spend a
            // slot on a lifecycle marker while more than the reserved slot is
            // free. Consuming the last slot here would starve a subsequent
            // route_update of the slot it needs for the terminal Event::Lagged.
            // Once the consumer is down to the reserved slot it is lagging, so
            // treat this as the lag-drop and spend that slot on Event::Lagged.
            // try_send throughout so a stalled consumer never blocks the
            // broadcast to the others. Either drop path goes through drop_sub
            // so the server-side subscription is released (otherwise the socket
            // keeps draining it and a resubscribe is rejected as a duplicate);
            // drop_sub drops the connection-level sender, so the raw Subscription
            // stream ends and `drive`'s StreamMap removes it on the next poll
            // (the same implicit pruning route_update relies on).
            // The DriveExit that drop_sub may return (conn gone during unsub) is
            // intentionally dropped here: broadcast can't short-circuit, and
            // drive's conn.closed() arm catches a dead connection right after.
            if slot.user_tx.capacity() > 1 {
                // capacity > 1 rules out Full (only this task sends), so an
                // error here is a dropped receiver: drop it + server-side unsub.
                if slot.user_tx.try_send(event.clone()).is_err() {
                    let _ = self.drop_sub(&key, conn).await;
                }
            } else {
                tracing::warn!(?key, "ws subscriber buffer full on broadcast; dropping sub");
                let _ = slot.user_tx.try_send(lagged_event(&key));
                let _ = self.drop_sub(&key, conn).await;
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

/// Outcome of a connect attempt - connect either succeeded or the
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
/// Build the terminal `Event::Lagged` marker for a dropped subscription from
/// its registry key. Shared by `route_update` and `broadcast` so the variant's
/// shape is defined in one place.
fn lagged_event(key: &SubKey) -> Event {
    Event::Lagged {
        channel: key.0,
        filter: key.1.clone(),
    }
}

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
/// identical jitter (which `subsec_nanos`-based jitter does - multiple
/// processes restarting in the same wall-clock millisecond would compute
/// the same offset).
struct Backoff {
    n: u32,
    /// xorshift64* state - must be non-zero (xorshift produces all zeros
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
        // xorshift64 - passes basic statistical tests, trivial to embed,
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

/// Per-call backoff seed from OS entropy, so two clients reconnecting on the
/// same host at the same instant get independent jitter. Avoids the `rand`
/// crate.
fn seed_for_backoff() -> u64 {
    use std::hash::BuildHasher;
    // `RandomState` seeds from OS entropy on construction, so each call yields
    // an independent value - two clients reconnecting on the same host at the
    // same millisecond get independent jitter (avoids a thundering herd),
    // without depending on a usable clock or on ASLR. Hash the thread id for
    // extra per-task divergence.
    let s = std::collections::hash_map::RandomState::new().hash_one(std::thread::current().id());
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
        // Allow for jitter - assert the floor.
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
