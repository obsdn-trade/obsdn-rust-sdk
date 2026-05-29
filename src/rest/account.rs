//! Account REST surface (`/accounts`, `/transfers/...`).

use std::sync::Arc;

use alloy_primitives::Address;

use crate::builder::Client;
use crate::error::{Error, Result};
use crate::rest::{AuthMode, RestClient};
use crate::sign::{
    scale_decimal_str, sign_transfer, sign_withdraw, signature_hex, TransferPayload,
    WithdrawPayload,
};
use crate::types::v1::{
    FaucetRequest, FaucetResponse, GetAccountRequest, GetAccountResponse,
    GetTransferHistoryRequest, GetTransferHistoryResponse, GetWithdrawalRequestsRequest,
    GetWithdrawalRequestsResponse, SendFundsRequest, SendFundsResponse, WithdrawCollateralRequest,
    WithdrawCollateralResponse,
};

/// Cheap handle to the account endpoints. Carries a back-reference to the
/// owning [`Client`] so the one-call [`Self::transfer`] / [`Self::withdraw`]
/// helpers can resolve the EIP-712 signer + domain in a single call.
#[derive(Debug, Clone)]
pub struct Account {
    rest: Arc<RestClient>,
    client: Client,
}

impl Account {
    pub(crate) fn with_client(rest: Arc<RestClient>, client: Client) -> Self {
        Self { rest, client }
    }

    /// `GET /accounts` - get authenticated account info.
    /// **Auth:** required (read-only allowed).
    pub async fn get(&self, req: GetAccountRequest) -> Result<GetAccountResponse> {
        self.rest
            .get_with_query("/accounts", &req, AuthMode::Required)
            .await
    }

    /// `POST /faucet` - request testnet funds.
    /// **INTERNAL** endpoint.
    #[doc(hidden)]
    pub async fn faucet(&self, req: FaucetRequest) -> Result<FaucetResponse> {
        self.rest.post("/faucet", &req, AuthMode::Optional).await
    }

    /// `POST /transfers/withdraw` - withdraw collateral on-chain.
    /// **Auth:** required. EIP-712 signed request.
    ///
    /// Low-level: the caller supplies a pre-signed [`WithdrawCollateralRequest`].
    /// Prefer [`Self::withdraw`] for the scale-sign-post flow.
    pub async fn withdraw_collateral(
        &self,
        req: WithdrawCollateralRequest,
    ) -> Result<WithdrawCollateralResponse> {
        self.rest
            .post("/transfers/withdraw", &req, AuthMode::Required)
            .await
    }

    /// `GET /transfers/history` - paginated transfer history.
    /// **Auth:** required (read-only allowed).
    pub async fn transfer_history(
        &self,
        req: GetTransferHistoryRequest,
    ) -> Result<GetTransferHistoryResponse> {
        self.rest
            .get_with_query("/transfers/history", &req, AuthMode::Required)
            .await
    }

    /// `GET /transfers/withdrawal-requests` - pending/finalized withdrawals.
    /// **Auth:** required (read-only allowed).
    pub async fn withdrawal_requests(
        &self,
        req: GetWithdrawalRequestsRequest,
    ) -> Result<GetWithdrawalRequestsResponse> {
        self.rest
            .get_with_query("/transfers/withdrawal-requests", &req, AuthMode::Required)
            .await
    }

    /// `POST /transfers/send-funds` - send funds to another account.
    /// **Auth:** required.
    ///
    /// Low-level: the caller supplies a pre-signed [`SendFundsRequest`].
    /// Prefer [`Self::transfer`] for the scale-sign-post flow.
    pub async fn send_funds(&self, req: SendFundsRequest) -> Result<SendFundsResponse> {
        self.rest
            .post("/transfers/send-funds", &req, AuthMode::Required)
            .await
    }

    /// One-call signed transfer: scale `amount`, sign the EIP-712 `Transfer`
    /// payload with the configured signer, and POST `/transfers/send-funds`.
    ///
    /// `from` is the client's sender address (the main wallet in
    /// delegated-signing mode, otherwise the signer's own address). The nonce
    /// is wall-clock nanoseconds.
    ///
    /// Errors:
    /// - `Error::Sign` - no `eip712_signer` configured, or `amount` is not a
    ///   positive finite number.
    pub async fn transfer(
        &self,
        to: Address,
        token: Address,
        amount: f64,
    ) -> Result<SendFundsResponse> {
        let signer = self.client.eip712_signer().cloned().ok_or_else(|| {
            Error::Sign("no eip712_signer configured; call ClientBuilder::eip712_signer".into())
        })?;
        if !amount.is_finite() || amount <= 0.0 {
            return Err(Error::Sign(
                "transfer amount must be a positive finite number".into(),
            ));
        }
        let from = self.client.sender_address();
        // The signed `amount` and the wire `amt` string are derived from the
        // same decimal text, so the server scales an identical value.
        let amt = format!("{amount}");
        let amount_x18 = scale_decimal_str(&amt)?;
        let nonce = super::now_unix_nanos();
        let payload = TransferPayload {
            from,
            to,
            token,
            amount: amount_x18,
            nonce,
        };
        let domain = self.client.eip_domain_clone();
        let sig = sign_transfer(signer.as_ref(), &domain, payload)?;
        let req = SendFundsRequest {
            from: format!("{from:#x}"),
            to: format!("{to:#x}"),
            tkn: format!("{token:#x}"),
            amt,
            nonce,
            sig: signature_hex(&sig),
        };
        self.send_funds(req).await
    }

    /// One-call signed withdrawal: scale `amount`, sign the EIP-712 `Withdraw`
    /// payload, and POST `/transfers/withdraw`. The chain-writer service then
    /// submits the on-chain transaction; observe completion via the
    /// `notification` WS channel.
    ///
    /// Errors:
    /// - `Error::Sign` - no `eip712_signer` configured, or `amount` is not a
    ///   positive finite number.
    pub async fn withdraw(
        &self,
        token: Address,
        amount: f64,
    ) -> Result<WithdrawCollateralResponse> {
        let signer = self.client.eip712_signer().cloned().ok_or_else(|| {
            Error::Sign("no eip712_signer configured; call ClientBuilder::eip712_signer".into())
        })?;
        if !amount.is_finite() || amount <= 0.0 {
            return Err(Error::Sign(
                "withdraw amount must be a positive finite number".into(),
            ));
        }
        let sender = self.client.sender_address();
        let amt = format!("{amount}");
        let amount_x18 = scale_decimal_str(&amt)?;
        let nonce = super::now_unix_nanos();
        let payload = WithdrawPayload {
            sender,
            token,
            amount: amount_x18,
            nonce,
        };
        let domain = self.client.eip_domain_clone();
        let sig = sign_withdraw(signer.as_ref(), &domain, payload)?;
        let req = WithdrawCollateralRequest {
            tkn: format!("{token:#x}"),
            amt,
            nonce,
            sig: signature_hex(&sig),
        };
        self.withdraw_collateral(req).await
    }
}
