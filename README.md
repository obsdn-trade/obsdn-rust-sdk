# obsdn-sdk (Rust)

Async Rust SDK for the [OBSDN](https://obsdn.trade) perpetual exchange.

- **REST** — full coverage of the public service surface (~50 RPCs across 11 handles: orders, markets, account, asset, auth, chain, general, portfolio, price, subaccount, vault).
- **EIP-712 signing** — local secp256k1 signer with byte-equal output to the Go reference (`pkg/ethsig`). Order, Transfer, Withdraw, Vault {Create,Stake,Unstake}, Subaccount, Register, DelegatedSigner.
- **WebSocket** — managed client with auto-reconnect, GSN gap detection, exponential backoff, auth replay, and typed views per channel (`book`, `ticker`, `oracle`, `trade`, `order`).

## Status

`publish = false` — not on crates.io. Intended to be imported / forked by integrating market-makers.

## Install (git dependency)

```toml
[dependencies]
obsdn-sdk = { git = "https://github.com/obsdn-trade/obsdn-rust-sdk", branch = "master" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Or as a git submodule:

```bash
git submodule add git@github.com:obsdn-trade/obsdn-rust-sdk.git vendor/obsdn-sdk
```

```toml
obsdn-sdk = { path = "vendor/obsdn-sdk" }
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
.
├── Cargo.toml          # Crate manifest
├── Cargo.lock          # Committed for reproducibility
├── README.md           # ← you are here
├── examples/           # Runnable cargo examples (see above)
├── scripts/
│   ├── codegen-rust/   # Out-of-band proto codegen (see "Codegen" below)
│   └── ...             # Go-side EIP-712 fixture exporter
├── src/
│   ├── lib.rs
│   ├── builder.rs      # Client + ClientBuilder
│   ├── env.rs          # Local / Staging / Production / Custom
│   ├── error.rs
│   ├── auth.rs         # HMAC signer
│   ├── market_cache.rs # Lazy market-index cache (TTL 60s)
│   ├── rest/           # REST handles + helpers + auth layer
│   ├── sign/           # EIP-712 templates + LocalSigner
│   ├── types/          # Generated wire types (committed under generated/)
│   └── ws/             # Managed WS, typed views, GSN tracking
└── tests/              # Codegen smoke, REST contract, EIP-712 golden, WS chaos
```

## Build / test

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo doc --no-deps
```

`cargo build` does NOT require `buf` or `protoc` — wire types live committed under `src/types/generated/`.

## Codegen

Wire types are regenerated via the codegen binary at `scripts/codegen-rust/`. Point it at a checkout of the OBSDN proto definitions:

```bash
cargo run --release --manifest-path scripts/codegen-rust/Cargo.toml -- \
  --proto-dir   <path-to-api/proto> \
  --out-dir     src/types/generated
```

`buf` must be on PATH for this. Commit the regenerated files; CI fails if `git diff --exit-code src/types/generated/` is dirty.

## Documentation

- Architecture overview (ASCII diagrams): [`docs/architecture.md`](docs/architecture.md).
- API reference: `cargo doc --open`. Internal-only proto fields are tagged `#[doc(hidden)]` so they don't render but stay reachable.
- WebSocket protocol: see the OBSDN public docs site.

## License

Dual-licensed under MIT or Apache 2.0.
