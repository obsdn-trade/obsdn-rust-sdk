//! End-to-end staging tests against the live matching engine + pulse WS.
//!
//! Two tests:
//! - `e2e_combined_flow`: register → faucet → ws auth → subscribe private order
//!   (wildcard) → place via REST → **observe the order update over WS** →
//!   cancel via REST → observe the cancel → set leverage → cleanup. One account
//!   lifecycle proves C1 (Order uint16), C2 (Register 4-field), H1 (portfolio
//!   RPCs) on the REST side AND the WS wildcard-routing fix on the same flow:
//!   `Order { market: None }` must receive updates the server stamps with a
//!   concrete market.
//! - `e2e_ws_public_book`: public book channel, no auth - snapshot-first
//!   ordering + a follow-up update, live `as_book` deserialization.
//!
//! Run: OBSDN_STAGING=1 cargo test --test e2e_staging -- --nocapture --test-threads=1
//!
//! GSN per channel is logged, never asserted contiguous: pulse `gsn` is a single
//! global event watermark bumped across all channels, so per-subscription values
//! jump arbitrarily. The logs characterize the real (sparse) sequencing.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use obsdn_sdk::sign::{
    self, signature_hex, DelegatedSignerPayload, OrderPayload, OrderSide, RegisterPayload,
};
use obsdn_sdk::types::v1::{
    CancelAllOrdersRequest, FaucetRequest, PlaceOrderRequest, RegisterSignerRequest,
    SetLeverageRequest,
};
use obsdn_sdk::ws::{Channel, WsEvent, WsUpdate, WsUpdateKind};
use obsdn_sdk::{Client, Env, LocalSigner};

fn skip() -> bool {
    if std::env::var("OBSDN_STAGING").is_err() {
        eprintln!("skipping: set OBSDN_STAGING=1 to enable");
        return true;
    }
    false
}

fn nonce() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

/// Generous per-event ceiling - staging can be quiet and a placed/cancelled
/// order has to round-trip through the matching engine before its update fans
/// back out over the socket.
const EVENT_TIMEOUT: Duration = Duration::from_secs(20);

struct TestAccount {
    client: Client,
    sender: Arc<LocalSigner>,
    signer: Arc<LocalSigner>,
}

/// Register a fresh signer, returning an authed client. Proves C2 (4-field
/// Register struct accepted by the server).
async fn setup_test_account() -> TestAccount {
    let sender =
        LocalSigner::from_hex("0x0000000000000000000000000000000000000000000000000000000000000001")
            .unwrap();
    let sender_addr = obsdn_sdk::EipSigner::address(&sender);

    let signer =
        LocalSigner::from_hex("0x0000000000000000000000000000000000000000000000000000000000000002")
            .unwrap();
    let signer_addr = obsdn_sdk::EipSigner::address(&signer);

    let domain = sign::sdk_domain(&Env::Staging);
    let n = nonce();
    let message = "rust-sdk-e2e-test".to_string();

    // C2: Register struct now includes sender field - this proves the 4-field struct works.
    let sndr_sig = sign::sign_register(
        &sender,
        &domain,
        RegisterPayload {
            sender: sender_addr,
            signer: signer_addr,
            message: message.clone(),
            nonce: n,
        },
    )
    .unwrap();

    let signer_sig = sign::sign_delegated_signer(
        &signer,
        &domain,
        DelegatedSignerPayload {
            account: sender_addr,
        },
    )
    .unwrap();

    let unauthed = Client::builder().env(Env::Staging).build().unwrap();

    let req = RegisterSignerRequest {
        sndr_addr: format!("{}", sender_addr),
        signer_addr: format!("{}", signer_addr),
        nonce: n,
        sndr_sig: signature_hex(&sndr_sig),
        signer_sig: signature_hex(&signer_sig),
        msg: message,
        nm: "rust-sdk-e2e".into(),
        eoa_only: true,
    };
    eprintln!("DEBUG register request:");
    eprintln!("  sndr_addr:   {}", req.sndr_addr);
    eprintln!("  signer_addr: {}", req.signer_addr);
    eprintln!("  nonce:       {}", req.nonce);

    let reg_resp = unauthed
        .auth_api()
        .register_signer(req)
        .await
        .expect("C2: register_signer should accept 4-field Register struct");

    let api_key = reg_resp.api_key.as_ref().expect("should return api_key");
    eprintln!(
        "OK C2: registered signer. api_key={}",
        &api_key.api_key[..8]
    );

    let sender = Arc::new(sender);
    let signer = Arc::new(signer);
    let client = Client::builder()
        .env(Env::Staging)
        .api_key(&api_key.api_key, &api_key.api_secret)
        .eip_signer(signer.clone())
        .build()
        .unwrap();

    TestAccount {
        client,
        sender,
        signer,
    }
}

/// Place a resting (far-from-market) limit buy so it sits on the book without
/// matching - returns its oid. Far price keeps the position flat, so the flow
/// exercises the order channel only. Proves C1 (uint16 marketIndex signature).
async fn place_resting_order(acct: &TestAccount, market: &str) -> String {
    let sender_addr = obsdn_sdk::EipSigner::address(acct.sender.as_ref());
    let domain = acct.client.eip712_domain().clone();
    let market_info = acct
        .client
        .resolve_market(market)
        .await
        .expect("resolve market");
    let market_index: u16 = market_info.idx.parse().expect("idx as u16");

    let order_nonce = nonce();
    let payload = OrderPayload {
        sender: sender_addr,
        market_index,
        side: OrderSide::Buy,
        size: sign::scale_f64(0.0001).unwrap(),
        price: sign::scale_f64(1000.0).unwrap(),
        nonce: order_nonce,
    };
    let sig = sign::sign_order(acct.signer.as_ref(), &domain, payload).unwrap();

    let place_resp = acct
        .client
        .orders()
        .place(PlaceOrderRequest {
            mkt_id: market.into(),
            sd: 1, // BUY
            ot: 1, // LIMIT
            sz: 0.0001,
            px: 1000.0,
            nonce: order_nonce,
            sig: signature_hex(&sig),
            ..Default::default()
        })
        .await
        .expect("C1: place order should accept uint16 marketIndex signature");

    place_resp
        .ord
        .as_ref()
        .expect("should have order")
        .oid
        .clone()
}

/// Pull the next [`WsEvent::Update`] off a subscription within [`EVENT_TIMEOUT`],
/// skipping lifecycle markers. Returns `None` on timeout or stream end.
async fn next_update<S>(stream: &mut S) -> Option<WsUpdate>
where
    S: futures_util::Stream<Item = WsEvent> + Unpin,
{
    loop {
        match tokio::time::timeout(EVENT_TIMEOUT, stream.next()).await {
            Ok(Some(WsEvent::Update(u))) => return Some(u),
            Ok(Some(WsEvent::Reconnected)) => {
                eprintln!("  (reconnected - continuing)");
                continue;
            }
            Ok(Some(WsEvent::Unauthorized(msg))) => panic!("unexpected Unauthorized: {msg}"),
            Ok(None) => return None, // stream ended
            Err(_) => return None,   // timeout
        }
    }
}

/// Scan up to `max_frames` order frames for one carrying `oid` (other order
/// churn may interleave). Returns the matching [`OrderView`] state on hit.
async fn await_order_update<S>(
    stream: &mut S,
    oid: &str,
    max_frames: usize,
) -> Option<obsdn_sdk::ws::OrderView>
where
    S: futures_util::Stream<Item = WsEvent> + Unpin,
{
    for _ in 0..max_frames {
        let u = next_update(stream).await?;
        let orders = u.as_orders().expect("decode order update");
        eprintln!(
            "  order frame: gsn={} kind={:?} filter={:?} oids=[{}]",
            u.gsn,
            u.kind,
            u.filter,
            orders
                .iter()
                .map(|o| o.oid.as_str())
                .collect::<Vec<_>>()
                .join(",")
        );
        if let Some(o) = orders.into_iter().find(|o| o.oid == oid) {
            return Some(o);
        }
    }
    None
}

/// Full lifecycle on one registered account, with the WS as live observer of
/// the REST mutations.
#[tokio::test]
async fn e2e_combined_flow() {
    if skip() {
        return;
    }

    // --- C2: Register signer (4-field struct) ---
    let acct = setup_test_account().await;
    let client = &acct.client;
    let sender_addr = obsdn_sdk::EipSigner::address(acct.sender.as_ref());

    // --- Faucet staging USDC (best-effort - may need Twingate) ---
    let faucet_resp = client
        .account()
        .faucet(FaucetRequest {
            usr_addr: format!("{:#x}", sender_addr),
            asset: "USDC".into(),
            amt: "10000".into(),
            on_chain: false,
        })
        .await;
    match &faucet_resp {
        Ok(_) => eprintln!("OK: faucet 10000 USDC"),
        Err(e) => eprintln!("WARN: faucet failed (may need Twingate): {e}"),
    }

    // --- WS: authenticate + subscribe private order (wildcard) ---
    let ws = client.ws();
    let address = ws.authenticate().await.expect("ws authenticate");
    eprintln!("OK: ws authenticated as {address}");

    let mut orders = ws
        .subscribe(Channel::Order { market: None })
        .await
        .expect("subscribe order wildcard");

    // Drain the initial snapshot (current open orders - may be empty for a
    // fresh account but still arrives as a frame).
    if let Some(snap) = next_update(&mut orders).await {
        eprintln!(
            "order snapshot: gsn={} kind={:?} count={}",
            snap.gsn,
            snap.kind,
            snap.as_orders().map(|o| o.len()).unwrap_or(0)
        );
    }

    // --- C1: place a resting order via REST → observe it over WS ---
    let oid = place_resting_order(&acct, "BTC-PERP").await;
    eprintln!("OK C1: placed order {oid}, awaiting wildcard WS update...");

    let placed = await_order_update(&mut orders, &oid, 5).await.expect(
        "wildcard Order{market:None} must receive the placed order update (proves routing)",
    );
    assert_eq!(placed.oid, oid);
    eprintln!(
        "OK HIGH-1: wildcard sub received placed order, st={}",
        placed.st
    );

    // --- Cancel via REST → observe the cancel over WS ---
    client.orders().cancel(&oid).await.expect("cancel order");
    eprintln!("cancelled {oid} via REST, awaiting cancel WS update...");

    let cancelled = await_order_update(&mut orders, &oid, 5)
        .await
        .expect("wildcard sub must receive the cancel update");
    eprintln!(
        "OK: wildcard sub received cancel update, st={} cancel_req={} done_rsn={}",
        cancelled.st, cancelled.cancel_req, cancelled.done_rsn
    );

    // --- H1: SetLeverage ---
    let lev_resp = client
        .portfolio()
        .set_leverage(SetLeverageRequest {
            mkt_id: "BTC-PERP".into(),
            lev: 5,
        })
        .await;
    match &lev_resp {
        Ok(r) => eprintln!("OK H1: set_leverage ok={}", r.ok),
        Err(e) => eprintln!("WARN H1: set_leverage: {e} (may need open position)"),
    }

    // --- Cleanup ---
    let _ = client
        .orders()
        .cancel_all(CancelAllOrdersRequest::default())
        .await;
    ws.shutdown().await.ok();

    eprintln!("\n=== E2E COMBINED FLOW PASSED ===");
    eprintln!("  C2: Register 4-field struct       - VERIFIED (signer registered)");
    eprintln!("  C1: Order uint16 marketIndex      - VERIFIED (order placed + accepted)");
    eprintln!("  HIGH-1: WS wildcard order routing - VERIFIED (place + cancel observed over WS)");
    eprintln!("  H1: SetLeverage endpoint          - TESTED");
}

/// Public book channel - no auth. First frame must be a `Snapshot` with a
/// populated book; a follow-up frame should arrive (staging book churns).
/// Proves snapshot-before-update ordering and live `as_book` deserialization.
#[tokio::test]
async fn e2e_ws_public_book() {
    if skip() {
        return;
    }

    let client = Client::builder().env(Env::Staging).build().unwrap();
    let ws = client.ws();

    let mut stream = ws
        .subscribe(Channel::Book {
            market: "BTC-PERP".into(),
        })
        .await
        .expect("subscribe book");

    let first = next_update(&mut stream)
        .await
        .expect("should receive first book frame");
    eprintln!("first book frame: gsn={} kind={:?}", first.gsn, first.kind);
    assert_eq!(
        first.kind,
        WsUpdateKind::Snapshot,
        "first book frame must be a snapshot"
    );
    let book = first.as_book().expect("decode book snapshot");
    assert!(
        !book.bids.is_empty() || !book.asks.is_empty(),
        "snapshot book should have at least one side populated"
    );
    eprintln!(
        "  snapshot: {} bids, {} asks, checksum={}",
        book.bids.len(),
        book.asks.len(),
        book.checksum
    );

    let second = next_update(&mut stream)
        .await
        .expect("should receive a follow-up book frame (staging book churns)");
    eprintln!(
        "follow-up book frame: gsn={} kind={:?} (delta vs snapshot gsn={})",
        second.gsn,
        second.kind,
        second.gsn as i128 - first.gsn as i128
    );
    second.as_book().expect("decode book follow-up");

    ws.shutdown().await.ok();
    eprintln!("=== E2E WS PUBLIC BOOK PASSED ===");
}
