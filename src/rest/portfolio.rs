//! Portfolio REST surface (`/portfolio/...`, `/positions/...`, `/funding/...`).

use std::sync::Arc;

use crate::error::Result;
use crate::rest::query::percent_encode_segment;
use crate::rest::{AuthMode, RestClient};
use crate::types::v1::{
    GetFundingPaymentsRequest, GetFundingPaymentsResponse, GetPnLHistoryRequest,
    GetPnLHistoryResponse, GetPortfolioHistoryRequest, GetPortfolioHistoryResponse,
    GetPortfolioRequest, GetPortfolioResponse, GetPositionHistoryRequest,
    GetPositionHistoryResponse, GetTradingCalendarRequest, GetTradingCalendarResponse,
    PlaceOrderRequest, SetLeverageRequest, SetLeverageResponse, SetMarginModeRequest,
    SetMarginModeResponse, TransferMarginRequest, TransferMarginResponse,
};

/// Cheap handle to portfolio endpoints.
#[derive(Debug, Clone)]
pub struct Portfolio {
    rest: Arc<RestClient>,
}

impl Portfolio {
    pub(crate) fn new(rest: Arc<RestClient>) -> Self {
        Self { rest }
    }

    /// `GET /portfolio` - current portfolio (collateral, positions, margin).
    /// **Auth:** required (read-only allowed).
    pub async fn get(&self, req: GetPortfolioRequest) -> Result<GetPortfolioResponse> {
        self.rest
            .get_with_query("/portfolio", &req, AuthMode::Required)
            .await
    }

    /// `GET /positions/history` - historical position changes.
    /// **Auth:** required (read-only allowed).
    pub async fn position_history(
        &self,
        req: GetPositionHistoryRequest,
    ) -> Result<GetPositionHistoryResponse> {
        self.rest
            .get_with_query("/positions/history", &req, AuthMode::Required)
            .await
    }

    /// `GET /funding/payments` - funding payments paid/received.
    /// **Auth:** required (read-only allowed).
    pub async fn funding_payments(
        &self,
        req: GetFundingPaymentsRequest,
    ) -> Result<GetFundingPaymentsResponse> {
        self.rest
            .get_with_query("/funding/payments", &req, AuthMode::Required)
            .await
    }

    /// `GET /portfolio/history` - historical portfolio snapshots.
    /// **Auth:** required (read-only allowed).
    pub async fn history(
        &self,
        req: GetPortfolioHistoryRequest,
    ) -> Result<GetPortfolioHistoryResponse> {
        self.rest
            .get_with_query("/portfolio/history", &req, AuthMode::Required)
            .await
    }

    /// `GET /portfolio/pnl-history` - historical PnL.
    /// **Auth:** required (read-only allowed).
    pub async fn pnl_history(&self, req: GetPnLHistoryRequest) -> Result<GetPnLHistoryResponse> {
        self.rest
            .get_with_query("/portfolio/pnl-history", &req, AuthMode::Required)
            .await
    }

    /// `POST /portfolio/preview` - preview portfolio change after a hypothetical order.
    /// **Auth:** required (read-only allowed). **INTERNAL** endpoint.
    #[doc(hidden)]
    pub async fn preview_order(&self, req: PlaceOrderRequest) -> Result<GetPortfolioResponse> {
        self.rest
            .post("/portfolio/preview", &req, AuthMode::Required)
            .await
    }

    /// `GET /portfolio/trading-calendar` - market trading hours.
    /// **Auth:** required (read-only allowed).
    pub async fn trading_calendar(
        &self,
        req: GetTradingCalendarRequest,
    ) -> Result<GetTradingCalendarResponse> {
        self.rest
            .get_with_query("/portfolio/trading-calendar", &req, AuthMode::Required)
            .await
    }

    /// `POST /positions/{mkt_id}/leverage` - update leverage for a market position.
    /// **Auth:** required.
    pub async fn set_leverage(&self, req: SetLeverageRequest) -> Result<SetLeverageResponse> {
        let path = format!(
            "/positions/{}/leverage",
            percent_encode_segment(&req.mkt_id)
        );
        self.rest.post(&path, &req, AuthMode::Required).await
    }

    /// `POST /positions/{mkt_id}/margin-mode` - switch cross/isolated margin.
    /// **Auth:** required.
    pub async fn set_margin_mode(
        &self,
        req: SetMarginModeRequest,
    ) -> Result<SetMarginModeResponse> {
        let path = format!(
            "/positions/{}/margin-mode",
            percent_encode_segment(&req.mkt_id)
        );
        self.rest.post(&path, &req, AuthMode::Required).await
    }

    /// `POST /positions/{mkt_id}/margin` - add/remove margin on an isolated position.
    /// **Auth:** required.
    pub async fn transfer_margin(
        &self,
        req: TransferMarginRequest,
    ) -> Result<TransferMarginResponse> {
        let path = format!("/positions/{}/margin", percent_encode_segment(&req.mkt_id));
        self.rest.post(&path, &req, AuthMode::Required).await
    }
}
