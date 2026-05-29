//! Asset REST surface - `AssetService` in `api/proto/nil/v1/asset.proto`.

use std::sync::Arc;

use crate::error::Result;
use crate::rest::{Auth, RestClient};
use crate::types::v1::{GetAssetsRequest, GetAssetsResponse};

/// Cheap handle to asset endpoints.
#[derive(Debug, Clone)]
pub struct AssetApi {
    rest: Arc<RestClient>,
}

impl AssetApi {
    pub(crate) fn new(rest: Arc<RestClient>) -> Self {
        Self { rest }
    }

    /// `GET /assets` - list available assets.
    /// **Auth:** none.
    pub async fn get_assets(&self, req: GetAssetsRequest) -> Result<GetAssetsResponse> {
        self.rest.get_with_query("/assets", &req, Auth::None).await
    }
}
