//! Chain REST surface (`/chain/...`).

use std::sync::Arc;

use crate::error::Result;
use crate::rest::{AuthMode, RestClient};
use crate::types::v1::{
    GetChainConfigRequest, GetChainConfigResponse, GetLastOnchainBlockRequest,
    GetLastOnchainBlockResponse, SubmitOnchainEventsRequest, SubmitOnchainEventsResponse,
};

/// Cheap handle to chain endpoints.
#[derive(Debug, Clone)]
pub struct Chain {
    rest: Arc<RestClient>,
}

impl Chain {
    pub(crate) fn new(rest: Arc<RestClient>) -> Self {
        Self { rest }
    }

    /// `GET /chain/config` - read on-chain config (contract addresses, etc).
    /// **Auth:** none.
    pub async fn config(&self, req: GetChainConfigRequest) -> Result<GetChainConfigResponse> {
        self.rest
            .get_with_query("/chain/config", &req, AuthMode::None)
            .await
    }

    /// `GET /chain/last-onchain-block` - last processed block height.
    /// **INTERNAL** endpoint - only reachable from internal hosts.
    #[doc(hidden)]
    pub async fn last_onchain_block(
        &self,
        req: GetLastOnchainBlockRequest,
    ) -> Result<GetLastOnchainBlockResponse> {
        self.rest
            .get_with_query("/chain/last-onchain-block", &req, AuthMode::Optional)
            .await
    }

    /// `POST /chain/onchain-events` - submit batch of on-chain events.
    /// **INTERNAL** endpoint.
    #[doc(hidden)]
    pub async fn submit_onchain_events(
        &self,
        req: SubmitOnchainEventsRequest,
    ) -> Result<SubmitOnchainEventsResponse> {
        self.rest
            .post("/chain/onchain-events", &req, AuthMode::Optional)
            .await
    }
}
