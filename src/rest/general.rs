//! General REST surface — `GeneralService` in `api/proto/nil/v1/general.proto`.

use std::sync::Arc;

use crate::error::Result;
use crate::rest::{Auth, RestClient};
use crate::types::v1::{
    GetClientInfoRequest, GetClientInfoResponse, GetErrorCodesRequest, GetErrorCodesResponse,
    GetFeeTiersRequest, GetFeeTiersResponse,
};

/// Cheap handle to general endpoints.
#[derive(Debug, Clone)]
pub struct GeneralApi {
    rest: Arc<RestClient>,
}

impl GeneralApi {
    pub(crate) fn new(rest: Arc<RestClient>) -> Self {
        Self { rest }
    }

    /// `GET /client` — recommended client config (rate limits, etc).
    /// **Auth:** none.
    pub async fn get_client_info(
        &self,
        req: GetClientInfoRequest,
    ) -> Result<GetClientInfoResponse> {
        self.rest.get_with_query("/client", &req, Auth::None).await
    }

    /// `GET /error-codes` — enumerate server error codes for client mapping.
    /// **Auth:** none.
    pub async fn get_error_codes(
        &self,
        req: GetErrorCodesRequest,
    ) -> Result<GetErrorCodesResponse> {
        self.rest
            .get_with_query("/error-codes", &req, Auth::None)
            .await
    }

    /// `GET /fee-tiers` — all available fee tier definitions.
    /// **Auth:** none.
    pub async fn get_fee_tiers(&self, req: GetFeeTiersRequest) -> Result<GetFeeTiersResponse> {
        self.rest
            .get_with_query("/fee-tiers", &req, Auth::None)
            .await
    }
}
