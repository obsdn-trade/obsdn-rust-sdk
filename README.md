# obsdn-sdk

[![CI](https://github.com/obsdn-trade/obsdn-rust-sdk/actions/workflows/ci.yml/badge.svg)](https://github.com/obsdn-trade/obsdn-rust-sdk/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE-MIT)
[![MSRV](https://img.shields.io/badge/MSRV-1.95-blue.svg)](Cargo.toml)

Async Rust client for the [OBSDN](https://obsdn.trade) perpetual exchange - REST, EIP-712 order signing, and a managed WebSocket feed in one crate.

## Contents

- [Features](#features)
- [Status](#status)
- [Installation](#installation)
- [Configuration](#configuration)
- [Getting started](#getting-started)
- [Examples](#examples)
- [Project layout](#project-layout)
- [Building and testing](#building-and-testing)
- [Code generation](#code-generation)
- [Documentation](#documentation)
- [Safety](#safety)
- [Supported Rust versions](#supported-rust-versions)
- [Getting help](#getting-help)
- [Contributing](#contributing)
- [Disclaimer](#disclaimer)
- [License](#license)

## Features

- **REST** - the public service surface (~50 RPCs) across 11 typed handles: `orders`, `markets`, `account`, `asset`, `auth`, `chain`, `general`, `portfolio`, `price`, `subaccount`, and `vault`. Covers leverage, margin mode, margin transfer, and fee-tier endpoints. Authenticated requests are signed with HMAC.
- **EIP-712 signing** - a local secp256k1 signer (`LocalSigner`) whose output is byte-equal to the exchange's reference signer, verified against golden fixtures. Templates: Order, Transfer, Withdraw, Vault (Create / Stake / Unstake), Subaccount, Register, and DelegatedSigner.
- **WebSocket** - a managed client with automatic reconnect, exponential backoff, and HMAC auth replay. Typed views per channel: `book` (with checksum), `ticker`, `oracle`, `trade`, and `order`. The raw `gsn` (global sequence number) watermark is exposed on each frame; the client does not infer gaps (`gsn` is sparse per subscription), so resync via REST after a reconnect if you need byte-perfect catch-up.

## Status

This crate is `0.1.0` and pre-1.0: the public API may change between releases. It is distributed via git rather than crates.io ([installation](#installation)). The `master` branch tracks the latest changes; pin a commit if you need a stable reference.

## Installation

Add it as a git dependency:

```toml
[dependencies]
obsdn-sdk = { git = "https://github.com/obsdn-trade/obsdn-rust-sdk", branch = "master" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Or vendor it as a git submodule:

```bash
git submodule add git@github.com:obsdn-trade/obsdn-rust-sdk.git vendor/obsdn-sdk
```

```toml
obsdn-sdk = { path = "vendor/obsdn-sdk" }
```

## Configuration

The client targets `Production` (`https://api.obsdn.trade`) by default. To point at a non-public host (an internal host, a forked stack, or a local backend), pass `Env::Custom` with your own REST/WS URLs and a matching EIP-712 domain via `ClientBuilder::eip712_domain`.

Credentials are read from the environment in the examples:

| Variable            | Purpose                                                                 |
|---------------------|-------------------------------------------------------------------------|
| `OBSDN_API_KEY`     | HMAC API key - required for authenticated endpoints and private channels. |
| `OBSDN_API_SECRET`  | HMAC API secret. Pair with `OBSDN_API_KEY`.                             |
| `OBSDN_PRIVATE_KEY` | secp256k1 private key (hex, `0x`-prefixed or bare) for EIP-712 signing. |
| `RUST_LOG`          | `tracing-subscriber` filter, e.g. `obsdn_sdk=debug,info`.               |

## Getting started

Read public market data:

```rust
use obsdn_sdk::Client;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Defaults to production; authenticated with HMAC credentials.
    let client = Client::builder()
        .api_key(std::env::var("OBSDN_API_KEY")?, std::env::var("OBSDN_API_SECRET")?)
        .build()?;

    // Call a REST handle: `markets()` exposes the markets endpoints.
    let markets = client.markets().list().await?;
    println!("{} markets available", markets.markets().len());
    Ok(())
}
```

Place an order - `place_limit` resolves the market index, signs the EIP-712 payload, and posts it:

```rust
use std::sync::Arc;
use obsdn_sdk::rest::orders::LimitOrder;
use obsdn_sdk::types::v1::OrderSide;
use obsdn_sdk::{Client, Env, LocalSigner};

// The signer holds the secp256k1 key used for EIP-712 signing.
let signer = Arc::new(LocalSigner::from_hex(&std::env::var("OBSDN_PRIVATE_KEY")?)?);

// The env is set explicitly: when `.env()` is omitted the builder defaults to
// `Env::Production`, and the call below signs and submits a REAL order. Use
// `Env::Staging` to try it against staging first.
let client = Client::builder()
    .env(Env::Production)
    .api_key(std::env::var("OBSDN_API_KEY")?, std::env::var("OBSDN_API_SECRET")?)
    .eip712_signer(signer) // attach the signer so orders can be signed
    .build()?;

// A limit buy: 0.001 BTC-PERP at 50,000. `place_limit` handles index lookup,
// signing, and the POST.
client
    .orders()
    .place_limit(LimitOrder::new("BTC-PERP", OrderSide::Buy, "50000", "0.001"))
    .await?;
```

Stream the order book over the managed WebSocket:

```rust
use futures_util::StreamExt;
use obsdn_sdk::ws::{Channel, Event};
use obsdn_sdk::Client;

let client = Client::builder().build()?;

// `subscribe` returns a stream that survives reconnects and replays the
// subscription automatically.
let mut stream = client
    .ws()
    .subscribe(Channel::Book { market: "BTC-PERP".into() })
    .await?;

while let Some(evt) = stream.next().await {
    // `as_book` gives a typed view over the raw update frame.
    if let Event::Update(u) = evt {
        let book = u.as_book()?;
        println!("{} bids / {} asks", book.bids.len(), book.asks.len());
    }
}
```

## Examples

The `examples/` directory holds runnable end-to-end flows. Run one with `cargo run --example NAME`.

| Example             | What it shows                                                            |
|---------------------|--------------------------------------------------------------------------|
| `place_order`       | REST + EIP-712 signing via `place_limit`. Quotes 5% under mark.           |
| `cancel_order`      | Cancel by order id (HMAC only, no EIP-712 needed).                       |
| `ws_book`           | Public managed WebSocket, typed `BookView`, prints 10 frames.            |
| `ws_private_orders` | HMAC auth on WebSocket + `Channel::Order`, streams order lifecycle events. |
| `transfer`          | Sign EIP-712 `Transfer`, post `/transfers/send-funds`.                   |
| `withdraw`          | Sign EIP-712 `Withdraw`, post `/transfers/withdraw`.                     |
| `book_with_resync`  | Maintain a local book; on reconnect, refetch via REST snapshot.          |

## Project layout

```
.
├── Cargo.toml          # Crate manifest
├── Cargo.lock          # Committed for reproducible builds
├── examples/           # Runnable cargo examples (see above)
├── scripts/
│   ├── codegen-rust/   # Out-of-band proto codegen (see "Code generation")
│   └── ...             # Go-side EIP-712 fixture exporter
├── src/
│   ├── lib.rs
│   ├── builder.rs      # Client + ClientBuilder
│   ├── error.rs
│   ├── auth.rs         # HMAC signer
│   ├── market_cache.rs # Lazy market-index cache (TTL 60s)
│   ├── rest/           # REST handles + helpers + auth layer
│   ├── sign/           # EIP-712 templates + LocalSigner
│   ├── types/          # Generated wire types (committed under generated/)
│   └── ws/             # Managed WS, typed channel views
└── tests/              # Golden, chaos, REST smoke, staging E2E
```

## Building and testing

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

The Makefile mirrors CI. Run `make help` to list targets: `make check` runs the full gate (style, lint, test, doc); `make fmt` formats; `make deny` runs the supply-chain gate; `make e2e` runs the live staging suite.

`cargo build` requires no external code-generation tooling. Wire types are committed under `src/types/generated/`.

The offline `cargo test` suite covers unit tests, EIP-712 golden fixtures, the WebSocket chaos suite (in-process mock for reconnect, sub-replay, wildcard routing, and sparse GSN), and wiremock REST smoke. The live integration tests below are gated on environment variables and skip when those are unset, so the offline suite stays green in credential-less CI.

### Live integration tests

These hit real servers and self-skip without their environment variable. Run the stateful E2E flow with `--test-threads=1`.

```bash
# Production smoke (unauthenticated public endpoints)
OBSDN_SMOKE=1 cargo test --test integration_smoke -- --nocapture

# Staging smoke (public + authed)
OBSDN_STAGING=1 cargo test --test staging_smoke -- --nocapture

# E2E staging - REST + live WS observer:
#   e2e_combined_flow: register → faucet → ws auth → place/cancel via REST,
#                      observing order updates over the WS wildcard sub
#   e2e_ws_public_book: public book snapshot + follow-up update (no auth)
OBSDN_STAGING=1 cargo test --test e2e_staging -- --nocapture --test-threads=1
```

See [`docs/integration-testing.md`](docs/integration-testing.md) for environment configuration and fixtures.

## Code generation

Wire types in `src/types/generated/` are committed, so building the SDK needs no extra toolchain. To regenerate them, run the codegen tool in `scripts/codegen-rust/` against the schema definitions:

```bash
cargo run --release --manifest-path scripts/codegen-rust/Cargo.toml -- \
  --proto-dir <path-to-proto-dir> \
  --out-dir   src/types/generated
```

Commit the regenerated files; CI fails if `git diff --exit-code src/types/generated/` is dirty.

## Documentation

- API reference: `cargo doc --open`. Internal-only fields are tagged `#[doc(hidden)]`, so they stay reachable without rendering.
- Architecture overview (with diagrams): [`docs/architecture.md`](docs/architecture.md).
- Integration testing guide: [`docs/integration-testing.md`](docs/integration-testing.md).
- WebSocket protocol: see the [OBSDN documentation site](https://docs.obsdn.trade/).

## Safety

This crate sets `#![forbid(unsafe_code)]` - the entire client is implemented in safe Rust. TLS is provided by `rustls`, so there is no OpenSSL dependency.

## Supported Rust versions

The minimum supported Rust version is **1.95**, pinned in `Cargo.toml` (`rust-version`) and checked in CI. Raising the MSRV is a breaking change and is called out in the changelog.

## Getting help

Open an issue on [GitHub](https://github.com/obsdn-trade/obsdn-rust-sdk/issues) for bugs or questions about the SDK. For exchange API semantics, see the [OBSDN documentation site](https://docs.obsdn.trade/).

## Contributing

Issues and pull requests are welcome. Before opening a PR, run `make check` (formatting, lint, tests, and docs) and keep generated wire types in sync if you touch the proto definitions. Commits follow the conventional-commit style used in the history.

## Disclaimer

Trading perpetual futures carries financial risk. This software is provided "as is", without warranty of any kind. You are responsible for any orders, transfers, and withdrawals you sign and submit with it. Review the source and test against staging before using it with real funds.

## License

Licensed under the [MIT License](LICENSE-MIT).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this crate by you shall be licensed as MIT, without any additional terms or conditions.
