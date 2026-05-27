# obsdn-sdk (Rust) — architecture

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
                         .eip_signer(local)    ← EIP-712 for orders/transfers
                         .build()?
                                  │
                                  ▼
┌────────────────────────────────────────────────────────────────────────────┐
│                              Client (Arc-clone-cheap)                      │
│                                                                            │
│   .markets()    .orders()    .portfolio()   .price()   .transfers()  .ws() │
│       │             │              │            │           │          │   │
└───────┼─────────────┼──────────────┼────────────┼───────────┼──────────┼───┘
        │             │              │            │           │          │
        ▼             ▼              ▼            ▼           ▼          ▼
  ┌──────────────────────────────────────────────────┐  ┌────────────────────┐
  │                 REST layer (rest/)               │  │   WS managed       │
  │                                                  │  │   (ws/managed.rs)  │
  │  ┌──────────────────────────────────────────┐    │  │                    │
  │  │ MarketsApi  OrdersApi  PortfolioApi  …   │    │  │  WsClient          │
  │  └──────────────────────────────────────────┘    │  │   ├─ supervisor    │
  │                       │                          │  │   │  (reconnect+   │
  │                       ▼                          │  │   │   exp backoff) │
  │  ┌──────────────────────────────────────────┐    │  │   ├─ subs registry │
  │  │   RestClient  (reqwest::Client, Arc)     │    │  │   ├─ GSN tracker   │
  │  │   ├─ HMAC sign (ts+method+path+body)     │    │  │   │  (gsn.rs)      │
  │  │   ├─ retry on 5xx / network              │    │  │   └─ replay subs   │
  │  │   ├─ JSON-named proto via pbjson         │    │  │      after reconn  │
  │  │   └─ Error enum (Http/Api/Sign/Config)   │    │  │                    │
  │  └──────────────────────────────────────────┘    │  │  Channel::Book{…}  │
  │                                                  │  │  Channel::Order    │
  │  place_easy() flow:                              │  │  Channel::Trade    │
  │   1. resolve_market(mkt_id) → MarketCache        │  │  Channel::Fill     │
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

  types/v1/             prost+pbjson generated from api/proto/nil/v1/

  ws/views.rs           Per-frame parsers: BookView TickerView OracleView
                        TradeView OrderView. NOT running state — decode +
                        drop. Caller owns aggregation.

  ws/gsn.rs             Tracks last_gsn per (channel,market). Emits Gap
                        when next != last+1. Caller does REST resync.
```

## Event flow on WS

```
   pulse  ──frame──▶  Connection  ──▶  GsnTracker  ──▶  SubscriptionStream
                       │                    │
                       │                    └─ contiguous? → WsEvent::Update
                       │                    └─ skipped?    → WsEvent::Gap{from,to}
                       │
                       ├─ disconnect      → supervisor backoff + reconnect
                       │                  → re-subscribe all subs
                       │                  → emit WsEvent::Reconnected
                       │                    (next frame is fresh Snapshot)
                       │
                       └─ 401/403         → WsEvent::Unauthorized(msg)
```

## Order placement happy path

```
  bot.place_easy(PlaceEasy::limit("BTC-PERP", Buy, 50_000.0, 0.001))
       │
       ▼
  OrdersApi::place_easy
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
  ✗ No auto-resync on Gap (SDK signals; you decide what to refetch)
```

## Crate layout

```
  obsdn-rust-sdk/
  ├── Cargo.toml          standalone crate
  ├── examples/           place_order, cancel_order, ws_book,
  │                       ws_private_orders, transfer, withdraw,
  │                       book_with_resync
  ├── scripts/            codegen-rust (proto → prost+pbjson)
  └── src/
      ├── lib.rs          re-exports Client, Env, LocalSigner
      ├── builder.rs      ClientBuilder, Client (Arc handle)
      ├── env.rs          Local / Staging / Production / Custom
      ├── market_cache.rs lazy TTL cache
      ├── auth.rs         HMAC signer
      ├── error.rs        Error enum + Result alias
      ├── rest/           orders / portfolio / markets / price /
      │                   transfers + RestClient + query helpers
      ├── sign/           EIP-712 order/transfer/withdraw + LocalSigner
      ├── types/          generated wire types (nil.v1)
      └── ws/             managed.rs (top-level), connection.rs (raw),
                          channel.rs, event.rs, frame.rs, views.rs, gsn.rs
```
