//! Public (no-auth) staging read-path smoke tests.
//!
//! Exercises every public read endpoint against live staging
//! (`nova.staging.obsdn.trade`). These tests never mutate state and never
//! authenticate - the authenticated read/write order lifecycle lives in
//! `e2e_staging.rs` (which registers a fresh key per run), and additional WS
//! coverage in `staging_ws.rs`.
//!
//! Run: `OBSDN_STAGING=1 cargo test --test staging_smoke -- --nocapture`
//!
//! All tests are skipped unless `OBSDN_STAGING=1` so the suite compiles (and
//! no-ops) in CI without network access.

use obsdn_sdk::types::v1::{
    GetAssetsRequest, GetChainConfigRequest, GetClientInfoRequest, GetErrorCodesRequest,
    GetFeeTiersRequest, GetFundingRateHistoryRequest, GetMarketCandlesRequest,
    GetMarketTradesRequest, GetPricesRequest,
};
use obsdn_sdk::{sign, Client, Env};

const ONE_MINUTE_NS: i64 = 60_000_000_000;

fn skip_unless_staging() -> bool {
    if std::env::var("OBSDN_STAGING").is_err() {
        eprintln!("skipping: set OBSDN_STAGING=1 to enable");
        return true;
    }
    false
}

fn unauthed() -> Client {
    Client::builder()
        .env(Env::Staging)
        .build()
        .expect("build staging client")
}

// ---------------------------------------------------------------------------
// Public (no-auth) read endpoints
// ---------------------------------------------------------------------------

#[tokio::test]
async fn staging_get_markets() {
    if skip_unless_staging() {
        return;
    }
    let resp = unauthed().markets().list().await.expect("list");
    assert!(!resp.mkts.is_empty(), "staging should have markets");
    // Every market index must fit in the EIP-712 `uint16 marketIndex`.
    for m in &resp.mkts {
        let _idx: u16 = m.idx.parse().expect("market idx must parse as u16");
    }
    assert!(
        resp.mkts.iter().any(|m| m.mkt_id == "BTC-PERP"),
        "BTC-PERP should be listed"
    );
    eprintln!("OK: {} markets", resp.mkts.len());
}

#[tokio::test]
async fn staging_get_assets() {
    if skip_unless_staging() {
        return;
    }
    let resp = unauthed()
        .asset()
        .list(GetAssetsRequest {})
        .await
        .expect("list");
    let usdc = resp
        .assets
        .iter()
        .find(|a| a.asset == "USDC")
        .expect("USDC collateral asset present");
    assert_eq!(usdc.dec, 6, "USDC on-chain decimals");
    eprintln!("OK: {} assets, USDC dec={}", resp.assets.len(), usdc.dec);
}

/// The single most important read test: the EIP-712 domain the SDK signs
/// with (`sign::default_eip712_domain(Env::Staging)`) MUST match the domain the live
/// backend verifies against (`GET /chain/config`). A mismatch in chain_id
/// or verifying_contract silently rejects every order/withdraw/transfer.
#[tokio::test]
async fn staging_chain_config_matches_sdk_domain() {
    if skip_unless_staging() {
        return;
    }
    let resp = unauthed()
        .chain()
        .config(GetChainConfigRequest {})
        .await
        .expect("config");
    let live = resp
        .domain
        .expect("chain config should carry an EIP-712 domain");

    let sdk = sign::default_eip712_domain(&Env::Staging);
    assert_eq!(
        live.nm,
        sdk.name.as_deref().unwrap_or_default(),
        "domain name drift"
    );
    assert_eq!(
        live.ver,
        sdk.version.as_deref().unwrap_or_default(),
        "domain version drift"
    );
    assert_eq!(
        live.chain_id,
        sdk.chain_id.expect("sdk chain_id").to_string(),
        "domain chain_id drift"
    );
    assert_eq!(
        live.verif_contract.to_lowercase(),
        format!("{}", sdk.verifying_contract.expect("sdk contract")).to_lowercase(),
        "verifying contract drift - signatures would be rejected"
    );
    eprintln!(
        "OK: SDK domain matches live staging ({} / {} / {} / {})",
        live.nm, live.ver, live.chain_id, live.verif_contract
    );
}

#[tokio::test]
async fn staging_get_prices() {
    if skip_unless_staging() {
        return;
    }
    let resp = unauthed()
        .price()
        .list(GetPricesRequest {
            assets: vec!["BTC".into(), "ETH".into()],
        })
        .await
        .expect("list");
    assert!(!resp.prices.is_empty(), "should return at least one price");
    for p in &resp.prices {
        assert!(!p.mark_px.is_empty(), "{} mark_px present", p.asset);
    }
    eprintln!("OK: {} prices", resp.prices.len());
}

#[tokio::test]
async fn staging_get_fee_tiers() {
    if skip_unless_staging() {
        return;
    }
    let resp = unauthed()
        .general()
        .fee_tiers(GetFeeTiersRequest {})
        .await
        .expect("fee_tiers");
    assert!(!resp.tiers.is_empty(), "staging should have fee tiers");
    eprintln!("OK: {} fee tiers", resp.tiers.len());
}

#[tokio::test]
async fn staging_get_error_codes() {
    if skip_unless_staging() {
        return;
    }
    let resp = unauthed()
        .general()
        .error_codes(GetErrorCodesRequest::default())
        .await
        .expect("error_codes");
    assert!(!resp.errs.is_empty(), "should enumerate error codes");
    eprintln!("OK: {} error codes", resp.errs.len());
}

#[tokio::test]
async fn staging_get_client_info_unauthed() {
    if skip_unless_staging() {
        return;
    }
    let resp = unauthed()
        .general()
        .client_info(GetClientInfoRequest {})
        .await
        .expect("client_info");
    assert!(
        !resp.is_auth,
        "unauthenticated client must report is_auth=false"
    );
    eprintln!("OK: client_info is_auth={}", resp.is_auth);
}

#[tokio::test]
async fn staging_get_order_book() {
    if skip_unless_staging() {
        return;
    }
    let resp = unauthed()
        .markets()
        .order_book("BTC-PERP")
        .await
        .expect("order_book");
    let book = resp.book.expect("orderbook payload present");
    eprintln!(
        "OK: BTC-PERP book bids={} asks={} gsn={}",
        book.bids.len(),
        book.asks.len(),
        resp.gsn
    );
}

#[tokio::test]
async fn staging_get_market_trades() {
    if skip_unless_staging() {
        return;
    }
    let resp = unauthed()
        .markets()
        .trades(
            "BTC-PERP",
            GetMarketTradesRequest {
                lmt: 10,
                ..Default::default()
            },
        )
        .await
        .expect("trades");
    eprintln!("OK: {} recent trades", resp.trades.len());
}

#[tokio::test]
async fn staging_get_market_candles() {
    if skip_unless_staging() {
        return;
    }
    let resp = unauthed()
        .markets()
        .candles(
            "BTC-PERP",
            GetMarketCandlesRequest {
                intv: ONE_MINUTE_NS,
                ..Default::default()
            },
        )
        .await
        .expect("candles");
    eprintln!("OK: {} candles", resp.data.len());
}

#[tokio::test]
async fn staging_get_funding_rate_history() {
    if skip_unless_staging() {
        return;
    }
    let resp = unauthed()
        .markets()
        .funding_rate_history(
            "BTC-PERP",
            GetFundingRateHistoryRequest {
                lmt: 10,
                ..Default::default()
            },
        )
        .await
        .expect("funding_rate_history");
    eprintln!("OK: {} funding-rate items", resp.items.len());
}
