# obsdn-sdk (Rust) - architecture

ASCII map of what the SDK does, what it holds in memory, and what it
deliberately leaves to the caller. Pair with `cargo doc --open` for the
full API reference; this file is the orientation, not the spec.

## Top-level shape

```
┌────────────────────────────────────────────────────────────────────────────┐
│                           Your bot / strategy                              │
└─────────────────────────────────┬──────────────────────────────────────────┘
                                  │
                       Client::builder()
                         .env(Env::Production)
                         .api_key(k, s)        ← HMAC for REST/WS auth
                         .eip712_signer(local)    ← EIP-712 for orders/transfers
                         .build()?
                                  │
                                  ▼
┌────────────────────────────────────────────────────────────────────────────┐
│                              Client (Arc-clone-cheap)                      │
│                                                                            │
│   .markets()    .orders()    .portfolio()   .price()   .account()    .ws() │
│       │             │              │            │           │          │   │
└───────┼─────────────┼──────────────┼────────────┼───────────┼──────────┼───┘
        │             │              │            │           │          │
        ▼             ▼              ▼            ▼           ▼          ▼
  ┌──────────────────────────────────────────────────┐  ┌────────────────────┐
  │                 REST layer (rest/)               │  │   WS managed       │
  │                                                  │  │   (ws/managed.rs)  │
  │  ┌──────────────────────────────────────────┐    │  │                    │
  │  │ Markets  Orders  Portfolio  …   │    │  │  WsClient          │
  │  └──────────────────────────────────────────┘    │  │   ├─ supervisor    │
  │                       │                          │  │   │  (reconnect+   │
  │                       ▼                          │  │   │   exp backoff) │
  │  ┌──────────────────────────────────────────┐    │  │   ├─ subs registry │
  │  │   RestClient  (reqwest::Client, Arc)     │    │  │   ├─ no gap detect │
  │  │   ├─ HMAC sign (ts+method+path+body)     │    │  │   │  (raw gsn)     │
  │  │   ├─ retry on 5xx / network              │    │  │   └─ replay subs   │
  │  │   ├─ JSON-named wire types               │    │  │      after reconn  │
  │  │   └─ Error enum (Http/Api/Sign/Config)   │    │  │                    │
  │  └──────────────────────────────────────────┘    │  │  Channel::Book{…}  │
  │                                                  │  │  Channel::Order    │
  │  place_limit() flow:                              │  │  Channel::Trade    │
  │   1. resolve_market(mkt_id) → MarketCache        │  │  Channel::Event    │
  │   2. scale_f64 size/px → 18-dec fixed            │  │  Channel::Position │
  │   3. sign EIP-712 Order (sign/order.rs)          │  │  Channel::Oracle   │
  │   4. POST /orders with HMAC + sig                │  │  Channel::Ticker   │
  └────────────────┬─────────────────────────────────┘  └──────────┬─────────┘
                   │                                               │
                   │  HTTPS                                        │  WSS
                   ▼                                               ▼
        ┌──────────────────────────┐                  ┌──────────────────────┐
        │   nova  (REST/gRPC API)  │                  │   pulse  (WebSocket) │
        │   :8080 / :9090          │                  │   :8082 /ws          │
        └──────────────────────────┘                  └──────────────────────┘
```

## Internal helpers

```
  market_cache.rs       60s lazy TTL of GetMarkets, single-flight, used to
                        map "BTC-PERP" → market_index for signing

  sign/order.rs         EIP-712 Order template (sender, market_index, side,
                        size_x18, price_x18, nonce). Domain from .env

  sign/transfer.rs      EIP-712 Transfer / Withdraw templates

  auth.rs               HMAC-SHA256 signer for REST + WS handshake

  types/v1/             generated wire types committed under src/types/generated/

  ws/views.rs           Per-frame parsers: BookView TickerView OracleView
                        TradeView OrderView. NOT running state - decode +
                        drop. Caller owns aggregation.

  ws/managed.rs         Supervisor: one socket multiplexes every sub,
                        auto-reconnect + auth/sub replay. No gap detection;
                        gsn is exposed raw and a slow consumer is dropped
                        with a terminal Event::Lagged. Resync via REST.
```

## Event flow on WS

```
   pulse  ──frame──▶  Connection  ──▶  Supervisor  ──▶  SubscriptionStream
                       │                    │
                       │                    └─ data frame    → Event::Update
                       │                    └─ slow consumer  → Event::Lagged
                       │                       (sub buffer full; sub dropped)
                       │
                       ├─ disconnect      → supervisor backoff + reconnect
                       │                  → re-subscribe all subs
                       │                  → emit Event::Reconnected
                       │                    (next frame is fresh Snapshot)
                       │
                       └─ 401/403         → Event::Unauthorized(msg)
```

## Order placement happy path

```
  bot.place_limit(LimitOrder::new("BTC-PERP", Buy, 50_000.0, 0.001))
       │
       ▼
  Orders::place_limit
       │  reject if order_type ≠ Limit          (no true MARKET on server)
       │  reject if side ≠ Buy/Sell
       ▼
  Client::resolve_market("BTC-PERP")
       │  MarketCache hit? return cached. miss? fetch /markets, swap Arc.
       ▼
  scale_f64(0.001) → "1000000000000000"  (18-dec fixed)
  scale_f64(50_000.0) → "50000000000000000000000"
       │
       ▼
  sign_order(local_signer, eip_domain, OrderPayload{ … })
       │  → 65-byte ECDSA sig (r||s||v)
       ▼
  RestClient::post("/orders", PlaceOrderRequest{ sig, … }, Auth::Required)
       │  HMAC headers: x-api-key, x-api-ts, x-api-sig
       ▼
  nova → orbit (sequencer) → matching engine → reply
       ▼
  PlaceOrderResponse  (await_match=false → returns at commit;
                       await_match=true  → returns post-execution)
```

## What it ISN'T

```
  ✗ Not a Hummingbot-style stateful client
  ✗ No live order book maintained for you (you keep the BTreeMap)
  ✗ No open-orders / positions / balances cache
  ✗ No strategy framework, no risk layer, no portfolio analytics
  ✗ No gap detection (gsn is a sparse watermark; resync via REST on reconnect)
```

## Crate layout

```
  obsdn-rust-sdk/
  ├── Cargo.toml          standalone crate
  ├── examples/           place_order, cancel_order, ws_book,
  │                       ws_private_orders, transfer, withdraw,
  │                       book_with_resync
  ├── scripts/            codegen-rust (regenerates committed wire types)
  └── src/
      ├── lib.rs          re-exports Client, Env, LocalSigner
      ├── builder.rs      ClientBuilder, Client (Arc handle)
      ├── market_cache.rs lazy TTL cache
      ├── auth.rs         HMAC signer
      ├── error.rs        Error enum + Result alias
      ├── rest/           orders / portfolio / markets / price /
      │                   account + RestClient + query helpers
      ├── sign/           EIP-712 order/transfer/withdraw + LocalSigner
      ├── types/          generated wire types (committed under generated/)
      └── ws/             managed.rs (top-level), connection.rs (raw),
                          channel.rs, event.rs, frame.rs, views.rs, auth.rs
```
