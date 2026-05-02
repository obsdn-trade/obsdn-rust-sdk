//! Account REST surface — `AccountService` in `api/proto/nil/v1/account.proto`.

use std::sync::Arc;

use crate::error::Result;
use crate::rest::{Auth, RestClient};
use crate::types::v1::{
    FaucetRequest, FaucetResponse, GetAccountRequest, GetAccountResponse,
    GetTransferHistoryRequest, GetTransferHistoryResponse, GetWithdrawalRequestsRequest,
    GetWithdrawalRequestsResponse, SendFundsRequest, SendFundsResponse, WithdrawCollateralRequest,
    WithdrawCollateralResponse,
};

/// Cheap handle to the account endpoints.
#[derive(Debug, Clone)]
pub struct AccountApi {
    rest: Arc<RestClient>,
}

impl AccountApi {
    pub(crate) fn new(rest: Arc<RestClient>) -> Self {
        Self { rest }
    }

    /// `GET /accounts` — get authenticated account info.
    /// **Auth:** required (read-only allowed).
    pub async fn get(&self, req: GetAccountRequest) -> Result<GetAccountResponse> {
        self.rest
            .get_with_query("/accounts", &req, Auth::Required)
            .await
    }

    /// `POST /faucet` — request testnet funds.
    /// **INTERNAL** endpoint — only reachable from internal/Twingate hosts.
    #[doc(hidden)]
    pub async fn faucet(&self, req: FaucetRequest) -> Result<FaucetResponse> {
        self.rest.post("/faucet", &req, Auth::Optional).await
    }

    /// `POST /transfers/withdraw` — withdraw collateral on-chain.
    /// **Auth:** required. EIP-712 signed request.
    pub async fn withdraw_collateral(
        &self,
        req: WithdrawCollateralRequest,
    ) -> Result<WithdrawCollateralResponse> {
        self.rest
            .post("/transfers/withdraw", &req, Auth::Required)
            .await
    }

    /// `GET /transfers/history` — paginated transfer history.
    /// **Auth:** required (read-only allowed).
    pub async fn get_transfer_history(
        &self,
        req: GetTransferHistoryRequest,
    ) -> Result<GetTransferHistoryResponse> {
        self.rest
            .get_with_query("/transfers/history", &req, Auth::Required)
            .await
    }

    /// `GET /transfers/withdrawal-requests` — pending/finalized withdrawals.
    /// **Auth:** required (read-only allowed).
    pub async fn get_withdrawal_requests(
        &self,
        req: GetWithdrawalRequestsRequest,
    ) -> Result<GetWithdrawalRequestsResponse> {
        self.rest
            .get_with_query("/transfers/withdrawal-requests", &req, Auth::Required)
            .await
    }

    /// `POST /transfers/send-funds` — send funds to another account.
    /// **Auth:** required.
    pub async fn send_funds(&self, req: SendFundsRequest) -> Result<SendFundsResponse> {
        self.rest
            .post("/transfers/send-funds", &req, Auth::Required)
            .await
    }
}
