//! Price REST surface — `PriceService` in `api/proto/nil/v1/price.proto`.

use std::sync::Arc;

use crate::error::Result;
use crate::rest::{Auth, RestClient};
use crate::types::v1::{GetPricesRequest, GetPricesResponse};

/// Cheap handle to price endpoints.
#[derive(Debug, Clone)]
pub struct PriceApi {
    rest: Arc<RestClient>,
}

impl PriceApi {
    pub(crate) fn new(rest: Arc<RestClient>) -> Self {
        Self { rest }
    }

    /// `GET /prices` — current oracle prices.
    /// **Auth:** none.
    pub async fn get_prices(&self, req: GetPricesRequest) -> Result<GetPricesResponse> {
        self.rest.get_with_query("/prices", &req, Auth::None).await
    }
}
