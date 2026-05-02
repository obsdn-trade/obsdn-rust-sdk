//! Orders REST surface — `OrderService` in `api/proto/nil/v1/order.proto`.

use std::sync::Arc;

use crate::builder::Client;
use crate::error::{Error, Result};
use crate::market_cache::MarketCache;
use crate::rest::query::percent_encode_segment;
use crate::rest::{Auth, RestClient};
use crate::sign::{
    order::{OrderPayload, OrderSide as SignOrderSide},
    scale_f64, sign_order, signature_hex,
};
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
/// [`RestClient`] — clone freely.
///
/// Constructed via [`crate::Client::orders`]. `client` is the back-reference
/// used by [`Self::place_easy`] for resolve→sign→post in one call.
#[derive(Debug, Clone)]
pub struct OrdersApi {
    rest: Arc<RestClient>,
    client: Client,
}

impl OrdersApi {
    pub(crate) fn with_client(rest: Arc<RestClient>, client: Client) -> Self {
        Self { rest, client }
    }

    /// `POST /orders` — place a single order.
    ///
    /// **Auth:** required. EIP-712 `sig` must be populated by the caller
    /// (Phase 4 will provide a typed signer).
    pub async fn place(&self, req: PlaceOrderRequest) -> Result<PlaceOrderResponse> {
        self.rest.post("/orders", &req, Auth::Required).await
    }

    /// `POST /orders/group` — place a group of related orders (BRACKET).
    /// **Auth:** required.
    pub async fn place_group(
        &self,
        req: PlaceOrderGroupRequest,
    ) -> Result<PlaceOrderGroupResponse> {
        self.rest.post("/orders/group", &req, Auth::Required).await
    }

    /// `POST /orders/twap` — place TWAP sub-orders.
    /// **Auth:** required.
    pub async fn place_twap(&self, req: PlaceTwapOrdersRequest) -> Result<PlaceTwapOrdersResponse> {
        self.rest.post("/orders/twap", &req, Auth::Required).await
    }

    /// `DELETE /orders/{oid}` — cancel by order ID.
    /// **Auth:** required.
    pub async fn cancel(&self, oid: &str) -> Result<CancelOrderResponse> {
        let path = format!("/orders/{}", percent_encode_segment(oid));
        // CancelOrderRequest has only the `oid` field, already in the path.
        let _ = CancelOrderRequest::default();
        self.rest.delete(&path, Auth::Required).await
    }

    /// `DELETE /orders/by-client-id/{cl_oid}` — cancel by client-assigned ID.
    /// **Auth:** required.
    pub async fn cancel_by_client_id(&self, cl_oid: &str) -> Result<CancelOrderByClientIdResponse> {
        let path = format!("/orders/by-client-id/{}", percent_encode_segment(cl_oid));
        let _ = CancelOrderByClientIdRequest::default();
        self.rest.delete(&path, Auth::Required).await
    }

    /// `DELETE /orders` — cancel multiple orders by criteria.
    /// **Auth:** required.
    pub async fn cancel_many(&self, req: CancelOrdersRequest) -> Result<CancelOrdersResponse> {
        self.rest
            .delete_with_body("/orders", &req, Auth::Required)
            .await
    }

    /// `DELETE /orders/all` — cancel all open orders, optionally filtered.
    /// **Auth:** required.
    pub async fn cancel_all(&self, req: CancelAllOrdersRequest) -> Result<CancelAllOrdersResponse> {
        // No `body: "*"` in the http annotation — server reads filters
        // from query params.
        self.rest
            .delete_with_query("/orders/all", &req, Auth::Required)
            .await
    }

    /// `GET /orders/{oid}` — fetch a single order.
    /// **Auth:** required (read-only allowed).
    pub async fn get(&self, oid: &str) -> Result<GetOrderResponse> {
        let path = format!("/orders/{}", percent_encode_segment(oid));
        let _ = GetOrderRequest::default();
        self.rest.get(&path, Auth::Required).await
    }

    /// `GET /orders/by-client-id/{cl_oid}` — fetch by client-assigned ID.
    /// **Auth:** required (read-only allowed).
    pub async fn get_by_client_id(&self, cl_oid: &str) -> Result<GetOrderByClientIdResponse> {
        let path = format!("/orders/by-client-id/{}", percent_encode_segment(cl_oid));
        let _ = GetOrderByClientIdRequest::default();
        self.rest.get(&path, Auth::Required).await
    }

    /// `GET /orders` — list open orders.
    /// **Auth:** required (read-only allowed).
    pub async fn list_open(&self, req: ListOpenOrdersRequest) -> Result<ListOpenOrdersResponse> {
        self.rest
            .get_with_query("/orders", &req, Auth::Required)
            .await
    }

    /// `GET /orders/history` — list historical orders with pagination.
    /// **Auth:** required (read-only allowed).
    pub async fn list_history(
        &self,
        req: ListOrderHistoryRequest,
    ) -> Result<ListOrderHistoryResponse> {
        self.rest
            .get_with_query("/orders/history", &req, Auth::Required)
            .await
    }

    /// One-call resolve-sign-place for the common LIMIT path.
    ///
    /// Resolves `mkt_id` via the client's market cache, scales `size`/`px`
    /// to 18-decimal fixed-point, signs the EIP-712 `Order` payload with
    /// the configured signer, and POSTs `/orders`.
    ///
    /// `nonce` defaults to wall-clock nanoseconds when zero — pass an
    /// explicit value for deterministic test fixtures or when retrying
    /// idempotently.
    ///
    /// **Scope:** LIMIT only. The exchange does not implement a true
    /// MARKET order — IOC at top-of-book is the supported substitute, set
    /// `tif = TimeInForce::Ioc` on a LIMIT and pick a price that crosses.
    /// STOP / TWAP / order-group flows require fields (`stop_t`,
    /// `stop_px`, `expire_ts`, `sched_ts`, ...) that this helper
    /// deliberately doesn't expose; build a raw [`PlaceOrderRequest`] +
    /// [`crate::Client::sign_place_order`] for those. Calling
    /// `place_easy` with any non-`Limit` order type returns `Error::Sign`.
    ///
    /// Errors:
    /// - `Error::Config` — `mkt_id` is unknown.
    /// - `Error::Sign` — no `eip_signer` configured, scaling failed, or
    ///   `order_type` is not `Limit`.
    pub async fn place_easy(&self, req: PlaceEasy<'_>) -> Result<PlaceOrderResponse> {
        let client = &self.client;
        let signer = client.eip_signer().cloned().ok_or_else(|| {
            Error::Sign("no eip_signer configured; call ClientBuilder::eip_signer".into())
        })?;
        // Reject unsupported order types BEFORE signing. Exchange does
        // not implement a true MARKET order — accepting MARKET here would
        // sign + post with surprising semantics. STOP / TWAP need extra
        // fields this helper doesn't expose.
        match req.order_type {
            OrderType::Limit => {}
            other => {
                return Err(Error::Sign(format!(
                    "place_easy supports Limit only; got {other:?}. \
                     For MARKET-like behavior use Limit + TimeInForce::Ioc + a crossing price. \
                     Build a raw PlaceOrderRequest + Client::sign_place_order for STOP/TWAP/etc.",
                )));
            }
        }
        let market = client.resolve_market(req.mkt_id).await?;
        let market_index = MarketCache::idx_as_u8(&market)?;
        let size_x18 = scale_f64(req.size)?;
        let price_x18 = scale_f64(req.price)?;
        let nonce = if req.nonce == 0 {
            now_unix_nanos()
        } else {
            req.nonce
        };

        let payload = OrderPayload {
            sender: signer.address(),
            market_index,
            side: match req.side {
                OrderSide::Buy => SignOrderSide::Buy,
                OrderSide::Sell => SignOrderSide::Sell,
                _ => {
                    return Err(Error::Sign(format!(
                        "side must be Buy or Sell, got {:?}",
                        req.side
                    )))
                }
            },
            size: size_x18,
            price: price_x18,
            nonce,
        };
        let domain = client.eip_domain_clone();
        let sig = sign_order(signer.as_ref(), &domain, payload)?;

        let placed = PlaceOrderRequest {
            mkt_id: market.mkt_id.clone(),
            sd: req.side as i32,
            ot: req.order_type as i32,
            sz: req.size,
            px: req.price,
            tif: req.tif as i32,
            po: req.post_only,
            ro: req.reduce_only,
            stp: req.stp as i32,
            cl_oid: req.client_order_id.unwrap_or_default().to_string(),
            nonce,
            sig: signature_hex(&sig),
            r#await: req.await_match,
            ..Default::default()
        };
        self.place(placed).await
    }
}

/// Inputs for [`OrdersApi::place_easy`]. Mirrors
/// [`PlaceOrderRequest`] minus the fields the helper fills in (`nonce`,
/// `sig`). Optional fields default to "off"/"unspecified" — the same
/// proto defaults a hand-built request would produce.
#[derive(Debug, Clone)]
pub struct PlaceEasy<'a> {
    /// Market symbol (e.g. `"BTC-PERP"`).
    pub mkt_id: &'a str,
    /// Buy or sell — anything else returns `Error::Sign`.
    pub side: OrderSide,
    /// `Limit` only. The exchange has no true MARKET order — use a
    /// crossing limit with `tif = TimeInForce::Ioc` for that behavior.
    /// MARKET / STOP / TWAP / GTT / scheduled types are rejected here;
    /// see [`OrdersApi::place_easy`] doc for the rationale.
    pub order_type: OrderType,
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
    pub client_order_id: Option<&'a str>,
    /// `0` → use wall-clock nanos. Pass non-zero for deterministic test
    /// fixtures or idempotent retry.
    pub nonce: u64,
    /// If true, server waits for matching-engine confirmation before
    /// returning. Only valid for LIMIT.
    pub await_match: bool,
}

impl<'a> PlaceEasy<'a> {
    /// Common shape: a LIMIT order with sane defaults (no STP, no
    /// post-only, GTC, server-assigned cl_oid).
    pub fn limit(mkt_id: &'a str, side: OrderSide, price: f64, size: f64) -> Self {
        Self {
            mkt_id,
            side,
            order_type: OrderType::Limit,
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
}

/// Wall-clock nanos since the Unix epoch — matches the Go `time.Now().UnixNano()`
/// pattern used as the default order nonce upstream
/// (`pkg/exc/client.go::Place`).
fn now_unix_nanos() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        // Never panics for any realistic clock — a pre-1970 wall clock
        // would be a bigger problem than nonce uniqueness.
        .unwrap_or(0)
}
