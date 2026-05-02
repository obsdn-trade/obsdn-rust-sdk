# obsdn-sdk (Rust)

Async Rust SDK for the [OBSDN](https://obsdn.trade) perpetual exchange.

- **REST** — full coverage of the public service surface (~50 RPCs across 11 handles: orders, markets, account, asset, auth, chain, general, portfolio, price, subaccount, vault).
- **EIP-712 signing** — local secp256k1 signer with byte-equal output to the Go reference (`pkg/ethsig`). Order, Transfer, Withdraw, Vault {Create,Stake,Unstake}, Subaccount, Register, DelegatedSigner.
- **WebSocket** — managed client with auto-reconnect, GSN gap detection, exponential backoff, auth replay, and typed views per channel (`book`, `ticker`, `oracle`, `trade`, `order`).

## Status

Phases 1–6 implemented. Phase 7 (this) wires examples + ergonomics + docs. Phase 8 (CI) follows. Crate is `publish = false` — used as a path dep from the OBSDN monorepo.

## Install (path dependency)

```toml
[dependencies]
obsdn-sdk = { path = "../path/to/sdk/rust" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

## Environment variables

| Variable             | Purpose                                                                  |
|----------------------|--------------------------------------------------------------------------|
| `OBSDN_API_KEY`      | HMAC API key — required for authenticated endpoints / private channels. |
| `OBSDN_API_SECRET`   | HMAC API secret. Pair with `OBSDN_API_KEY`.                              |
| `OBSDN_PRIVATE_KEY`  | secp256k1 private key (hex, `0x`-prefixed or bare) for EIP-712 signing.  |
| `OBSDN_ENV`          | Optional — `staging` (default) / `production` / `local`.                 |
| `RUST_LOG`           | Standard `tracing-subscriber` filter (e.g. `obsdn_sdk=debug,info`).      |

> ⚠️ Never commit secrets. Use a local `.env` (gitignored) or your secret manager.

## Quick start

```rust
use obsdn_sdk::{Client, Env};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = Client::builder()
        .env(Env::Staging)
        .api_key(std::env::var("OBSDN_API_KEY")?, std::env::var("OBSDN_API_SECRET")?)
        .build()?;
    let markets = client.markets().get_markets().await?;
    println!("{} markets available", markets.mkts.len());
    Ok(())
}
```

Place an order in three lines (resolve market index + sign + post via the
ergonomic helper):

```rust
use std::sync::Arc;
use obsdn_sdk::rest::orders::PlaceEasy;
use obsdn_sdk::types::v1::OrderSide;
use obsdn_sdk::{Client, Env, LocalSigner};

let signer = Arc::new(LocalSigner::from_hex(&std::env::var("OBSDN_PRIVATE_KEY")?)?);
let client = Client::builder()
    .env(Env::Staging)
    .api_key(key, secret)
    .eip_signer(signer)
    .build()?;
client
    .orders()
    .place_easy(PlaceEasy::limit("BTC-PERP", OrderSide::Buy, 50_000.0, 0.001))
    .await?;
```

## Examples

All under `examples/`. Run with `cargo run --example NAME`.

| Example              | What it shows                                                            |
|----------------------|--------------------------------------------------------------------------|
| `place_order`        | REST + EIP-712 signing via `place_easy`. Quotes 5% under mark.           |
| `cancel_order`       | Cancel by order id (HMAC-only, no EIP-712 needed).                       |
| `ws_book`            | Public managed WS, typed `BookView`, prints 10 frames.                   |
| `ws_private_orders`  | HMAC auth on WS + `Channel::Order`, streams every order lifecycle event. |
| `transfer`           | Sign EIP-712 `Transfer`, post `/transfers/send-funds`.                   |
| `withdraw`           | Sign EIP-712 `Withdraw`, post `/transfers/withdraw`.                     |
| `book_with_resync`   | Maintain a local book; on `Gap`, refetch via REST snapshot.              |

## Layout

```
sdk/rust/
├── Cargo.toml          # Crate manifest (path dep on api/proto)
├── Cargo.lock          # Committed for reproducibility
├── build.rs            # buf export → prost+pbjson codegen
├── README.md           # ← you are here
├── examples/           # Runnable cargo examples (see above)
├── scripts/            # Go-side EIP-712 fixture exporter
├── src/
│   ├── lib.rs
│   ├── builder.rs      # Client + ClientBuilder
│   ├── env.rs          # Local / Staging / Production / Custom
│   ├── error.rs
│   ├── auth.rs         # HMAC signer
│   ├── market_cache.rs # Lazy market-index cache (TTL 60s)
│   ├── rest/           # REST handles + helpers + auth layer
│   ├── sign/           # EIP-712 templates + LocalSigner
│   ├── types/          # Generated wire types
│   └── ws/             # Managed WS, typed views, GSN tracking
└── tests/              # Codegen smoke, REST contract, EIP-712 golden, WS chaos
```

## Build / test

```bash
make sdk.rust.check     # fmt + clippy -D warnings + test (the CI gate)
cargo doc --no-deps     # browse the rendered docs
cargo build --examples  # ensure all examples still compile
```

`buf` must be installed at build time (codegen runs on each `cargo build`). Run `make codegen` once on a fresh clone to populate the buf cache.

## Documentation

- Architecture overview (ASCII diagrams): [`docs/architecture.md`](docs/architecture.md).
- API reference: `cargo doc --open` after a successful build. Internal-only proto fields/endpoints are rendered as `#[doc(hidden)]` and don't appear in the generated docs but remain reachable.
- Implementation plan + design notes: `plans/260424-0946-rust-sdk/` (gitignored — internal).
- WebSocket protocol: `docs/api/ws-integration.md`, `docs/api/websocket-channels.md`.

## License

Dual-licensed under MIT or Apache 2.0, matching the parent repo.
