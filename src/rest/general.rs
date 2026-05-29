//! General REST surface (`/client`, `/error-codes`, `/fee-tiers`).

use std::sync::Arc;

use crate::error::Result;
use crate::rest::{AuthMode, RestClient};
use crate::types::v1::{
    GetClientInfoRequest, GetClientInfoResponse, GetErrorCodesRequest, GetErrorCodesResponse,
    GetFeeTiersRequest, GetFeeTiersResponse,
};

/// Cheap handle to general endpoints.
#[derive(Debug, Clone)]
pub struct General {
    rest: Arc<RestClient>,
}

impl General {
    pub(crate) fn new(rest: Arc<RestClient>) -> Self {
        Self { rest }
    }

    /// `GET /client` - recommended client config (rate limits, etc).
    /// **Auth:** none.
    pub async fn client_info(&self, req: GetClientInfoRequest) -> Result<GetClientInfoResponse> {
        self.rest
            .get_with_query("/client", &req, AuthMode::None)
            .await
    }

    /// `GET /error-codes` - enumerate server error codes for client mapping.
    /// **Auth:** none.
    pub async fn error_codes(&self, req: GetErrorCodesRequest) -> Result<GetErrorCodesResponse> {
        self.rest
            .get_with_query("/error-codes", &req, AuthMode::None)
            .await
    }

    /// `GET /fee-tiers` - all available fee tier definitions.
    /// **Auth:** none.
    pub async fn fee_tiers(&self, req: GetFeeTiersRequest) -> Result<GetFeeTiersResponse> {
        self.rest
            .get_with_query("/fee-tiers", &req, AuthMode::None)
            .await
    }
}
