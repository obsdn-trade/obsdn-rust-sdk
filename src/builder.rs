//! `Client` + `ClientBuilder` - public entry point.

use std::sync::Arc;
use std::time::Duration;

use alloy_sol_types::Eip712Domain;
use url::Url;

use crate::auth::HmacSigner;
use crate::env::Env;
use crate::error::{Error, Result};
use crate::market_cache::MarketCache;
use crate::rest::{
    account::AccountApi, asset::AssetApi, auth_api::AuthApi, chain::ChainApi, general::GeneralApi,
    markets::MarketsApi, orders::OrdersApi, portfolio::PortfolioApi, price::PriceApi,
    subaccount::SubaccountApi, vault::VaultApi, RestClient,
};
use alloy_primitives::Address;

use crate::sign::{order::OrderPayload, sdk_domain, sign_order, signature_hex, EipSigner};
use crate::types::v1::Market;
use crate::ws::WsClient;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Top-level handle. Holds a shared `RestClient`; per-service handles are
/// constructed on demand (`client.orders()`, `client.markets()`).
#[derive(Clone)]
pub struct Client {
    rest: Arc<RestClient>,
    eip_signer: Option<Arc<dyn EipSigner>>,
    domain: Eip712Domain,
    /// Retained for [`Self::ws`] - Phase 5 needs the WS URL + (optional)
    /// HMAC creds to authenticate private channels.
    env: Env,
    hmac: Option<HmacSigner>,
    /// Lazy market metadata cache - populated on first
    /// [`Self::resolve_market`] (or `OrdersApi::place_easy`) call.
    markets_cache: Arc<MarketCache>,
    /// Explicit sender (main wallet) address for delegated signing.
    /// When set, EIP-712 payloads use this as `sender`/`from` while the
    /// `eip_signer` key produces the cryptographic signature.
    /// Falls back to `eip_signer.address()` when `None`.
    sender_address: Option<Address>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("rest", &self.rest)
            .field("eip_signer", &self.eip_signer.as_ref().map(|s| s.address()))
            .field("sender_address", &self.sender_address)
            .field("domain", &"<Eip712Domain>")
            .finish()
    }
}

impl Client {
    /// Begin building a client. Call `.build()` to finalize.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Order endpoints (`OrderService`). The handle carries a back-reference
    /// to this client so ergonomic helpers (`place_easy`) can resolve the
    /// market index + sign in one call.
    pub fn orders(&self) -> OrdersApi {
        OrdersApi::with_client(Arc::clone(&self.rest), self.clone())
    }

    /// Market data endpoints (`MarketService`).
    pub fn markets(&self) -> MarketsApi {
        MarketsApi::new(Arc::clone(&self.rest))
    }

    /// Account / transfer endpoints (`AccountService`).
    pub fn account(&self) -> AccountApi {
        AccountApi::new(Arc::clone(&self.rest))
    }

    /// Asset metadata endpoints (`AssetService`).
    pub fn asset(&self) -> AssetApi {
        AssetApi::new(Arc::clone(&self.rest))
    }

    /// Auth / API-key endpoints (`AuthService`).
    pub fn auth_api(&self) -> AuthApi {
        AuthApi::new(Arc::clone(&self.rest))
    }

    /// Chain config + on-chain event endpoints (`ChainService`).
    pub fn chain(&self) -> ChainApi {
        ChainApi::new(Arc::clone(&self.rest))
    }

    /// Misc client/server metadata endpoints (`GeneralService`).
    pub fn general(&self) -> GeneralApi {
        GeneralApi::new(Arc::clone(&self.rest))
    }

    /// Portfolio / positions / PnL endpoints (`PortfolioService`).
    pub fn portfolio(&self) -> PortfolioApi {
        PortfolioApi::new(Arc::clone(&self.rest))
    }

    /// Oracle prices (`PriceService`).
    pub fn price(&self) -> PriceApi {
        PriceApi::new(Arc::clone(&self.rest))
    }

    /// Subaccount endpoints (`SubaccountService`).
    pub fn subaccount(&self) -> SubaccountApi {
        SubaccountApi::new(Arc::clone(&self.rest))
    }

    /// Vault endpoints (`VaultService`). Mostly internal.
    pub fn vault(&self) -> VaultApi {
        VaultApi::new(Arc::clone(&self.rest))
    }

    /// WebSocket factory (Phase 5 thin client). Construct fresh
    /// connections via `client.ws().connect().await`. Inherits the
    /// HMAC API key configured on the builder so private channels can
    /// authenticate.
    pub fn ws(&self) -> WsClient {
        WsClient::new(self.env.clone(), self.hmac.clone())
    }

    /// EIP-712 signer attached to this client, if any. Used for offline
    /// signing of orders / transfers / withdrawals before sending the REST
    /// request.
    pub fn eip_signer(&self) -> Option<&Arc<dyn EipSigner>> {
        self.eip_signer.as_ref()
    }

    /// Sender (main wallet) address used in EIP-712 payloads.
    ///
    /// In delegated-signing mode (set via [`ClientBuilder::sender`]) this
    /// returns the main wallet address while [`Self::eip_signer`] holds the
    /// delegated key. In normal mode it falls back to the signer's address.
    ///
    /// Panics if no EIP signer is configured - callers that need the sender
    /// address should check for a signer first.
    pub(crate) fn sender_address(&self) -> Address {
        self.sender_address.unwrap_or_else(|| {
            self.eip_signer
                .as_ref()
                .expect("no eip_signer configured")
                .address()
        })
    }

    /// EIP-712 domain for this client's environment. Pass to
    /// [`crate::sign::sign_order`] etc. when invoking the low-level signers
    /// directly.
    pub fn eip712_domain(&self) -> &Eip712Domain {
        &self.domain
    }

    /// Sign an [`OrderPayload`] and write the resulting `0x...` hex
    /// signature into `req.sig`. Requires an attached EIP-712 signer (set
    /// via [`ClientBuilder::eip_signer`]).
    ///
    /// Low-level - callers populate `payload.market_index` and
    /// `payload.sender` themselves. In delegated-signing mode, set
    /// `payload.sender` to the main wallet address (not the signer's).
    /// Prefer [`crate::rest::orders::OrdersApi::place_easy`] for the
    /// resolve-sign-place flow.
    pub fn sign_place_order(
        &self,
        req: &mut crate::types::v1::PlaceOrderRequest,
        payload: OrderPayload,
    ) -> Result<()> {
        let signer = self.eip_signer.as_ref().ok_or_else(|| {
            Error::Sign("no eip_signer configured; call ClientBuilder::eip_signer".into())
        })?;
        let sig = sign_order(signer.as_ref(), &self.domain, payload)?;
        req.sig = signature_hex(&sig);
        Ok(())
    }

    /// Resolve a market by `mkt_id` (e.g. `"BTC-PERP"`) via the lazy cache.
    /// Returns the full [`Market`] proto - caller picks the fields they
    /// need (`idx`, `base_incr`, `mark_px`, ...).
    pub async fn resolve_market(&self, mkt_id: &str) -> Result<Market> {
        self.markets_cache.resolve(mkt_id).await
    }

    /// Drop the cached markets snapshot. The next
    /// [`Self::resolve_market`] call will re-fetch. Use after a server
    /// "market not found" against a known-good symbol.
    pub async fn invalidate_market_cache(&self) {
        self.markets_cache.invalidate().await;
    }

    pub(crate) fn eip_domain_clone(&self) -> Eip712Domain {
        self.domain.clone()
    }
}

/// Builder for [`Client`]. Defaults: env=Production, timeout=10s, no signer.
#[derive(Default)]
pub struct ClientBuilder {
    env: Option<Env>,
    base_url_override: Option<String>,
    signer: Option<HmacSigner>,
    eip_signer: Option<Arc<dyn EipSigner>>,
    sender_address: Option<Address>,
    domain_override: Option<Eip712Domain>,
    timeout: Option<Duration>,
    user_agent: Option<String>,
    danger_accept_invalid_certs: bool,
}

impl std::fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("env", &self.env)
            .field("base_url_override", &self.base_url_override)
            .field("signer", &self.signer)
            .field("eip_signer", &self.eip_signer.as_ref().map(|s| s.address()))
            .field("sender_address", &self.sender_address)
            .field("timeout", &self.timeout)
            .field("user_agent", &self.user_agent)
            .finish()
    }
}

impl ClientBuilder {
    /// Target environment (REST + WS endpoints).
    pub fn env(mut self, env: Env) -> Self {
        self.env = Some(env);
        self
    }

    /// Override REST base URL - useful for tests (wiremock, local stubs).
    /// Takes precedence over [`Self::env`] for the REST client; WS routing
    /// (Phase 5+) still respects `env`.
    pub fn rest_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url_override = Some(url.into());
        self
    }

    /// HMAC API key + secret pair. Required for authenticated endpoints.
    pub fn api_key(mut self, api_key: impl Into<String>, api_secret: impl Into<String>) -> Self {
        self.signer = Some(HmacSigner::new(api_key, api_secret));
        self
    }

    /// Attach an EIP-712 signer for offline signing of order / transfer /
    /// withdrawal payloads. Use [`crate::LocalSigner`] for a local secp256k1
    /// key, or implement [`EipSigner`] for hardware wallets.
    pub fn eip_signer(mut self, signer: Arc<dyn EipSigner>) -> Self {
        self.eip_signer = Some(signer);
        self
    }

    /// Set the sender (main wallet) address for delegated signing.
    ///
    /// In delegated-signing mode the EIP-712 payloads carry the *main wallet*
    /// address as `sender`/`from`, while the [`Self::eip_signer`] key
    /// produces the cryptographic signature. This mirrors the Go SDK's
    /// `senderAddress` / `signerPrivateKey` separation.
    ///
    /// When omitted the signer's own address is used - correct for
    /// non-delegated (direct) signing.
    pub fn sender(mut self, addr: Address) -> Self {
        self.sender_address = Some(addr);
        self
    }

    /// Override the EIP-712 domain. By default the domain follows
    /// [`Self::env`]; this is the escape hatch for non-public chains.
    pub fn eip712_domain(mut self, domain: Eip712Domain) -> Self {
        self.domain_override = Some(domain);
        self
    }

    /// Per-request timeout. Default: 10s.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Override the default `User-Agent` header (`obsdn-sdk-rust/<ver>`).
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }

    /// Skip TLS certificate verification. **Staging/testing only** - never
    /// enable in production.
    pub fn danger_accept_invalid_certs(mut self, accept: bool) -> Self {
        self.danger_accept_invalid_certs = accept;
        self
    }

    /// Finalize. Returns `Error::Config` if the resolved base URL is
    /// invalid.
    pub fn build(self) -> Result<Client> {
        let env = self.env.unwrap_or(Env::Production);
        let base = match self.base_url_override {
            Some(s) => s,
            None => env.rest_base_url().to_string(),
        };
        let base_url = Url::parse(&base)
            .map_err(|e| Error::Config(format!("invalid base url {base}: {e}")))?;
        let timeout = self.timeout.unwrap_or(DEFAULT_TIMEOUT);
        let hmac = self.signer.clone();
        let rest = RestClient::new(
            base_url,
            self.signer,
            timeout,
            self.user_agent,
            self.danger_accept_invalid_certs,
        )?;
        let rest = Arc::new(rest);
        let domain = match (&env, self.domain_override) {
            (_, Some(d)) => d,
            (Env::Custom { .. }, None) => {
                return Err(Error::Config(
                    "Env::Custom requires an explicit .eip712_domain() - \
                     the SDK cannot guess the correct chain_id / verifying_contract"
                        .into(),
                ));
            }
            (env, None) => sdk_domain(env),
        };
        let markets_api = MarketsApi::new(Arc::clone(&rest));
        let markets_cache = Arc::new(MarketCache::new(markets_api));
        Ok(Client {
            rest,
            eip_signer: self.eip_signer,
            domain,
            env,
            hmac,
            markets_cache,
            sender_address: self.sender_address,
        })
    }
}
