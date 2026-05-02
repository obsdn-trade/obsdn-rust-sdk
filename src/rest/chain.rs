//! Chain REST surface — `ChainService` in `api/proto/nil/v1/chain.proto`.

use std::sync::Arc;

use crate::error::Result;
use crate::rest::{Auth, RestClient};
use crate::types::v1::{
    GetChainConfigRequest, GetChainConfigResponse, GetLastOnchainBlockRequest,
    GetLastOnchainBlockResponse, SubmitOnchainEventsRequest, SubmitOnchainEventsResponse,
};

/// Cheap handle to chain endpoints.
#[derive(Debug, Clone)]
pub struct ChainApi {
    rest: Arc<RestClient>,
}

impl ChainApi {
    pub(crate) fn new(rest: Arc<RestClient>) -> Self {
        Self { rest }
    }

    /// `GET /chain/config` — read on-chain config (contract addresses, etc).
    /// **Auth:** none.
    pub async fn get_chain_config(
        &self,
        req: GetChainConfigRequest,
    ) -> Result<GetChainConfigResponse> {
        self.rest
            .get_with_query("/chain/config", &req, Auth::None)
            .await
    }

    /// `GET /chain/last-onchain-block` — last processed block height.
    /// **INTERNAL** endpoint — only reachable from internal hosts.
    #[doc(hidden)]
    pub async fn get_last_onchain_block(
        &self,
        req: GetLastOnchainBlockRequest,
    ) -> Result<GetLastOnchainBlockResponse> {
        self.rest
            .get_with_query("/chain/last-onchain-block", &req, Auth::Optional)
            .await
    }

    /// `POST /chain/onchain-events` — submit batch of on-chain events.
    /// **INTERNAL** endpoint.
    #[doc(hidden)]
    pub async fn submit_onchain_events(
        &self,
        req: SubmitOnchainEventsRequest,
    ) -> Result<SubmitOnchainEventsResponse> {
        self.rest
            .post("/chain/onchain-events", &req, Auth::Optional)
            .await
    }
}
