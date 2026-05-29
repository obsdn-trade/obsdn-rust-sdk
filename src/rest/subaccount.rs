//! Subaccount REST surface (`/subaccounts/...`).

use std::sync::Arc;

use crate::error::Result;
use crate::rest::{AuthMode, RestClient};
use crate::types::v1::{
    CreateSubaccountRequest, CreateSubaccountResponse, DeleteSubaccountRequest,
    DeleteSubaccountResponse, GetSubaccountCollateralRequest, GetSubaccountCollateralResponse,
    GetSubaccountPortfolioHistoryRequest, GetSubaccountPortfolioHistoryResponse,
    GetSubaccountPortfolioRequest, GetSubaccountPortfolioResponse, SetSubaccountFrozenRequest,
    SetSubaccountFrozenResponse,
};

/// Cheap handle to subaccount endpoints.
#[derive(Debug, Clone)]
pub struct Subaccount {
    rest: Arc<RestClient>,
}

impl Subaccount {
    pub(crate) fn new(rest: Arc<RestClient>) -> Self {
        Self { rest }
    }

    /// `POST /subaccounts` - create a new subaccount.
    /// **Auth:** required.
    pub async fn create(&self, req: CreateSubaccountRequest) -> Result<CreateSubaccountResponse> {
        self.rest
            .post("/subaccounts", &req, AuthMode::Required)
            .await
    }

    /// `POST /subaccounts/frozen` - freeze/unfreeze a subaccount.
    /// **Auth:** required.
    pub async fn set_frozen(
        &self,
        req: SetSubaccountFrozenRequest,
    ) -> Result<SetSubaccountFrozenResponse> {
        self.rest
            .post("/subaccounts/frozen", &req, AuthMode::Required)
            .await
    }

    /// `GET /subaccounts/portfolio` - portfolio for a subaccount.
    /// **Auth:** required (read-only allowed).
    pub async fn portfolio(
        &self,
        req: GetSubaccountPortfolioRequest,
    ) -> Result<GetSubaccountPortfolioResponse> {
        self.rest
            .get_with_query("/subaccounts/portfolio", &req, AuthMode::Required)
            .await
    }

    /// `GET /subaccounts/collateral` - collateral assets for a subaccount.
    /// **Auth:** required (read-only allowed).
    pub async fn collateral(
        &self,
        req: GetSubaccountCollateralRequest,
    ) -> Result<GetSubaccountCollateralResponse> {
        self.rest
            .get_with_query("/subaccounts/collateral", &req, AuthMode::Required)
            .await
    }

    /// `GET /subaccounts/portfolio/history` - historical portfolio snapshots.
    /// **Auth:** required (read-only allowed).
    pub async fn portfolio_history(
        &self,
        req: GetSubaccountPortfolioHistoryRequest,
    ) -> Result<GetSubaccountPortfolioHistoryResponse> {
        self.rest
            .get_with_query("/subaccounts/portfolio/history", &req, AuthMode::Required)
            .await
    }

    /// `DELETE /subaccounts` - delete a subaccount.
    /// **Auth:** required.
    pub async fn delete(&self, req: DeleteSubaccountRequest) -> Result<DeleteSubaccountResponse> {
        self.rest
            .delete_with_body("/subaccounts", &req, AuthMode::Required)
            .await
    }
}
