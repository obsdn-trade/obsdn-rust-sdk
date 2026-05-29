//! Subaccount REST surface - `SubaccountService` in
//! `api/proto/nil/v1/subaccount.proto`.

use std::sync::Arc;

use crate::error::Result;
use crate::rest::{Auth, RestClient};
use crate::types::v1::{
    CreateSubaccountRequest, CreateSubaccountResponse, DeleteSubaccountRequest,
    DeleteSubaccountResponse, GetSubaccountCollateralRequest, GetSubaccountCollateralResponse,
    GetSubaccountPortfolioHistoryRequest, GetSubaccountPortfolioHistoryResponse,
    GetSubaccountPortfolioRequest, GetSubaccountPortfolioResponse, SetSubaccountFrozenRequest,
    SetSubaccountFrozenResponse,
};

/// Cheap handle to subaccount endpoints.
#[derive(Debug, Clone)]
pub struct SubaccountApi {
    rest: Arc<RestClient>,
}

impl SubaccountApi {
    pub(crate) fn new(rest: Arc<RestClient>) -> Self {
        Self { rest }
    }

    /// `POST /subaccounts` - create a new subaccount.
    /// **Auth:** required.
    pub async fn create(&self, req: CreateSubaccountRequest) -> Result<CreateSubaccountResponse> {
        self.rest.post("/subaccounts", &req, Auth::Required).await
    }

    /// `POST /subaccounts/frozen` - freeze/unfreeze a subaccount.
    /// **Auth:** required.
    pub async fn set_frozen(
        &self,
        req: SetSubaccountFrozenRequest,
    ) -> Result<SetSubaccountFrozenResponse> {
        self.rest
            .post("/subaccounts/frozen", &req, Auth::Required)
            .await
    }

    /// `GET /subaccounts/portfolio` - portfolio for a subaccount.
    /// **Auth:** required (read-only allowed).
    pub async fn get_portfolio(
        &self,
        req: GetSubaccountPortfolioRequest,
    ) -> Result<GetSubaccountPortfolioResponse> {
        self.rest
            .get_with_query("/subaccounts/portfolio", &req, Auth::Required)
            .await
    }

    /// `GET /subaccounts/collateral` - collateral assets for a subaccount.
    /// **Auth:** required (read-only allowed).
    pub async fn get_collateral(
        &self,
        req: GetSubaccountCollateralRequest,
    ) -> Result<GetSubaccountCollateralResponse> {
        self.rest
            .get_with_query("/subaccounts/collateral", &req, Auth::Required)
            .await
    }

    /// `GET /subaccounts/portfolio/history` - historical portfolio snapshots.
    /// **Auth:** required (read-only allowed).
    pub async fn get_portfolio_history(
        &self,
        req: GetSubaccountPortfolioHistoryRequest,
    ) -> Result<GetSubaccountPortfolioHistoryResponse> {
        self.rest
            .get_with_query("/subaccounts/portfolio/history", &req, Auth::Required)
            .await
    }

    /// `DELETE /subaccounts` - delete a subaccount.
    /// **Auth:** required.
    pub async fn delete(&self, req: DeleteSubaccountRequest) -> Result<DeleteSubaccountResponse> {
        self.rest
            .delete_with_body("/subaccounts", &req, Auth::Required)
            .await
    }
}
