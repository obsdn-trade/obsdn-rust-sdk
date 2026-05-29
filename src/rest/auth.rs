//! Auth REST surface (`/auth/...`).
//!
//! Reached via [`crate::Client::auth`]. Distinct from the crate-internal
//! HMAC signing module (`crate::auth`, `pub(crate)`).

use std::sync::Arc;

use crate::error::Result;
use crate::rest::{AuthMode, RestClient};
use crate::types::v1::{
    CreateApiKeyRequest, CreateApiKeyResponse, DeleteApiKeysRequest, DeleteApiKeysResponse,
    GetApiKeysRequest, GetApiKeysResponse, GetChildAccountApiKeysRequest,
    GetChildAccountApiKeysResponse, RegisterChildAccountSignerRequest,
    RegisterChildAccountSignerResponse, RegisterSignerRequest, RegisterSignerResponse,
};

/// Cheap handle to auth endpoints.
#[derive(Debug, Clone)]
pub struct Auth {
    rest: Arc<RestClient>,
}

impl Auth {
    pub(crate) fn new(rest: Arc<RestClient>) -> Self {
        Self { rest }
    }

    /// `POST /auth/signers` - register a signer + bootstrap an API key.
    /// **Auth:** none. Wallet + signer signatures inside the body.
    pub async fn register_signer(
        &self,
        req: RegisterSignerRequest,
    ) -> Result<RegisterSignerResponse> {
        self.rest.post("/auth/signers", &req, AuthMode::None).await
    }

    /// `POST /auth/api-keys` - issue an additional API key for the
    /// authenticated wallet.
    /// **Auth:** required.
    pub async fn create_api_key(&self, req: CreateApiKeyRequest) -> Result<CreateApiKeyResponse> {
        self.rest
            .post("/auth/api-keys", &req, AuthMode::Required)
            .await
    }

    /// `GET /auth/api-keys` - list API keys for the authenticated wallet.
    /// **Auth:** required (read-only allowed).
    pub async fn api_keys(&self, req: GetApiKeysRequest) -> Result<GetApiKeysResponse> {
        self.rest
            .get_with_query("/auth/api-keys", &req, AuthMode::Required)
            .await
    }

    /// `DELETE /auth/api-keys` - revoke API keys.
    /// **Auth:** required.
    pub async fn delete_api_keys(
        &self,
        req: DeleteApiKeysRequest,
    ) -> Result<DeleteApiKeysResponse> {
        self.rest
            .delete_with_body("/auth/api-keys", &req, AuthMode::Required)
            .await
    }

    /// `POST /auth/child-accounts/signers` - register a signer for a child
    /// (subaccount) wallet.
    /// **Auth:** required.
    pub async fn register_child_account_signer(
        &self,
        req: RegisterChildAccountSignerRequest,
    ) -> Result<RegisterChildAccountSignerResponse> {
        self.rest
            .post("/auth/child-accounts/signers", &req, AuthMode::Required)
            .await
    }

    /// `GET /auth/child-accounts/api-keys` - list API keys for child accounts.
    /// **Auth:** required (read-only allowed).
    pub async fn child_account_api_keys(
        &self,
        req: GetChildAccountApiKeysRequest,
    ) -> Result<GetChildAccountApiKeysResponse> {
        self.rest
            .get_with_query("/auth/child-accounts/api-keys", &req, AuthMode::Required)
            .await
    }
}
