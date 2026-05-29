# Integration Testing

## Test Suites

### Unit / Offline Tests (no network)

```bash
cargo test --all-targets
```

Runs all 86 tests including:

- **Golden EIP-712 tests** (`tests/eip712_golden.rs`) — Rust signing output matches the exchange's reference signer byte-for-byte across 10 template families (Order, Transfer, Withdraw, Register, DelegatedSigner, CreateVault, StakeVault, UnstakeVault, CreateSubaccount, RegisterChildAccountSigner).
- **WS chaos tests** (`tests/ws_chaos.rs`) - reconnect, gap detection, frame loss.
- **REST smoke** (`tests/rest_smoke.rs`, `tests/rest_phase3_smoke.rs`) - wiremock-based.
- **View roundtrip** - BookView, TickerView, OracleView, OrderView deserialization.
- **Codegen** (`tests/codegen_smoke.rs`) - generated types compile and have expected fields.

### Production Smoke (unauthenticated)

```bash
OBSDN_SMOKE=1 cargo test --test integration_smoke -- --nocapture
```

Hits live production `GET /markets` and (with creds) `GET /accounts`. Gated by `OBSDN_SMOKE=1`.

### Staging Smoke (unauthenticated)

```bash
OBSDN_STAGING=1 cargo test --test staging_smoke -- --nocapture
```

Hits live staging public endpoints: markets, fee-tiers, client-info, portfolio (with hardcoded test key). Gated by `OBSDN_STAGING=1`.

### E2E Staging (REST + live WS observer)

```bash
OBSDN_STAGING=1 cargo test --test e2e_staging -- --nocapture --test-threads=1
```

Full end-to-end against the live staging matching engine **and** pulse WS. Gated by `OBSDN_STAGING=1`; self-skips when unset. Two tests:

**`e2e_combined_flow`** - one account lifecycle, with the WS as live observer of the REST mutations:

1. Sender keypair (pk=0x01) + signer keypair (pk=0x02)
2. **Register signer** - sender signs 4-field `Register` struct (C2 proof), signer signs `DelegatedSigner`, POST `/auth/signers` returns API key
3. **Faucet** - request 10k USDC on staging
4. **WS authenticate** + subscribe `Order { market: None }` (wildcard); drain initial snapshot
5. **Place order** via REST - sign `Order` with `uint16 marketIndex` (C1 proof) → **assert the order update arrives on the wildcard sub** stamped with the concrete market (`filter="BTC-PERP"`) - live proof of the WS wildcard-routing fix
6. **Cancel order** via REST → assert the follow-up update arrives on the wildcard sub
7. **Set leverage** - POST `/positions/BTC-PERP/leverage` (H1 proof)
8. Cleanup - cancel all, WS shutdown

**`e2e_ws_public_book`** - public book channel, no auth: asserts the first frame is a `Snapshot` with a populated book, then a follow-up `Update` arrives; validates live `as_book()` deserialization.

Per-channel GSN is logged, never asserted contiguous - pulse `gsn` is a global event watermark, sparse per subscription by design (observed deltas of tens-to-hundreds between consecutive frames on one channel).

**What it proves:**

| Finding | Verification |
|---------|-------------|
| C1: Order.marketIndex uint16 | Order placed + accepted by matching engine |
| C2: Register 4-field struct | Signer registered, API key returned |
| H1: Portfolio REST wrappers | SetLeverage endpoint responds |
| WS wildcard routing | `Order { market: None }` receives concrete-market updates (place + cancel observed over WS) |
| WS snapshot ordering | Public book delivers `Snapshot` before `Update` |

## Environment Config

| Env | REST URL | Chain ID | Verifying Contract |
|-----|----------|----------|--------------------|
| Staging | `nova.staging.obsdn.trade` | 10143 (Monad testnet) | `0xB95aE40b700FDBb0906b8Dc2AeBBDd94848325Df` |
| Production | `api.obsdn.trade` | 143 (Monad mainnet) | `0x90c3747cd4E6bC6FbebB1b3C54D99737590eBE45` |

Live domain values can be fetched from `GET /chain/config`.

## Regenerating Golden Fixtures

When EIP-712 struct definitions change (field added/removed, type changed):

1. Fix the Rust `sol!` struct in `src/sign/*.rs`
2. Update `tests/eip712_golden.rs` to match new fields
3. Run golden tests - the "left" (got) values are the correct ones
4. Update `tests/fixtures/eip712/*.json` with the correct struct_hash, digest, signature
5. Verify: `cargo test --test eip712_golden`

The fixture `domain_separator` only changes if the EIP-712 domain changes.

## Regenerating Wire Types

```bash
cargo run --manifest-path scripts/codegen-rust/Cargo.toml -- \
  --proto-dir /path/to/schema \
  --out-dir src/types/generated
```

Output must be committed — CI enforces `git diff --exit-code src/types/generated/`.
