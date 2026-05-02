//! Portfolio REST surface — `PortfolioService` in
//! `api/proto/nil/v1/portfolio.proto`.

use std::sync::Arc;

use crate::error::Result;
use crate::rest::{Auth, RestClient};
use crate::types::v1::{
    GetFundingPaymentsRequest, GetFundingPaymentsResponse, GetPnLHistoryRequest,
    GetPnLHistoryResponse, GetPortfolioHistoryRequest, GetPortfolioHistoryResponse,
    GetPortfolioRequest, GetPortfolioResponse, GetPositionHistoryRequest,
    GetPositionHistoryResponse, GetTradingCalendarRequest, GetTradingCalendarResponse,
    PlaceOrderRequest,
};

/// Cheap handle to portfolio endpoints.
#[derive(Debug, Clone)]
pub struct PortfolioApi {
    rest: Arc<RestClient>,
}

impl PortfolioApi {
    pub(crate) fn new(rest: Arc<RestClient>) -> Self {
        Self { rest }
    }

    /// `GET /portfolio` — current portfolio (collateral, positions, margin).
    /// **Auth:** required (read-only allowed).
    pub async fn get(&self, req: GetPortfolioRequest) -> Result<GetPortfolioResponse> {
        self.rest
            .get_with_query("/portfolio", &req, Auth::Required)
            .await
    }

    /// `GET /positions/history` — historical position changes.
    /// **Auth:** required (read-only allowed).
    pub async fn get_position_history(
        &self,
        req: GetPositionHistoryRequest,
    ) -> Result<GetPositionHistoryResponse> {
        self.rest
            .get_with_query("/positions/history", &req, Auth::Required)
            .await
    }

    /// `GET /funding/payments` — funding payments paid/received.
    /// **Auth:** required (read-only allowed).
    pub async fn get_funding_payments(
        &self,
        req: GetFundingPaymentsRequest,
    ) -> Result<GetFundingPaymentsResponse> {
        self.rest
            .get_with_query("/funding/payments", &req, Auth::Required)
            .await
    }

    /// `GET /portfolio/history` — historical portfolio snapshots.
    /// **Auth:** required (read-only allowed).
    pub async fn get_history(
        &self,
        req: GetPortfolioHistoryRequest,
    ) -> Result<GetPortfolioHistoryResponse> {
        self.rest
            .get_with_query("/portfolio/history", &req, Auth::Required)
            .await
    }

    /// `GET /portfolio/pnl-history` — historical PnL.
    /// **Auth:** required (read-only allowed).
    pub async fn get_pnl_history(
        &self,
        req: GetPnLHistoryRequest,
    ) -> Result<GetPnLHistoryResponse> {
        self.rest
            .get_with_query("/portfolio/pnl-history", &req, Auth::Required)
            .await
    }

    /// `POST /portfolio/preview` — preview portfolio change after a hypothetical order.
    /// **Auth:** required (read-only allowed). **INTERNAL** endpoint.
    #[doc(hidden)]
    pub async fn preview_order(&self, req: PlaceOrderRequest) -> Result<GetPortfolioResponse> {
        self.rest
            .post("/portfolio/preview", &req, Auth::Required)
            .await
    }

    /// `GET /portfolio/trading-calendar` — market trading hours.
    /// **Auth:** required (read-only allowed).
    pub async fn get_trading_calendar(
        &self,
        req: GetTradingCalendarRequest,
    ) -> Result<GetTradingCalendarResponse> {
        self.rest
            .get_with_query("/portfolio/trading-calendar", &req, Auth::Required)
            .await
    }
}
