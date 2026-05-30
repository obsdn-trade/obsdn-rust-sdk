//! Orders REST surface (`/orders`).

use std::sync::Arc;

use crate::builder::Client;
use crate::error::{Error, Result};
use crate::market_cache::MarketCache;
use crate::rest::query::percent_encode_segment;
use crate::rest::{AuthMode, RestClient};
use crate::sign::{order::OrderPayload, scale_f64, sign_order, signature_hex};
use crate::types::v1::{
    CancelAllOrdersRequest, CancelAllOrdersResponse, CancelOrderByClientIdRequest,
    CancelOrderByClientIdResponse, CancelOrderRequest, CancelOrderResponse, CancelOrdersRequest,
    CancelOrdersResponse, GetOrderByClientIdRequest, GetOrderByClientIdResponse, GetOrderRequest,
    GetOrderResponse, ListOpenOrdersRequest, ListOpenOrdersResponse, ListOrderHistoryRequest,
    ListOrderHistoryResponse, OrderSide, OrderType, PlaceOrderGroupRequest,
    PlaceOrderGroupResponse, PlaceOrderRequest, PlaceOrderResponse, PlaceTwapOrdersRequest,
    PlaceTwapOrdersResponse, SelfTradePrevention, TimeInForce,
};

/// Cheap handle to the order endpoints. Holds an `Arc` to the shared
/// [`RestClient`] - clone freely.
///
/// Constructed via [`crate::Client::orders`]. `client` is the back-reference
/// used by [`Self::place_limit`] for resolve→sign→post in one call.
#[derive(Debug, Clone)]
pub struct Orders {
    rest: Arc<RestClient>,
    client: Client,
}

impl Orders {
    pub(crate) fn with_client(rest: Arc<RestClient>, client: Client) -> Self {
        Self { rest, client }
    }

    /// `POST /orders` - place a single order.
    ///
    /// **Auth:** required. EIP-712 `sig` must be pre-populated; use
    /// [`Self::place_limit`] for the sign-then-place helper.
    pub async fn place(&self, req: PlaceOrderRequest) -> Result<PlaceOrderResponse> {
        self.rest.post("/orders", &req, AuthMode::Required).await
    }

    /// `POST /orders/group` - place a group of related orders (BRACKET).
    /// **Auth:** required.
    pub async fn place_group(
        &self,
        req: PlaceOrderGroupRequest,
    ) -> Result<PlaceOrderGroupResponse> {
        self.rest
            .post("/orders/group", &req, AuthMode::Required)
            .await
    }

    /// `POST /orders/twap` - place TWAP sub-orders.
    /// **Auth:** required.
    pub async fn place_twap(&self, req: PlaceTwapOrdersRequest) -> Result<PlaceTwapOrdersResponse> {
        self.rest
            .post("/orders/twap", &req, AuthMode::Required)
            .await
    }

    /// `DELETE /orders/{oid}` - cancel by order ID.
    /// **Auth:** required.
    pub async fn cancel(&self, oid: &str) -> Result<CancelOrderResponse> {
        let path = format!("/orders/{}", percent_encode_segment(oid));
        // CancelOrderRequest has only the `oid` field, already in the path.
        let _ = CancelOrderRequest::default();
        self.rest.delete(&path, AuthMode::Required).await
    }

    /// `DELETE /orders/by-client-id/{cl_oid}` - cancel by client-assigned ID.
    /// **Auth:** required.
    pub async fn cancel_by_client_id(&self, cl_oid: &str) -> Result<CancelOrderByClientIdResponse> {
        let path = format!("/orders/by-client-id/{}", percent_encode_segment(cl_oid));
        let _ = CancelOrderByClientIdRequest::default();
        self.rest.delete(&path, AuthMode::Required).await
    }

    /// `DELETE /orders` - cancel multiple orders by criteria.
    /// **Auth:** required.
    pub async fn cancel_many(&self, req: CancelOrdersRequest) -> Result<CancelOrdersResponse> {
        self.rest
            .delete_with_body("/orders", &req, AuthMode::Required)
            .await
    }

    /// `DELETE /orders/all` - cancel all open orders, optionally filtered.
    /// **Auth:** required.
    pub async fn cancel_all(&self, req: CancelAllOrdersRequest) -> Result<CancelAllOrdersResponse> {
        // Filters are query params, not a body.
        self.rest
            .delete_with_query("/orders/all", &req, AuthMode::Required)
            .await
    }

    /// `GET /orders/{oid}` - fetch a single order.
    /// **Auth:** required (read-only allowed).
    pub async fn get(&self, oid: &str) -> Result<GetOrderResponse> {
        let path = format!("/orders/{}", percent_encode_segment(oid));
        let _ = GetOrderRequest::default();
        self.rest.get(&path, AuthMode::Required).await
    }

    /// `GET /orders/by-client-id/{cl_oid}` - fetch by client-assigned ID.
    /// **Auth:** required (read-only allowed).
    pub async fn get_by_client_id(&self, cl_oid: &str) -> Result<GetOrderByClientIdResponse> {
        let path = format!("/orders/by-client-id/{}", percent_encode_segment(cl_oid));
        let _ = GetOrderByClientIdRequest::default();
        self.rest.get(&path, AuthMode::Required).await
    }

    /// `GET /orders` - list open orders.
    /// **Auth:** required (read-only allowed).
    pub async fn list_open(&self, req: ListOpenOrdersRequest) -> Result<ListOpenOrdersResponse> {
        self.rest
            .get_with_query("/orders", &req, AuthMode::Required)
            .await
    }

    /// `GET /orders/history` - list historical orders with pagination.
    /// **Auth:** required (read-only allowed).
    pub async fn list_history(
        &self,
        req: ListOrderHistoryRequest,
    ) -> Result<ListOrderHistoryResponse> {
        self.rest
            .get_with_query("/orders/history", &req, AuthMode::Required)
            .await
    }

    /// One-call resolve-sign-place for the common LIMIT path.
    ///
    /// Resolves `mkt_id` via the client's market cache, scales `size`/`px`
    /// to 18-decimal fixed-point, signs the EIP-712 `Order` payload with
    /// the configured signer, and POSTs `/orders`.
    ///
    /// `nonce` defaults to wall-clock nanoseconds when zero - pass an
    /// explicit value for deterministic test fixtures or when retrying
    /// idempotently.
    ///
    /// **Precision:** `price` and `size` are `f64`, which provides ~15-17
    /// significant decimal digits. This is sufficient for most trading use
    /// cases. For sub-penny precision at high prices (e.g., exact
    /// `0.01` increments above `$100,000`), use
    /// [`crate::sign::scale_decimal_str`] with a raw [`PlaceOrderRequest`]
    /// and [`crate::Client::sign_place_order`] instead.
    ///
    /// **Scope:** LIMIT only. The exchange does not implement a true
    /// MARKET order - IOC at top-of-book is the supported substitute, set
    /// `tif = TimeInForce::Ioc` on a LIMIT and pick a price that crosses.
    /// STOP / TWAP / order-group flows require fields (`stop_t`,
    /// `stop_px`, `exp_ts`, `sched_ts`, ...) that this helper
    /// deliberately doesn't expose; build a raw [`PlaceOrderRequest`] +
    /// [`crate::Client::sign_place_order`] for those.
    ///
    /// Errors:
    /// - `Error::Config` - `mkt_id` is unknown.
    /// - `Error::Sign` - no `eip712_signer` configured, scaling failed, or
    ///   `side` is `Unspecified`.
    pub async fn place_limit(&self, req: LimitOrder) -> Result<PlaceOrderResponse> {
        let client = &self.client;
        let signer = client.eip712_signer().cloned().ok_or_else(|| {
            Error::Sign("no eip712_signer configured; call ClientBuilder::eip712_signer".into())
        })?;
        if !req.size.is_finite() || req.size <= 0.0 {
            return Err(Error::Sign(
                "order size must be a positive finite number".into(),
            ));
        }
        if !req.price.is_finite() || req.price <= 0.0 {
            return Err(Error::Sign(
                "order price must be a positive finite number".into(),
            ));
        }
        let market = client.resolve_market(&req.mkt_id).await?;
        let market_index = MarketCache::idx_as_u16(&market)?;
        let size_x18 = scale_f64(req.size)?;
        let price_x18 = scale_f64(req.price)?;
        let nonce = if req.nonce == 0 {
            super::now_unix_nanos()?
        } else {
            req.nonce
        };

        let payload = OrderPayload {
            sender: client.sender_address()?,
            market_index,
            // `OrderSide::Unspecified` is rejected by `try_into()`.
            side: req.side.try_into()?,
            size: size_x18,
            price: price_x18,
            nonce,
        };
        let domain = client.eip_domain_clone();
        let sig = sign_order(signer.as_ref(), &domain, payload)?;

        let placed = PlaceOrderRequest {
            mkt_id: market.mkt_id.clone(),
            sd: req.side as i32,
            ot: OrderType::Limit as i32,
            // Decimal string matches the value scaled into the signed x18
            // payload (`scale_f64` formats `f64` the same way), so server-side
            // signature verification re-derives the identical price/size.
            sz: format!("{}", req.size),
            px: format!("{}", req.price),
            tif: req.tif as i32,
            po: req.post_only,
            ro: req.reduce_only,
            stp: req.stp as i32,
            cl_oid: req.client_order_id.unwrap_or_default(),
            nonce,
            sig: signature_hex(&sig),
            r#await: req.await_match,
            ..Default::default()
        };
        self.place(placed).await
    }
}

/// Inputs for [`Orders::place_limit`]. Mirrors
/// [`PlaceOrderRequest`] minus the fields the helper fills in (`nonce`,
/// `sig`). Optional fields default to "off"/"unspecified" - the same
/// proto defaults a hand-built request would produce.
#[derive(Debug, Clone)]
pub struct LimitOrder {
    /// Market symbol (e.g. `"BTC-PERP"`).
    pub mkt_id: String,
    /// Buy or sell (`Side::Buy` / `Side::Sell`). `Unspecified` returns
    /// `Error::Sign`.
    pub side: OrderSide,
    /// Quote-asset price (limit price).
    pub price: f64,
    /// Base-asset size.
    pub size: f64,
    /// Time in force. Default `OrderTimeInForceUnspecified` → server
    /// applies GTC.
    pub tif: TimeInForce,
    /// Post-only flag.
    pub post_only: bool,
    /// Reduce-only flag.
    pub reduce_only: bool,
    /// Self-trade prevention.
    pub stp: SelfTradePrevention,
    /// Optional client-assigned id (max 32 chars; `None` → server-assigned).
    pub client_order_id: Option<String>,
    /// `0` → use wall-clock nanos. Pass non-zero for deterministic test
    /// fixtures or idempotent retry.
    pub nonce: u64,
    /// If true, server waits for matching-engine confirmation before
    /// returning. Only valid for LIMIT.
    pub await_match: bool,
}

impl LimitOrder {
    /// A LIMIT order with sane defaults (GTC, no post-only/reduce-only, no
    /// STP, server-assigned client id, auto nonce). Refine via the builder
    /// methods.
    pub fn new(mkt_id: impl Into<String>, side: OrderSide, price: f64, size: f64) -> Self {
        Self {
            mkt_id: mkt_id.into(),
            side,
            price,
            size,
            tif: TimeInForce::Unspecified,
            post_only: false,
            reduce_only: false,
            stp: SelfTradePrevention::Unspecified,
            client_order_id: None,
            nonce: 0,
            await_match: false,
        }
    }

    /// Set the post-only flag (reject if the order would take liquidity).
    pub fn post_only(mut self, yes: bool) -> Self {
        self.post_only = yes;
        self
    }

    /// Set the reduce-only flag (never increase position size).
    pub fn reduce_only(mut self, yes: bool) -> Self {
        self.reduce_only = yes;
        self
    }

    /// Set the time-in-force (default: server applies GTC).
    pub fn time_in_force(mut self, tif: TimeInForce) -> Self {
        self.tif = tif;
        self
    }

    /// Set the self-trade-prevention policy.
    pub fn self_trade_prevention(mut self, stp: SelfTradePrevention) -> Self {
        self.stp = stp;
        self
    }

    /// Attach a caller-assigned client order id (max 32 chars).
    pub fn client_order_id(mut self, id: impl Into<String>) -> Self {
        self.client_order_id = Some(id.into());
        self
    }

    /// Pin the EIP-712 nonce (default `0` → wall-clock nanos). Use a fixed
    /// value for deterministic fixtures or idempotent retry.
    pub fn nonce(mut self, nonce: u64) -> Self {
        self.nonce = nonce;
        self
    }

    /// Wait for matching-engine confirmation before the call returns.
    pub fn await_match(mut self, yes: bool) -> Self {
        self.await_match = yes;
        self
    }
}
