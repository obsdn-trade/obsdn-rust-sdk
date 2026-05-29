//! Asset REST surface (`/assets`).

use std::sync::Arc;

use crate::error::Result;
use crate::rest::{AuthMode, RestClient};
use crate::types::v1::{GetAssetsRequest, GetAssetsResponse};

/// Cheap handle to asset endpoints.
#[derive(Debug, Clone)]
pub struct Asset {
    rest: Arc<RestClient>,
}

impl Asset {
    pub(crate) fn new(rest: Arc<RestClient>) -> Self {
        Self { rest }
    }

    /// `GET /assets` - list available assets.
    /// **Auth:** none.
    pub async fn list(&self, req: GetAssetsRequest) -> Result<GetAssetsResponse> {
        self.rest
            .get_with_query("/assets", &req, AuthMode::None)
            .await
    }
}
