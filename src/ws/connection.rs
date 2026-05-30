//! WebSocket raw connection (crate-internal).
//!
//! One [`WsConnection`] owns one tokio task driving the socket. The task
//! serves commands sent over an mpsc channel and routes inbound frames:
//! - control frames (`welcome` / `authenticated` / `subscribed` /
//!   `unsubscribed` / `error` / `pong`) complete the in-flight pending op.
//! - data frames (`snapshot` / `update`) fan out to the matching
//!   subscription's mpsc receiver.
//!
//! Ops are serialized: at most one pending request in flight. Server
//! replies in order per the wire spec, and serializing makes error
//! attribution unambiguous (server `error` frames carry no correlation id).
//! At ~tens of subs per connection in practice, the throughput cost is
//! negligible and the simplicity is worth it.
//!
//! Public callers go through [`super::managed::Session`] (the managed
//! supervisor) which uses this raw connection as its transport.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_stream::wrappers::ReceiverStream;
use tokio_tungstenite::tungstenite::Message;

use crate::auth::HmacSigner;
use crate::error::{Error, Result};

use super::auth as ws_auth;
use super::channel::{Channel, ChannelName};
use super::event::{ServerFrame, Update, UpdateKind, WireType};
use super::frame;

/// Default per-subscription buffer. 1024 messages is plenty for active
/// channels (book updates peak ~100/s); slow consumers fail loud rather
/// than silently dropping.
const SUB_BUFFER: usize = 1024;
/// Outbound command queue depth.
const CMD_BUFFER: usize = 64;

/// `(ChannelName, filter_string)`. Filter is `""` for filter-less channels
/// (`portfolio`, `notification`).
type SubKey = (ChannelName, String);

/// Live WebSocket connection. Cheap to clone - internal state is Arc'd so
/// the underlying task survives until every clone is dropped (or `close`
/// is called explicitly). Crate-internal - used by the managed supervisor.
#[derive(Clone)]
pub(crate) struct WsConnection {
    inner: Arc<Inner>,
}

struct Inner {
    cmd_tx: mpsc::Sender<Command>,
    connection_id: String,
    hmac: Option<HmacSigner>,
    /// Set to `true` when the driver task exits (socket closed by peer,
    /// read error, send error, or client `close`). Watch channel chosen
    /// over `Notify` because `Notify::notify_waiters` only wakes
    /// *current* waiters - a notification fired before the supervisor
    /// reaches `closed().await` would be lost. `watch::Receiver::changed`
    /// is naturally race-free: the snapshot value reflects the latest
    /// `send` regardless of subscribe-order.
    closed_rx: watch::Receiver<bool>,
}

/// One snapshot/update stream for a subscribed channel. Drop to free the
/// internal sender slot - note that this does NOT send `unsub` to the
/// server. Call [`WsConnection::unsubscribe`] explicitly for clean
/// shutdown of a single channel. Crate-internal - public callers receive
/// [`super::event::Event`] via [`super::managed::SubscriptionStream`].
pub(crate) type Subscription = ReceiverStream<Update>;

enum Command {
    Subscribe {
        channel: Channel,
        sender: mpsc::Sender<Update>,
        ack: oneshot::Sender<Result<()>>,
    },
    Unsubscribe {
        channel: Channel,
        ack: oneshot::Sender<Result<()>>,
    },
    Authenticate {
        timestamp: String,
        signature: String,
        key: String,
        ack: oneshot::Sender<Result<String>>,
    },
    Ping {
        ack: oneshot::Sender<Result<()>>,
    },
}

/// Tracks the single in-flight request waiting for its reply.
enum Pending {
    Sub {
        key: SubKey,
        ack: oneshot::Sender<Result<()>>,
    },
    Unsub {
        key: SubKey,
        ack: oneshot::Sender<Result<()>>,
    },
    Auth {
        ack: oneshot::Sender<Result<String>>,
    },
    Ping {
        ack: oneshot::Sender<Result<()>>,
    },
}

impl WsConnection {
    /// Crate-internal entry point used by the managed supervisor.
    pub(crate) async fn connect_raw(url: &str, hmac: Option<HmacSigner>) -> Result<Self> {
        Self::connect(url, hmac).await
    }

    async fn connect(url: &str, hmac: Option<HmacSigner>) -> Result<Self> {
        let (mut socket, _resp) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| Error::Ws(format!("connect {url}: {e}")))?;

        // Server sends `welcome` unprompted before anything else. Bound
        // the wait so a pathological proxy doesn't hang us forever.
        let welcome = match tokio::time::timeout(Duration::from_secs(10), socket.next()).await {
            Ok(Some(Ok(Message::Text(s)))) => s,
            Ok(Some(Ok(other))) => {
                return Err(Error::Ws(format!(
                    "expected text welcome frame, got {other:?}"
                )));
            }
            Ok(Some(Err(e))) => return Err(Error::Ws(format!("read welcome: {e}"))),
            Ok(None) => return Err(Error::Ws("connection closed before welcome".into())),
            Err(_) => return Err(Error::Ws("welcome frame did not arrive within 10s".into())),
        };
        let frame: ServerFrame = serde_json::from_str(&welcome)
            .map_err(|e| Error::Ws(format!("welcome parse: {e}; raw={welcome}")))?;
        if !matches!(frame.kind, WireType::Welcome) {
            return Err(Error::Ws(format!(
                "expected type=welcome, got {:?}",
                frame.kind
            )));
        }
        let connection_id = frame
            .connection_id
            .ok_or_else(|| Error::Ws("welcome frame missing connection_id".into()))?;

        let (cmd_tx, cmd_rx) = mpsc::channel(CMD_BUFFER);
        let (closed_tx, closed_rx) = watch::channel(false);
        let driver = Driver::new(socket, cmd_rx, closed_tx);
        tokio::spawn(driver.run());

        Ok(Self {
            inner: Arc::new(Inner {
                cmd_tx,
                connection_id,
                hmac,
                closed_rx,
            }),
        })
    }

    /// Future that completes when the underlying driver task exits - i.e.
    /// the socket has dropped, errored, or been explicitly closed. Used by
    /// the supervisor to detect connection loss instantly without waiting
    /// for the next periodic ping. Race-safe: returns immediately if the
    /// driver already exited before the caller awaited.
    pub(crate) async fn closed(&self) {
        let mut rx = self.inner.closed_rx.clone();
        if *rx.borrow() {
            return;
        }
        // changed() returns Err only if all senders are dropped, which we
        // treat as equivalent to "closed" - driver task is gone either
        // way.
        let _ = rx.changed().await;
    }

    /// Server-assigned ID for this connection. Useful for support tickets
    /// - the server logs everything keyed off this.
    #[allow(dead_code)]
    pub(crate) fn connection_id(&self) -> &str {
        &self.inner.connection_id
    }

    /// Subscribe to `channel`. Awaits the server `subscribed` ack and
    /// returns a stream of subsequent snapshot / update messages.
    pub(crate) async fn subscribe(&self, channel: Channel) -> Result<Subscription> {
        let (sender, receiver) = mpsc::channel(SUB_BUFFER);
        let (ack_tx, ack_rx) = oneshot::channel();
        self.inner
            .cmd_tx
            .send(Command::Subscribe {
                channel,
                sender,
                ack: ack_tx,
            })
            .await
            .map_err(|_| Error::Ws("connection task is gone".into()))?;
        ack_rx
            .await
            .map_err(|_| Error::Ws("connection task dropped subscribe ack".into()))??;
        Ok(ReceiverStream::new(receiver))
    }

    /// Unsubscribe from `channel`. Awaits the server `unsubscribed` ack.
    /// Drops the stream slot - the previously returned [`Subscription`]
    /// will yield `None` after any in-flight messages drain.
    pub(crate) async fn unsubscribe(&self, channel: Channel) -> Result<()> {
        let (ack_tx, ack_rx) = oneshot::channel();
        self.inner
            .cmd_tx
            .send(Command::Unsubscribe {
                channel,
                ack: ack_tx,
            })
            .await
            .map_err(|_| Error::Ws("connection task is gone".into()))?;
        ack_rx
            .await
            .map_err(|_| Error::Ws("connection task dropped unsubscribe ack".into()))?
    }

    /// Authenticate the connection. Required before subscribing to any
    /// private channel (`order`, `position`, `portfolio`, `notification`).
    /// Returns the wallet address the server resolved for this key.
    pub(crate) async fn authenticate(&self) -> Result<String> {
        let signer = self
            .inner
            .hmac
            .as_ref()
            .ok_or_else(|| Error::Ws("authenticate requires api_key on the Client".into()))?;
        let now = ws_auth::now_unix_secs()?;
        let (ts, sig) = ws_auth::build_ws_auth(signer, now);
        let (ack_tx, ack_rx) = oneshot::channel();
        self.inner
            .cmd_tx
            .send(Command::Authenticate {
                timestamp: ts,
                signature: sig,
                key: signer.api_key().to_string(),
                ack: ack_tx,
            })
            .await
            .map_err(|_| Error::Ws("connection task is gone".into()))?;
        ack_rx
            .await
            .map_err(|_| Error::Ws("connection task dropped auth ack".into()))?
    }

    /// Application-level `{"op":"ping"}`. Awaits the matching `pong`. The
    /// underlying WebSocket library already handles protocol-level pings -
    /// the supervisor uses this for periodic dead-socket detection
    /// (`super::managed::Supervisor::drive`).
    pub(crate) async fn ping(&self) -> Result<()> {
        let (ack_tx, ack_rx) = oneshot::channel();
        self.inner
            .cmd_tx
            .send(Command::Ping { ack: ack_tx })
            .await
            .map_err(|_| Error::Ws("connection task is gone".into()))?;
        ack_rx
            .await
            .map_err(|_| Error::Ws("connection task dropped ping ack".into()))?
    }

    /// Close the connection. After this returns, the driver task has
    /// exited - clones of this handle are no longer functional.
    pub(crate) async fn close(self) {
        // Dropping every cmd_tx clone causes recv() in the driver to
        // return None, which the driver treats as shutdown. We can't
        // force-drop other clones, but we can drop ours; callers who hold
        // additional clones must drop them too. The subscription receiver
        // ends naturally when the driver exits.
        drop(self.inner);
    }
}

/* ──── driver task ──────────────────────────────────────────────────── */

struct Driver<S> {
    socket: S,
    cmd_rx: mpsc::Receiver<Command>,
    pending: Option<Pending>,
    /// `(channel, filter)` → fan-out sender. Filter for filter-less
    /// channels is `""` to match the server, which omits the filter
    /// field on `portfolio` / `notification` updates.
    subscribers: HashMap<SubKey, mpsc::Sender<Update>>,
    /// Watch sender shared with [`Inner::closed_rx`]. Set to `true`
    /// exactly once on driver-task exit so any number of
    /// [`WsConnection::closed`] callers observe the death - even if they
    /// `await` after the exit.
    closed_tx: watch::Sender<bool>,
}

impl<S> Driver<S>
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error>
        + futures_util::Stream<
            Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>,
        > + Unpin
        + Send
        + 'static,
{
    fn new(socket: S, cmd_rx: mpsc::Receiver<Command>, closed_tx: watch::Sender<bool>) -> Self {
        Self {
            socket,
            cmd_rx,
            pending: None,
            subscribers: HashMap::new(),
            closed_tx,
        }
    }

    async fn run(mut self) {
        loop {
            tokio::select! {
                // Only pull a new command when no op is in flight, otherwise
                // we'd lose response correlation.
                cmd = self.cmd_rx.recv(), if self.pending.is_none() => {
                    match cmd {
                        Some(c) => {
                            if let Err(e) = self.dispatch_command(c).await {
                                tracing::warn!(error = %e, "ws send failed; closing");
                                break;
                            }
                        }
                        None => break, // all senders dropped
                    }
                }
                msg = self.socket.next() => {
                    match msg {
                        Some(Ok(Message::Text(s))) => self.handle_frame(&s),
                        Some(Ok(Message::Binary(_))) => {
                            // Server only sends text frames per docs; ignore.
                        }
                        Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {
                            // tokio-tungstenite auto-replies to protocol
                            // pings; nothing for us to do.
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            tracing::info!("ws closed by server");
                            break;
                        }
                        Some(Ok(Message::Frame(_))) => {} // raw frame, ignore
                        Some(Err(e)) => {
                            tracing::warn!(error = %e, "ws read error");
                            break;
                        }
                    }
                }
            }
        }
        // Fail any in-flight op so the caller doesn't hang on its oneshot.
        if let Some(p) = self.pending.take() {
            let err = || Error::Ws("connection closed".into());
            match p {
                Pending::Sub { ack, .. } | Pending::Unsub { ack, .. } | Pending::Ping { ack } => {
                    let _ = ack.send(Err(err()));
                }
                Pending::Auth { ack } => {
                    let _ = ack.send(Err(err()));
                }
            }
        }
        // Notify supervisor (via WsConnection::closed) BEFORE dropping
        // subscribers so the supervisor sees death even if it has no
        // subs registered. send_replace flips the watch; any future
        // subscriber to closed_rx sees the new value immediately.
        let _ = self.closed_tx.send(true);
        // Subscribers map drops here, ending all receiver streams.
    }

    async fn dispatch_command(&mut self, cmd: Command) -> Result<()> {
        match cmd {
            Command::Subscribe {
                channel,
                sender,
                ack,
            } => {
                let frame = match frame::subscribe(&channel) {
                    Ok(f) => f,
                    Err(e) => {
                        let _ = ack.send(Err(e));
                        return Ok(());
                    }
                };
                let key = (channel.name(), channel.filter().to_string());
                // Register before sending so the snapshot that follows the
                // ack lands in the subscriber's buffer.
                self.subscribers.insert(key.clone(), sender);
                self.pending = Some(Pending::Sub { key, ack });
                self.send_or_fail(frame).await?;
            }
            Command::Unsubscribe { channel, ack } => {
                let frame = match frame::unsubscribe(&channel) {
                    Ok(f) => f,
                    Err(e) => {
                        let _ = ack.send(Err(e));
                        return Ok(());
                    }
                };
                let key = (channel.name(), channel.filter().to_string());
                self.pending = Some(Pending::Unsub { key, ack });
                self.send_or_fail(frame).await?;
            }
            Command::Authenticate {
                timestamp,
                signature,
                key,
                ack,
            } => {
                let frame = match frame::auth(&key, &timestamp, &signature) {
                    Ok(f) => f,
                    Err(e) => {
                        let _ = ack.send(Err(e));
                        return Ok(());
                    }
                };
                self.pending = Some(Pending::Auth { ack });
                self.send_or_fail(frame).await?;
            }
            Command::Ping { ack } => {
                self.pending = Some(Pending::Ping { ack });
                self.send_or_fail(frame::ping()).await?;
            }
        }
        Ok(())
    }

    async fn send_or_fail(&mut self, msg: Message) -> Result<()> {
        if let Err(e) = self.socket.send(msg).await {
            let detail = format!("ws send: {e}");
            self.fail_pending(Error::Ws(detail.clone()));
            return Err(Error::Ws(detail));
        }
        Ok(())
    }

    fn fail_pending(&mut self, err: Error) {
        if let Some(p) = self.pending.take() {
            match p {
                Pending::Sub { ack, key } => {
                    self.subscribers.remove(&key);
                    let _ = ack.send(Err(err));
                }
                Pending::Unsub { ack, .. } => {
                    let _ = ack.send(Err(err));
                }
                Pending::Auth { ack } => {
                    let _ = ack.send(Err(err));
                }
                Pending::Ping { ack } => {
                    let _ = ack.send(Err(err));
                }
            }
        }
    }

    fn handle_frame(&mut self, raw: &str) {
        let frame: ServerFrame = match serde_json::from_str(raw) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(error = %e, raw = %raw, "ws frame parse");
                return;
            }
        };
        match frame.kind {
            WireType::Welcome => {
                // Spec says welcome arrives once, before anything else.
                // A second welcome means the server reset state - log and
                // ignore; the supervisor's reconnect path will handle it.
                tracing::warn!("unexpected second welcome frame");
            }
            WireType::Authenticated => {
                let address = frame.address.unwrap_or_default();
                if let Some(Pending::Auth { ack }) = self.pending.take() {
                    let _ = ack.send(Ok(address));
                } else {
                    tracing::warn!("authenticated frame with no pending auth");
                }
            }
            WireType::Subscribed => {
                if let Some(Pending::Sub { ack, key }) = self.pending.take() {
                    // Sanity: server echoes channel; if it diverges from
                    // what we registered, the route would never fire.
                    if let Some(server_ch) = frame.channel {
                        if server_ch != key.0 {
                            tracing::warn!(
                                expected = ?key.0, got = ?server_ch,
                                "subscribed channel mismatch"
                            );
                        }
                    }
                    let _ = ack.send(Ok(()));
                } else {
                    tracing::warn!("subscribed frame with no pending sub");
                }
            }
            WireType::Unsubscribed => {
                if let Some(Pending::Unsub { ack, key }) = self.pending.take() {
                    self.subscribers.remove(&key);
                    let _ = ack.send(Ok(()));
                } else {
                    tracing::warn!("unsubscribed frame with no pending unsub");
                }
            }
            WireType::Error => {
                let msg = frame.message.unwrap_or_else(|| "(no message)".into());
                let err = Error::Ws(format!("server error: {msg}"));
                if self.pending.is_some() {
                    self.fail_pending(err);
                } else {
                    tracing::warn!(message = %msg, "ws server error with no pending op");
                }
            }
            WireType::Pong => {
                if let Some(Pending::Ping { ack }) = self.pending.take() {
                    let _ = ack.send(Ok(()));
                } else {
                    // Server-initiated pong (rare); ignore.
                }
            }
            WireType::Snapshot | WireType::Update => self.route_data(frame),
        }
    }

    fn route_data(&mut self, frame: ServerFrame) {
        let Some(channel) = frame.channel else {
            tracing::warn!("data frame without channel");
            return;
        };
        let filter = frame.filter.unwrap_or_default();
        // Server stamps order/position/trade update frames with the concrete
        // market in `filter` even when we subscribed with `market: None`
        // (wildcard, registered under the empty filter); the snapshot for
        // that sub carries `filter:""`. Mirror the server's hierarchical
        // match: exact (channel,filter) first, else the (channel,"")
        // wildcard subscriber.
        let exact: SubKey = (channel, filter.clone());
        let key: SubKey = if self.subscribers.contains_key(&exact) {
            exact
        } else if !filter.is_empty() && self.subscribers.contains_key(&(channel, String::new())) {
            (channel, String::new())
        } else {
            // Could be a stale frame for a channel we already unsubscribed
            // from; server drains its outbox before honoring unsub.
            return;
        };
        // `key` was just resolved from `contains_key` above with no await in
        // between, so this lookup cannot miss. Use a defensive branch rather
        // than `expect` so a future refactor can never panic the driver task.
        let Some(sender) = self.subscribers.get(&key) else {
            // Unreachable under the single-task invariant above; log if a
            // future refactor ever breaks it, so the dropped frame is visible.
            tracing::warn!(?key, "ws route: subscriber vanished after contains_key");
            return;
        };
        let kind = match frame.kind {
            WireType::Snapshot => UpdateKind::Snapshot,
            _ => UpdateKind::Update,
        };
        let update = Update {
            kind,
            channel,
            gsn: frame.gsn.unwrap_or(0),
            ts: frame.ts.unwrap_or(0),
            filter,
            data: frame.data.unwrap_or(serde_json::Value::Null),
        };
        // try_send: if the consumer is slow, drop with a warn rather than
        // backpressure into the socket.
        match sender.try_send(update) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!(?key, "dropping ws update - subscriber buffer full");
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.subscribers.remove(&key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ws::channel::Channel;

    /// Build a `Channel` and round-trip its routing key shape.
    #[test]
    fn routing_key_for_book() {
        let ch = Channel::Book {
            market: "BTC-PERP".into(),
        };
        assert_eq!(ch.name(), ChannelName::Book);
        assert_eq!(ch.filter(), "BTC-PERP");
    }

    #[test]
    fn routing_key_for_portfolio_has_empty_filter() {
        let ch = Channel::Portfolio;
        assert_eq!(ch.name(), ChannelName::Portfolio);
        assert_eq!(ch.filter(), "");
    }
}
