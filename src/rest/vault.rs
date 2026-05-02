//! Vault REST surface — `VaultService` in `api/proto/nil/v1/vault.proto`.
//!
//! Every method here is marked **INTERNAL** in the proto. They're exposed
//! to support internal tooling (bots, dashboards) but not part of the
//! documented public SDK — hidden from `cargo doc` via `#[doc(hidden)]`.

use std::sync::Arc;

use crate::error::Result;
use crate::rest::{Auth, RestClient};
use crate::types::v1::{
    CreateVaultRequest, CreateVaultResponse, GetVaultAccountValueHistoryRequest,
    GetVaultAccountValueHistoryResponse, GetVaultOpenOrdersRequest, GetVaultOpenOrdersResponse,
    GetVaultOrderHistoryRequest, GetVaultOrderHistoryResponse, GetVaultPnLHistoryRequest,
    GetVaultPnLHistoryResponse, GetVaultPortfolioRequest, GetVaultPortfolioResponse,
    GetVaultPositionHistoryRequest, GetVaultPositionHistoryResponse, GetVaultStakerRequest,
    GetVaultStakerResponse, GetVaultStakersRequest, GetVaultStakersResponse, GetVaultStatsRequest,
    GetVaultStatsResponse, GetVaultTradeHistoryRequest, GetVaultTradeHistoryResponse,
    GetVaultTransferHistoryByUserRequest, GetVaultTransferHistoryByUserResponse,
    GetVaultTransferHistoryRequest, GetVaultTransferHistoryResponse, StakeVaultRequest,
    StakeVaultResponse, UnstakeVaultRequest, UnstakeVaultResponse,
};

/// Cheap handle to vault endpoints. **All endpoints are INTERNAL.**
#[derive(Debug, Clone)]
pub struct VaultApi {
    rest: Arc<RestClient>,
}

impl VaultApi {
    pub(crate) fn new(rest: Arc<RestClient>) -> Self {
        Self { rest }
    }

    /// `POST /vaults` — create a new vault. **Auth:** required. **INTERNAL.**
    #[doc(hidden)]
    pub async fn create(&self, req: CreateVaultRequest) -> Result<CreateVaultResponse> {
        self.rest.post("/vaults", &req, Auth::Required).await
    }

    /// `GET /vaults/portfolio` — vault portfolio. **INTERNAL.**
    #[doc(hidden)]
    pub async fn get_portfolio(
        &self,
        req: GetVaultPortfolioRequest,
    ) -> Result<GetVaultPortfolioResponse> {
        self.rest
            .get_with_query("/vaults/portfolio", &req, Auth::Optional)
            .await
    }

    /// `GET /vaults/trade-history` — vault trade history. **INTERNAL.**
    #[doc(hidden)]
    pub async fn get_trade_history(
        &self,
        req: GetVaultTradeHistoryRequest,
    ) -> Result<GetVaultTradeHistoryResponse> {
        self.rest
            .get_with_query("/vaults/trade-history", &req, Auth::Optional)
            .await
    }

    /// `GET /vaults/orders` — vault open orders. **INTERNAL.**
    #[doc(hidden)]
    pub async fn get_open_orders(
        &self,
        req: GetVaultOpenOrdersRequest,
    ) -> Result<GetVaultOpenOrdersResponse> {
        self.rest
            .get_with_query("/vaults/orders", &req, Auth::Optional)
            .await
    }

    /// `GET /vaults/order-history` — vault historical orders. **INTERNAL.**
    #[doc(hidden)]
    pub async fn get_order_history(
        &self,
        req: GetVaultOrderHistoryRequest,
    ) -> Result<GetVaultOrderHistoryResponse> {
        self.rest
            .get_with_query("/vaults/order-history", &req, Auth::Optional)
            .await
    }

    /// `GET /vaults/account-value-history` — vault NAV history. **INTERNAL.**
    #[doc(hidden)]
    pub async fn get_account_value_history(
        &self,
        req: GetVaultAccountValueHistoryRequest,
    ) -> Result<GetVaultAccountValueHistoryResponse> {
        self.rest
            .get_with_query("/vaults/account-value-history", &req, Auth::Optional)
            .await
    }

    /// `GET /vaults/pnl-history` — vault PnL history. **INTERNAL.**
    #[doc(hidden)]
    pub async fn get_pnl_history(
        &self,
        req: GetVaultPnLHistoryRequest,
    ) -> Result<GetVaultPnLHistoryResponse> {
        self.rest
            .get_with_query("/vaults/pnl-history", &req, Auth::Optional)
            .await
    }

    /// `POST /vaults/stake` — stake into a vault. **Auth:** required. **INTERNAL.**
    #[doc(hidden)]
    pub async fn stake(&self, req: StakeVaultRequest) -> Result<StakeVaultResponse> {
        self.rest.post("/vaults/stake", &req, Auth::Required).await
    }

    /// `POST /vaults/unstake` — unstake from a vault. **Auth:** required. **INTERNAL.**
    #[doc(hidden)]
    pub async fn unstake(&self, req: UnstakeVaultRequest) -> Result<UnstakeVaultResponse> {
        self.rest
            .post("/vaults/unstake", &req, Auth::Required)
            .await
    }

    /// `GET /vaults/transfer-history` — vault stake/unstake history. **INTERNAL.**
    #[doc(hidden)]
    pub async fn get_transfer_history(
        &self,
        req: GetVaultTransferHistoryRequest,
    ) -> Result<GetVaultTransferHistoryResponse> {
        self.rest
            .get_with_query("/vaults/transfer-history", &req, Auth::Optional)
            .await
    }

    /// `GET /vaults/user-transfer-history` — caller's vault transfers.
    /// **Auth:** required (read-only allowed). **INTERNAL.**
    #[doc(hidden)]
    pub async fn get_user_transfer_history(
        &self,
        req: GetVaultTransferHistoryByUserRequest,
    ) -> Result<GetVaultTransferHistoryByUserResponse> {
        self.rest
            .get_with_query("/vaults/user-transfer-history", &req, Auth::Required)
            .await
    }

    /// `GET /vaults/stats` — aggregated vault stats. **INTERNAL.**
    #[doc(hidden)]
    pub async fn get_stats(&self, req: GetVaultStatsRequest) -> Result<GetVaultStatsResponse> {
        self.rest
            .get_with_query("/vaults/stats", &req, Auth::Optional)
            .await
    }

    /// `GET /vaults/stakers` — vault staker list. **INTERNAL.**
    #[doc(hidden)]
    pub async fn get_stakers(
        &self,
        req: GetVaultStakersRequest,
    ) -> Result<GetVaultStakersResponse> {
        self.rest
            .get_with_query("/vaults/stakers", &req, Auth::Optional)
            .await
    }

    /// `GET /vaults/staker` — caller's stake info.
    /// **Auth:** required (read-only allowed). **INTERNAL.**
    #[doc(hidden)]
    pub async fn get_staker(&self, req: GetVaultStakerRequest) -> Result<GetVaultStakerResponse> {
        self.rest
            .get_with_query("/vaults/staker", &req, Auth::Required)
            .await
    }

    /// `GET /vaults/position-history` — vault position history. **INTERNAL.**
    #[doc(hidden)]
    pub async fn get_position_history(
        &self,
        req: GetVaultPositionHistoryRequest,
    ) -> Result<GetVaultPositionHistoryResponse> {
        self.rest
            .get_with_query("/vaults/position-history", &req, Auth::Optional)
            .await
    }
}
