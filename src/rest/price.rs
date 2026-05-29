//! Price REST surface (`/prices`).

use std::sync::Arc;

use crate::error::Result;
use crate::rest::{AuthMode, RestClient};
use crate::types::v1::{GetPricesRequest, GetPricesResponse};

/// Cheap handle to price endpoints.
#[derive(Debug, Clone)]
pub struct Price {
    rest: Arc<RestClient>,
}

impl Price {
    pub(crate) fn new(rest: Arc<RestClient>) -> Self {
        Self { rest }
    }

    /// `GET /prices` - current oracle prices.
    /// **Auth:** none.
    pub async fn list(&self, req: GetPricesRequest) -> Result<GetPricesResponse> {
        self.rest
            .get_with_query("/prices", &req, AuthMode::None)
            .await
    }
}
