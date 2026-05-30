//! WebSocket chaos / state-machine integration tests.
//!
//! Runs in-process against a small `MockPulse` that speaks just enough of
//! the wire protocol to exercise reconnect, sub-replay, wildcard fan-out,
//! and sparse-GSN handling. Kept dependency-free (no extra dev-deps) by
//! leaning on the same `tokio-tungstenite` already in the tree.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};
use tokio::time::timeout;
use tokio_tungstenite::{accept_async, tungstenite::Message};

use alloy_primitives::Address;
use alloy_sol_types::eip712_domain;
use obsdn_sdk::ws::{Channel, ChannelName, Event};
use obsdn_sdk::{Client, Env};

/// Server-side commands the mock pulse accepts from the test driver.
#[derive(Debug)]
enum MockCmd {
    /// Drop the current open connection (server-initiated close). Forces
    /// the client into reconnect.
    KillConn,
    /// Emit a snapshot/update frame on the active connection.
    Push {
        kind: &'static str, // "snapshot" or "update"
        channel: &'static str,
        filter: Option<&'static str>,
        gsn: u64,
        data: Value,
    },
    /// Reject the next auth attempt with this message. Otherwise the mock
    /// auto-acks any auth frame.
    RejectNextAuth(String),
}

#[derive(Default)]
struct MockState {
    /// Active per-connection sender for outbound text frames. `None` when
    /// no client is connected.
    out: Option<mpsc::Sender<Message>>,
    /// Reject next auth with this message (one-shot).
    reject_next_auth: Option<String>,
    /// Connection counter - incremented every accept. Doubles as a
    /// `connection_id` and lets tests assert that a reconnect actually
    /// happened.
    conn_seq: u64,
    /// (channel, filter) pairs the client has sent an `unsub` for, across all
    /// connections. Lets tests assert a server-side unsubscribe was issued.
    unsubbed: Vec<(String, String)>,
}

struct MockPulse {
    addr: SocketAddr,
    cmd_tx: mpsc::Sender<MockCmd>,
    state: Arc<Mutex<MockState>>,
}

impl MockPulse {
    async fn start() -> Self {
        // 0 → OS-assigned port. Loopback only; no external exposure.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        let state = Arc::new(Mutex::new(MockState::default()));
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<MockCmd>(32);

        // Acceptor task: one connection at a time. Pulse multiplexes all
        // subs on a single connection per client; we mimic that.
        let accept_state = state.clone();
        tokio::spawn(async move {
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let ws = match accept_async(stream).await {
                    Ok(w) => w,
                    Err(e) => {
                        eprintln!("mock accept_async err: {e}");
                        continue;
                    }
                };
                handle_conn(ws, accept_state.clone()).await;
            }
        });

        // Driver task: applies test commands to whichever connection is
        // currently open.
        let drive_state = state.clone();
        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                let mut s = drive_state.lock().await;
                match cmd {
                    MockCmd::KillConn => {
                        // Dropping the sender side ends `handle_conn`'s
                        // outbound loop, which closes the socket.
                        s.out = None;
                    }
                    MockCmd::Push {
                        kind,
                        channel,
                        filter,
                        gsn,
                        data,
                    } => {
                        if let Some(tx) = s.out.as_ref() {
                            let mut frame = json!({
                                "type": kind,
                                "channel": channel,
                                "gsn": gsn,
                                "ts": "0", // server emits ts as JSON-string i64 ns
                                "data": data,
                            });
                            if let Some(f) = filter {
                                frame["filter"] = json!(f);
                            }
                            let _ = tx.send(Message::Text(frame.to_string())).await;
                        }
                    }
                    MockCmd::RejectNextAuth(msg) => {
                        s.reject_next_auth = Some(msg);
                    }
                }
            }
        });

        Self {
            addr,
            cmd_tx,
            state,
        }
    }

    fn url(&self) -> String {
        format!("ws://{}/ws", self.addr)
    }

    async fn send(&self, cmd: MockCmd) {
        self.cmd_tx.send(cmd).await.expect("mock cmd send");
    }

    async fn conn_seq(&self) -> u64 {
        self.state.lock().await.conn_seq
    }

    /// Whether the client has sent an `unsub` for `(channel, filter)`.
    async fn was_unsubbed(&self, channel: &str, filter: &str) -> bool {
        self.state
            .lock()
            .await
            .unsubbed
            .iter()
            .any(|(c, f)| c == channel && f == filter)
    }
}

async fn handle_conn<S>(mut ws: tokio_tungstenite::WebSocketStream<S>, state: Arc<Mutex<MockState>>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let conn_seq = {
        let mut s = state.lock().await;
        s.conn_seq += 1;
        s.conn_seq
    };

    // Buffered outbound channel so push commands are non-blocking.
    // The sender is owned exclusively by `state.out` - handle_conn does
    // NOT retain a local clone. That way `MockCmd::KillConn` (which sets
    // `state.out = None`) drops the only sender and out_rx returns None,
    // unblocking this task's select! and forcing a clean close.
    let (out_tx, mut out_rx) = mpsc::channel::<Message>(64);
    {
        let mut s = state.lock().await;
        s.out = Some(out_tx);
    }

    // Send welcome with a per-connection id so reconnect tests can
    // distinguish.
    let welcome = json!({
        "type": "welcome",
        "connection_id": format!("mock-{conn_seq}"),
    });
    if ws.send(Message::Text(welcome.to_string())).await.is_err() {
        let mut s = state.lock().await;
        s.out = None;
        return;
    }

    // Track active subs so we can ignore stale-channel pushes.
    let mut subs: HashMap<(String, String), ()> = HashMap::new();

    loop {
        tokio::select! {
            // Outbound: forward push frames or close when sender dropped
            // (KillConn).
            msg = out_rx.recv() => {
                let Some(msg) = msg else {
                    let _ = ws.close(None).await;
                    return;
                };
                if let Message::Text(ref s) = msg {
                    // Filter outbound data frames against active subs so a
                    // stale push doesn't deliver to an unsubscribed channel.
                    // Mirror real pulse fan-out: a concrete-filter frame is
                    // delivered to an exact (ch,fi) subscriber OR a wildcard
                    // (ch,"") subscriber (server-side wildcard routing).
                    if let Ok(v) = serde_json::from_str::<Value>(s) {
                        let kind = v["type"].as_str().unwrap_or("");
                        if matches!(kind, "snapshot" | "update") {
                            let ch = v["channel"].as_str().unwrap_or("").to_string();
                            let fi = v["filter"].as_str().unwrap_or("").to_string();
                            let subscribed = subs.contains_key(&(ch.clone(), fi.clone()))
                                || (!fi.is_empty() && subs.contains_key(&(ch, String::new())));
                            if !subscribed {
                                continue;
                            }
                        }
                    }
                }
                if ws.send(msg).await.is_err() {
                    let mut s = state.lock().await;
                    s.out = None;
                    return;
                }
            }
            // Inbound: client commands. We handle just sub/unsub/auth/ping.
            inbound = ws.next() => {
                let Some(Ok(Message::Text(s))) = inbound else {
                    // Close, error, ping/pong, or other - bail.
                    let _ = ws.close(None).await;
                    let mut st = state.lock().await;
                    st.out = None;
                    return;
                };
                let v: Value = match serde_json::from_str(&s) { Ok(v) => v, Err(_) => continue };
                let op = v["op"].as_str().unwrap_or("");
                let channel = v["channel"].as_str().unwrap_or("").to_string();
                let filter = v["params"]["market"].as_str()
                    .or_else(|| v["params"]["asset"].as_str())
                    .or_else(|| v["params"]["event"].as_str())
                    .unwrap_or("")
                    .to_string();
                match op {
                    "sub" => {
                        subs.insert((channel.clone(), filter.clone()), ());
                        let mut ack = json!({ "type": "subscribed", "channel": channel });
                        if !filter.is_empty() { ack["filter"] = json!(filter); }
                        if ws.send(Message::Text(ack.to_string())).await.is_err() { return; }
                    }
                    "unsub" => {
                        subs.remove(&(channel.clone(), filter.clone()));
                        state
                            .lock()
                            .await
                            .unsubbed
                            .push((channel.clone(), filter.clone()));
                        let mut ack = json!({ "type": "unsubscribed", "channel": channel });
                        if !filter.is_empty() { ack["filter"] = json!(filter); }
                        if ws.send(Message::Text(ack.to_string())).await.is_err() { return; }
                    }
                    "auth" => {
                        let reject = state.lock().await.reject_next_auth.take();
                        let resp = if let Some(msg) = reject {
                            json!({ "type": "error", "message": msg })
                        } else {
                            json!({ "type": "authenticated", "address": "0xMOCKADDR" })
                        };
                        if ws.send(Message::Text(resp.to_string())).await.is_err() { return; }
                    }
                    "ping" => {
                        let pong = json!({ "type": "pong" });
                        if ws.send(Message::Text(pong.to_string())).await.is_err() { return; }
                    }
                    _ => {
                        // Unknown op - surface as error per the wire shape.
                        let resp = json!({ "type": "error", "message": format!("unknown op: {op}") });
                        let _ = ws.send(Message::Text(resp.to_string())).await;
                    }
                }
            }
        }
    }
}

/* ──── tests ───────────────────────────────────────────────────────── */

fn dummy_domain() -> alloy_sol_types::Eip712Domain {
    eip712_domain! {
        name: "Test",
        version: "1",
        chain_id: 1u64,
        verifying_contract: Address::ZERO,
    }
}

fn build_client(url: String) -> Client {
    Client::builder()
        .env(Env::Custom {
            rest: "http://127.0.0.1:1".into(),
            ws: url,
        })
        .eip712_domain(dummy_domain())
        .build()
        .expect("build client")
}

#[tokio::test]
async fn subscribe_and_receive_update() {
    let mock = MockPulse::start().await;
    let client = build_client(mock.url());
    let ws = client.ws();
    let mut stream = ws
        .subscribe(Channel::Book {
            market: "BTC-PERP".into(),
        })
        .await
        .expect("subscribe");
    // Server-initiated push after subscribe ack.
    mock.send(MockCmd::Push {
        kind: "snapshot",
        channel: "book",
        filter: Some("BTC-PERP"),
        gsn: 1,
        data: json!({"bids": [], "asks": []}),
    })
    .await;
    let evt = timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("first event in 2s")
        .expect("stream open");
    let Event::Update(u) = evt else {
        panic!("expected Update, got {evt:?}");
    };
    assert_eq!(u.channel, ChannelName::Book);
    assert_eq!(u.gsn, 1);
    ws.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn noncontiguous_gsn_does_not_emit_gap() {
    // Pulse `gsn` is a sparse global event watermark, not a dense per-sub
    // sequence - non-contiguous GSNs on one channel are normal (throttled /
    // selectively-emitted frames skip numbers). The SDK must NOT infer a
    // gap: both updates pass straight through, no synthetic event between.
    let mock = MockPulse::start().await;
    let client = build_client(mock.url());
    let ws = client.ws();
    let mut stream = ws
        .subscribe(Channel::Book {
            market: "BTC-PERP".into(),
        })
        .await
        .expect("subscribe");
    for gsn in [1, 5] {
        mock.send(MockCmd::Push {
            kind: "update",
            channel: "book",
            filter: Some("BTC-PERP"),
            gsn,
            data: json!({"changes": []}),
        })
        .await;
    }
    let a = timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("first event")
        .expect("stream open");
    let b = timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("second event")
        .expect("stream open");
    match (a, b) {
        (Event::Update(a), Event::Update(b)) => {
            assert_eq!(a.gsn, 1);
            assert_eq!(b.gsn, 5, "second update delivered as-is, no gap injected");
        }
        other => panic!("expected two consecutive Updates, got: {other:?}"),
    }
    ws.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn wildcard_sub_routes_concrete_filter_updates() {
    // A `market: None` sub registers under the empty filter, but the server
    // stamps update frames with the concrete market (snapshot carries ""),
    // mirroring nil's hierarchical wildcard match. The SDK must route the
    // concrete-filter update back to the wildcard subscriber - else a
    // market-maker subscribing to all-markets gets the snapshot then silence.
    let mock = MockPulse::start().await;
    let client = build_client(mock.url());
    let ws = client.ws();
    let mut stream = ws
        .subscribe(Channel::Trade { market: None })
        .await
        .expect("subscribe");
    // Snapshot for a wildcard sub carries filter="".
    mock.send(MockCmd::Push {
        kind: "snapshot",
        channel: "trade",
        filter: Some(""),
        gsn: 1,
        data: json!([]),
    })
    .await;
    // Update carries the concrete market.
    mock.send(MockCmd::Push {
        kind: "update",
        channel: "trade",
        filter: Some("BTC-PERP"),
        gsn: 2,
        data: json!({"px": "1", "sz": "1"}),
    })
    .await;
    let snap = timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("snapshot")
        .expect("stream open");
    let upd = timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("update routed to wildcard sub")
        .expect("stream open");
    match (snap, upd) {
        (Event::Update(s), Event::Update(u)) => {
            assert_eq!(s.filter, "");
            assert_eq!(
                u.filter, "BTC-PERP",
                "wildcard sub sees concrete-market update"
            );
            assert_eq!(u.gsn, 2);
        }
        other => panic!("expected snapshot+update, got: {other:?}"),
    }
    ws.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn reconnect_emits_reconnected_and_resubscribes() {
    let mock = MockPulse::start().await;
    let client = build_client(mock.url());
    let ws = client.ws();
    let mut stream = ws
        .subscribe(Channel::Book {
            market: "BTC-PERP".into(),
        })
        .await
        .expect("subscribe");
    // First update on connection #1.
    mock.send(MockCmd::Push {
        kind: "snapshot",
        channel: "book",
        filter: Some("BTC-PERP"),
        gsn: 1,
        data: json!({}),
    })
    .await;
    let first = timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("first")
        .expect("open");
    assert!(matches!(first, Event::Update(_)));

    // Server kills the conn; supervisor should reconnect within backoff
    // (~100ms-500ms) and resubscribe automatically.
    mock.send(MockCmd::KillConn).await;

    // Wait until the mock observes a fresh connection. Loop with a small
    // delay to avoid racing the accept handshake.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while mock.conn_seq().await < 2 {
        if tokio::time::Instant::now() >= deadline {
            panic!("mock did not see reconnect within 5s");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // After reconnect, push another update - supervisor should have
    // re-subscribed before this lands. Allow plenty of slack: the first
    // event we see may be either the Reconnected marker or, if the push
    // races ahead, an Update. Drain until we've seen both.
    mock.send(MockCmd::Push {
        kind: "update",
        channel: "book",
        filter: Some("BTC-PERP"),
        gsn: 100,
        data: json!({}),
    })
    .await;
    let mut saw_reconnected = false;
    let mut saw_update = false;
    let drain_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !(saw_reconnected && saw_update) {
        let remaining = drain_deadline.saturating_duration_since(tokio::time::Instant::now());
        let evt = match timeout(remaining, stream.next()).await {
            Ok(Some(e)) => e,
            Ok(None) => panic!("stream closed before reconnect events"),
            Err(_) => panic!("missing events: reconnected={saw_reconnected}, update={saw_update}"),
        };
        match evt {
            Event::Reconnected => saw_reconnected = true,
            Event::Update(u) => {
                assert_eq!(u.gsn, 100, "post-reconnect update gsn");
                saw_update = true;
            }
            Event::Unauthorized(m) => panic!("unexpected unauthorized: {m}"),
            _ => {}
        }
    }
    ws.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn auth_replay_failure_emits_unauthorized_after_reconnect() {
    let mock = MockPulse::start().await;
    // Build a client with HMAC creds so the supervisor will attempt auth.
    let client = Client::builder()
        .env(Env::Custom {
            rest: "http://127.0.0.1:1".into(),
            ws: mock.url(),
        })
        .eip712_domain(dummy_domain())
        .api_key("k", "s")
        .build()
        .expect("build client");
    let ws = client.ws();
    // Subscribe first so the supervisor opens a connection. Public sub -
    // pulse mock allows it without auth.
    let mut stream = ws
        .subscribe(Channel::Book {
            market: "BTC-PERP".into(),
        })
        .await
        .expect("subscribe");
    // Authenticate succeeds on the live connection (mock auto-acks).
    ws.authenticate().await.expect("initial auth");
    // Arm the rejection BEFORE the reconnect so the next auth replay
    // hits the rejection path.
    mock.send(MockCmd::RejectNextAuth("revoked".into())).await;
    // Force a reconnect. Supervisor will re-auth, fail, downgrade to
    // public, and broadcast Unauthorized.
    mock.send(MockCmd::KillConn).await;
    // Drain stream until we see Unauthorized. May be preceded by
    // Reconnected; tolerate either order.
    let mut saw_unauthorized = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !saw_unauthorized {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let evt = match timeout(remaining, stream.next()).await {
            Ok(Some(e)) => e,
            Ok(None) => panic!("stream closed before Unauthorized"),
            Err(_) => panic!("did not see Unauthorized within 5s"),
        };
        if let Event::Unauthorized(msg) = evt {
            assert!(
                msg.contains("revoked"),
                "msg should mention server reason: {msg}"
            );
            saw_unauthorized = true;
        }
    }
    ws.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn shutdown_closes_subscription_streams() {
    let mock = MockPulse::start().await;
    let client = build_client(mock.url());
    let ws = client.ws();
    let mut stream = ws
        .subscribe(Channel::Book {
            market: "BTC-PERP".into(),
        })
        .await
        .expect("subscribe");
    ws.shutdown().await.expect("shutdown");
    // After shutdown, the stream MUST end (None) so callers know to bail.
    let evt = timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("stream end within 2s");
    assert!(evt.is_none(), "stream should end after shutdown");
}

/// Repro for C2 (subscribe-future drop): caller cancels their
/// `subscribe(...).await` before the ack fires. Without GC, the SubKey
/// stays pinned in the registry forever and a fresh `subscribe(same)`
/// call would error "subscription request in flight".
///
/// We trigger cancellation via `tokio::select!` with an immediate
/// shutdown signal - deterministic vs racing a wall-clock timeout
/// against the localhost WS handshake.
#[tokio::test]
async fn dropped_subscribe_future_does_not_pin_channel() {
    use tokio::sync::oneshot;
    let mock = MockPulse::start().await;
    let client = build_client(mock.url());
    let ws = client.ws();
    let ws_clone = ws.clone();
    let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        tokio::select! {
            _ = ws_clone.subscribe(Channel::Book { market: "BTC-PERP".into() }) => {}
            _ = cancel_rx => {}
        }
    });
    // Let the cmd reach the supervisor before cancelling.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let _ = cancel_tx.send(());
    let _ = task.await;
    // Give the supervisor a chance to connect + GC.
    tokio::time::sleep(Duration::from_millis(300)).await;
    // Second subscribe to the SAME channel must succeed - registry must
    // not block it as "already subscribed" / "in flight".
    let mut stream = timeout(
        Duration::from_secs(2),
        ws.subscribe(Channel::Book {
            market: "BTC-PERP".into(),
        }),
    )
    .await
    .expect("second subscribe within 2s")
    .expect("second subscribe ok");
    mock.send(MockCmd::Push {
        kind: "snapshot",
        channel: "book",
        filter: Some("BTC-PERP"),
        gsn: 1,
        data: json!({}),
    })
    .await;
    let evt = timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("event")
        .expect("open");
    assert!(matches!(evt, Event::Update(_)));
    ws.shutdown().await.expect("shutdown");
}

/// Repro for C1 (Notify lost wakeup): kill the conn IMMEDIATELY after
/// welcome, before the supervisor reaches `closed().await`. With Notify
/// the wakeup would be missed and reconnect would block until the 15s
/// probe ping fires. With watch-channel, the supervisor reconnects
/// inside backoff window.
#[tokio::test]
async fn closed_signal_race_immediate_kill_after_welcome() {
    let mock = MockPulse::start().await;
    let client = build_client(mock.url());
    let ws = client.ws();
    // Subscribe so the supervisor opens conn #1.
    let mut stream = ws
        .subscribe(Channel::Book {
            market: "BTC-PERP".into(),
        })
        .await
        .expect("first subscribe");
    // Kill immediately. This races: supervisor may have just entered
    // drive() and may not yet be blocked on closed().
    mock.send(MockCmd::KillConn).await;
    // Within 5s, supervisor must reconnect (conn_seq >= 2) - the watch
    // channel guarantees the close signal isn't lost even if
    // closed().await happens after the driver exited.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while mock.conn_seq().await < 2 {
        if tokio::time::Instant::now() >= deadline {
            panic!("mock did not observe reconnect within 5s - closed signal lost");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // Drain reconnect events.
    let drain_deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let mut saw_reconnected = false;
    while !saw_reconnected {
        let remaining = drain_deadline.saturating_duration_since(tokio::time::Instant::now());
        let Ok(Some(evt)) = timeout(remaining, stream.next()).await else {
            break;
        };
        if matches!(evt, Event::Reconnected) {
            saw_reconnected = true;
        }
    }
    assert!(saw_reconnected, "Reconnected event missing after kill");
    ws.shutdown().await.expect("shutdown");
}

/// `authenticate()` invoked while the supervisor is between connections
/// (or during initial backoff) blocks until the next successful auth
/// replay and returns the resolved address.
#[tokio::test]
async fn disconnected_authenticate_blocks_until_replay() {
    let mock = MockPulse::start().await;
    let client = Client::builder()
        .env(Env::Custom {
            rest: "http://127.0.0.1:1".into(),
            ws: mock.url(),
        })
        .eip712_domain(dummy_domain())
        .api_key("k", "s")
        .build()
        .expect("build client");
    let ws = client.ws();
    // Open + drop a conn so the supervisor is mid-reconnect when
    // authenticate is called. We trigger reconnect by subscribing
    // first (forces a connect), kill, then call authenticate during
    // backoff.
    let mut stream = ws
        .subscribe(Channel::Book {
            market: "BTC-PERP".into(),
        })
        .await
        .expect("first subscribe");
    mock.send(MockCmd::KillConn).await;
    // Authenticate now - supervisor may still be in backoff or mid-connect.
    // With our fix, this blocks until next successful auth.
    let addr = timeout(Duration::from_secs(5), ws.authenticate())
        .await
        .expect("auth completes within 5s")
        .expect("auth ok");
    assert_eq!(addr, "0xMOCKADDR");
    // Drain stream - should see Reconnected. Bound time so a hung
    // supervisor can't deadlock the test.
    let _ = timeout(Duration::from_secs(2), async {
        while let Some(e) = stream.next().await {
            if matches!(e, Event::Reconnected) {
                break;
            }
        }
    })
    .await;
    ws.shutdown().await.expect("shutdown");
}

/// Slow consumer with a full buffer: supervisor must NOT block on
/// `user_tx.send.await`. With the fix (try_send), the sub gets dropped
/// and the supervisor stays responsive.
#[tokio::test]
async fn slow_consumer_does_not_deadlock_supervisor() {
    let mock = MockPulse::start().await;
    let client = build_client(mock.url());
    let ws = client.ws();
    // Subscribe but never read from `stream` - let buffer fill.
    let _stream = ws
        .subscribe(Channel::Book {
            market: "BTC-PERP".into(),
        })
        .await
        .expect("subscribe");
    // Push >256 frames (SUB_USER_BUFFER) WITHOUT reading. Supervisor
    // should drop the sub on Full rather than block.
    for gsn in 1..=300 {
        mock.send(MockCmd::Push {
            kind: "update",
            channel: "book",
            filter: Some("BTC-PERP"),
            gsn,
            data: json!({}),
        })
        .await;
    }
    // Give the supervisor time to process.
    tokio::time::sleep(Duration::from_millis(300)).await;
    // Critical: subscribe to a DIFFERENT channel - if supervisor is
    // wedged on the slow sub, this hangs. Bound the wait.
    let mut other = timeout(
        Duration::from_secs(2),
        ws.subscribe(Channel::Ticker {
            market: "ETH-PERP".into(),
        }),
    )
    .await
    .expect("subscribe to other channel within 2s - supervisor wedged otherwise")
    .expect("subscribe ok");
    mock.send(MockCmd::Push {
        kind: "snapshot",
        channel: "ticker",
        filter: Some("ETH-PERP"),
        gsn: 1,
        data: json!({}),
    })
    .await;
    let evt = timeout(Duration::from_secs(2), other.next())
        .await
        .expect("ticker event")
        .expect("open");
    assert!(matches!(evt, Event::Update(_)));
    ws.shutdown().await.expect("shutdown");
}

/// A consumer that overflows its buffer must receive a terminal
/// `Event::Lagged` marker before the stream ends, so a lag-drop is
/// distinguishable from a clean unsubscribe. The supervisor reserves the last
/// buffer slot for this marker; without that reservation it would be dropped
/// into the already-full channel. Regression test for that reservation.
#[tokio::test]
async fn slow_consumer_receives_lagged_marker_before_end() {
    let mock = MockPulse::start().await;
    let client = build_client(mock.url());
    let ws = client.ws();
    let mut stream = ws
        .subscribe(Channel::Book {
            market: "BTC-PERP".into(),
        })
        .await
        .expect("subscribe");
    // Overflow the 256-slot buffer without reading.
    for gsn in 1..=400 {
        mock.send(MockCmd::Push {
            kind: "update",
            channel: "book",
            filter: Some("BTC-PERP"),
            gsn,
            data: json!({}),
        })
        .await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Drain the buffered updates; the stream must yield exactly one Lagged
    // marker and then end (None), never silently ending without it.
    let mut saw_lagged = false;
    loop {
        match timeout(Duration::from_secs(2), stream.next())
            .await
            .expect("stream must not hang")
        {
            Some(Event::Update(_)) => {}
            Some(Event::Lagged { channel, filter }) => {
                assert_eq!(channel, ChannelName::Book);
                assert_eq!(filter, "BTC-PERP");
                saw_lagged = true;
            }
            Some(other) => panic!("unexpected event: {other:?}"),
            None => break,
        }
    }
    assert!(
        saw_lagged,
        "lagged subscriber must receive Event::Lagged before the stream ends"
    );

    // The lag-drop must also release the server-side subscription on the live
    // connection (route_update -> drop_sub -> conn.unsubscribe), so the socket
    // stops draining it and a resubscribe is not a duplicate.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while !mock.was_unsubbed("book", "BTC-PERP").await {
        if tokio::time::Instant::now() >= deadline {
            panic!("lag-drop did not issue a server-side unsub for the dropped sub");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    ws.shutdown().await.expect("shutdown");
}

/// A reconnect broadcast must not consume the slot reserved for the terminal
/// `Event::Lagged`. Fill the buffer to exactly its last free slot, then trigger
/// a reconnect: the `Reconnected` broadcast must yield the reserved slot to a
/// `Lagged` marker (the consumer is lagging) rather than spend it on the
/// lifecycle event and starve the lag signal. Regression test for the broadcast
/// path mirroring the route_update reservation.
#[tokio::test]
async fn reconnect_broadcast_does_not_starve_lagged_marker() {
    let mock = MockPulse::start().await;
    let client = build_client(mock.url());
    let ws = client.ws();
    let mut stream = ws
        .subscribe(Channel::Book {
            market: "BTC-PERP".into(),
        })
        .await
        .expect("subscribe");
    // Push exactly SUB_USER_BUFFER - 1 (255) updates without reading. Each is
    // delivered while capacity > 1, leaving capacity at exactly 1 (the reserved
    // slot); the sub is not dropped (the 256th frame would be). Nothing else
    // feeds the buffer until the reconnect below, so capacity is exactly 1.
    for gsn in 1..=255 {
        mock.send(MockCmd::Push {
            kind: "update",
            channel: "book",
            filter: Some("BTC-PERP"),
            gsn,
            data: json!({}),
        })
        .await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Reconnect: the supervisor resubscribes and broadcasts Reconnected to this
    // established sub. With only the reserved slot free, that broadcast must
    // become the Lagged drop, not a Reconnected that starves it.
    mock.send(MockCmd::KillConn).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while mock.conn_seq().await < 2 {
        if tokio::time::Instant::now() >= deadline {
            panic!("mock did not see reconnect within 5s");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Drain: the stream must yield a terminal Lagged and then end, never end
    // silently (which is what consuming the reserved slot would cause).
    let mut saw_lagged = false;
    loop {
        match timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("stream must not hang")
        {
            Some(Event::Update(_)) => {}
            Some(Event::Lagged { channel, .. }) => {
                assert_eq!(channel, ChannelName::Book);
                saw_lagged = true;
            }
            // A Reconnected here would mean the reserved slot was spent on the
            // lifecycle event instead of the lag marker.
            Some(Event::Reconnected) => panic!("reserved slot spent on Reconnected, not Lagged"),
            Some(other) => panic!("unexpected event: {other:?}"),
            None => break,
        }
    }
    assert!(
        saw_lagged,
        "reconnect broadcast must yield the reserved slot to Event::Lagged"
    );

    // The lag-drop must also release the server-side subscription (via
    // conn.unsubscribe), otherwise the socket keeps draining it and a
    // resubscribe is rejected as a duplicate. The unsub round-trips after the
    // Lagged marker is queued, so poll briefly for it.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while !mock.was_unsubbed("book", "BTC-PERP").await {
        if tokio::time::Instant::now() >= deadline {
            panic!("lag-drop did not issue a server-side unsub for the dropped sub");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    ws.shutdown().await.expect("shutdown");
}

/// Dropping the `SubscriptionStream` should let the supervisor notice
/// (next data frame) and unsub server-side; AND a fresh subscribe to
/// the same channel afterwards must succeed.
#[tokio::test]
async fn drop_subscription_then_resubscribe_works() {
    let mock = MockPulse::start().await;
    let client = build_client(mock.url());
    let ws = client.ws();
    {
        let _stream = ws
            .subscribe(Channel::Book {
                market: "BTC-PERP".into(),
            })
            .await
            .expect("subscribe");
        // Drop _stream at end of scope.
    }
    // Push a frame - supervisor sees user_tx Closed → unsubscribes
    // server-side and clears the slot.
    mock.send(MockCmd::Push {
        kind: "update",
        channel: "book",
        filter: Some("BTC-PERP"),
        gsn: 1,
        data: json!({}),
    })
    .await;
    // Give time for the cleanup.
    tokio::time::sleep(Duration::from_millis(200)).await;
    // Fresh subscribe - must succeed (slot was GC'd).
    let mut stream = timeout(
        Duration::from_secs(2),
        ws.subscribe(Channel::Book {
            market: "BTC-PERP".into(),
        }),
    )
    .await
    .expect("resubscribe")
    .expect("ok");
    mock.send(MockCmd::Push {
        kind: "snapshot",
        channel: "book",
        filter: Some("BTC-PERP"),
        gsn: 10,
        data: json!({}),
    })
    .await;
    let evt = timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("data")
        .expect("open");
    assert!(matches!(evt, Event::Update(_)));
    ws.shutdown().await.expect("shutdown");
}

/// First-time subscribe AFTER a reconnect already happened should NOT
/// receive `Reconnected` as its first event - from this caller's POV
/// they just subscribed.
#[tokio::test]
async fn first_subscribe_after_reconnect_does_not_see_reconnected() {
    let mock = MockPulse::start().await;
    let client = build_client(mock.url());
    let ws = client.ws();
    // Trigger a reconnect by subscribing then killing the conn.
    let mut existing = ws
        .subscribe(Channel::Book {
            market: "BTC-PERP".into(),
        })
        .await
        .expect("first subscribe");
    mock.send(MockCmd::KillConn).await;
    // Wait for reconnect.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while mock.conn_seq().await < 2 {
        if tokio::time::Instant::now() >= deadline {
            panic!("no reconnect");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // Drain Reconnected from existing.
    let _ = timeout(Duration::from_secs(2), async {
        while let Some(e) = existing.next().await {
            if matches!(e, Event::Reconnected) {
                break;
            }
        }
    })
    .await;
    // Now subscribe to a DIFFERENT channel - fresh sub. Push a frame.
    let mut fresh = ws
        .subscribe(Channel::Ticker {
            market: "ETH-PERP".into(),
        })
        .await
        .expect("fresh subscribe");
    mock.send(MockCmd::Push {
        kind: "snapshot",
        channel: "ticker",
        filter: Some("ETH-PERP"),
        gsn: 1,
        data: json!({}),
    })
    .await;
    let first = timeout(Duration::from_secs(2), fresh.next())
        .await
        .expect("first event")
        .expect("open");
    // Caller's first event MUST be data, not Reconnected - we never
    // experienced a disconnect from this sub's POV.
    assert!(
        matches!(first, Event::Update(_)),
        "expected data first, got {first:?}"
    );
    ws.shutdown().await.expect("shutdown");
}

/// Repro for P1-A (mid-replay conn death): the supervisor must NOT drop
/// active subs from the registry when the freshly-acquired connection
/// dies during sub replay. Setup: two subs, kill conn #1 to trigger
/// reconnect, then kill conn #2 immediately on first sub-ack so the
/// replay loop sees `connection task is gone` mid-flight. Conn #3 must
/// re-replay BOTH subs (registry intact), and both streams must keep
/// delivering data afterwards.
///
/// Without the fix, the second sub gets `Unauthorized` + dropped from
/// the registry on conn #2's death and never returns even after conn #3
/// stabilizes.
#[tokio::test]
async fn mid_replay_conn_death_preserves_subs() {
    let mock = MockPulse::start().await;
    let client = build_client(mock.url());
    let ws = client.ws();
    // Two subs, both on conn #1.
    let mut book = ws
        .subscribe(Channel::Book {
            market: "BTC-PERP".into(),
        })
        .await
        .expect("first book");
    let mut tick = ws
        .subscribe(Channel::Ticker {
            market: "ETH-PERP".into(),
        })
        .await
        .expect("first ticker");
    // Kill conn #1 - supervisor reconnects to conn #2 and begins replay.
    mock.send(MockCmd::KillConn).await;
    // Wait until we see conn #2 then immediately kill it. Racy by design:
    // we want the kill to land while conn #2's replay loop is still
    // iterating subs.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while mock.conn_seq().await < 2 {
        if tokio::time::Instant::now() >= deadline {
            panic!("no conn #2");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    mock.send(MockCmd::KillConn).await;
    // Wait for conn #3 - supervisor must reconnect AGAIN with both
    // registry slots intact.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while mock.conn_seq().await < 3 {
        if tokio::time::Instant::now() >= deadline {
            panic!("no conn #3 - supervisor stalled");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Push to both channels. Both streams must still deliver - proving
    // the supervisor re-subscribed both.
    mock.send(MockCmd::Push {
        kind: "snapshot",
        channel: "book",
        filter: Some("BTC-PERP"),
        gsn: 100,
        data: json!({}),
    })
    .await;
    mock.send(MockCmd::Push {
        kind: "snapshot",
        channel: "ticker",
        filter: Some("ETH-PERP"),
        gsn: 100,
        data: json!({"bid":{"px":"1","sz":"1"},"ask":{"px":"2","sz":"1"}}),
    })
    .await;
    // Drain each stream until we see Update - Reconnected may interleave.
    async fn await_update<S: futures_util::Stream<Item = Event> + Unpin>(s: &mut S) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let evt = match timeout(remaining, s.next()).await {
                Ok(Some(e)) => e,
                Ok(None) => panic!("stream ended unexpectedly - sub got dropped from registry"),
                Err(_) => panic!("no Update within deadline - sub silently lost"),
            };
            if matches!(evt, Event::Update(_)) {
                return;
            }
        }
    }
    await_update(&mut book).await;
    await_update(&mut tick).await;
    ws.shutdown().await.expect("shutdown");
}

/// `SubscriptionStream` implements `FusedStream`: after it yields `None` it must
/// report `is_terminated() == true`, the contract `select!`/stream combinators
/// rely on. Pre-poll it reports `false` (the documented post-poll semantics).
#[tokio::test]
async fn subscription_stream_is_fused_after_termination() {
    use futures_util::stream::FusedStream;

    let mock = MockPulse::start().await;
    let client = build_client(mock.url());
    let ws = client.ws();
    let mut stream = ws
        .subscribe(Channel::Book {
            market: "BTC-PERP".into(),
        })
        .await
        .expect("subscribe");
    assert!(!stream.is_terminated(), "fresh stream is not terminated");

    // Shutdown drops the supervisor's senders, ending the stream.
    ws.shutdown().await.expect("shutdown");
    while timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("stream must not hang")
        .is_some()
    {}
    assert!(
        stream.is_terminated(),
        "stream must report terminated after yielding None"
    );
}
