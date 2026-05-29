//! Markets REST surface (`/markets/...`, `/trade-history`).

use std::sync::Arc;

use crate::error::Result;
use crate::rest::query::percent_encode_segment;
use crate::rest::{AuthMode, RestClient};
use crate::types::v1::{
    GetAccountTradeHistoryRequest, GetAccountTradeHistoryResponse, GetFundingRateHistoryRequest,
    GetFundingRateHistoryResponse, GetMarketCandlesRequest, GetMarketCandlesResponse,
    GetMarketTradesRequest, GetMarketTradesResponse, GetMarketsResponse, GetOrderBookResponse,
};

/// Cheap handle to the market data endpoints.
#[derive(Debug, Clone)]
pub struct Markets {
    rest: Arc<RestClient>,
}

impl Markets {
    pub(crate) fn new(rest: Arc<RestClient>) -> Self {
        Self { rest }
    }

    /// `GET /markets` - list all available trading markets.
    /// **Auth:** none.
    pub async fn list(&self) -> Result<GetMarketsResponse> {
        self.rest.get("/markets", AuthMode::None).await
    }

    /// `GET /markets/{mkt_id}/orderbook` - current order book.
    /// **Auth:** none.
    pub async fn order_book(&self, mkt_id: &str) -> Result<GetOrderBookResponse> {
        let path = format!("/markets/{}/orderbook", percent_encode_segment(mkt_id));
        self.rest.get(&path, AuthMode::None).await
    }

    /// `GET /markets/{mkt_id}/trades` - recent trades for a market.
    /// **Auth:** none. `req` carries the pagination / time-range fields.
    pub async fn trades(
        &self,
        mkt_id: &str,
        mut req: GetMarketTradesRequest,
    ) -> Result<GetMarketTradesResponse> {
        req.mkt_id = String::new();
        let path = format!("/markets/{}/trades", percent_encode_segment(mkt_id));
        self.rest.get_with_query(&path, &req, AuthMode::None).await
    }

    /// `GET /markets/{mkt_id}/candles` - historical OHLCV data.
    /// **Auth:** none. `req` carries the interval / time-range fields.
    pub async fn candles(
        &self,
        mkt_id: &str,
        mut req: GetMarketCandlesRequest,
    ) -> Result<GetMarketCandlesResponse> {
        req.mkt_id = String::new();
        let path = format!("/markets/{}/candles", percent_encode_segment(mkt_id));
        self.rest.get_with_query(&path, &req, AuthMode::None).await
    }

    /// `GET /markets/{mkt_id}/funding-rate-history` - funding rate history.
    /// **Auth:** none. `req` carries the pagination / time-range fields.
    pub async fn funding_rate_history(
        &self,
        mkt_id: &str,
        mut req: GetFundingRateHistoryRequest,
    ) -> Result<GetFundingRateHistoryResponse> {
        req.mkt_id = String::new();
        let path = format!(
            "/markets/{}/funding-rate-history",
            percent_encode_segment(mkt_id)
        );
        self.rest.get_with_query(&path, &req, AuthMode::None).await
    }

    /// `GET /trade-history` - authenticated account's trade history.
    /// **Auth:** required (read-only allowed).
    pub async fn account_trade_history(
        &self,
        req: GetAccountTradeHistoryRequest,
    ) -> Result<GetAccountTradeHistoryResponse> {
        self.rest
            .get_with_query("/trade-history", &req, AuthMode::Required)
            .await
    }
}
