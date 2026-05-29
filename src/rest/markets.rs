//! Markets REST surface - `MarketService` in `api/proto/nil/v1/market.proto`.

use std::sync::Arc;

use crate::error::Result;
use crate::rest::query::percent_encode_segment;
use crate::rest::{Auth, RestClient};
use crate::types::v1::{
    GetAccountTradeHistoryRequest, GetAccountTradeHistoryResponse, GetFundingRateHistoryRequest,
    GetFundingRateHistoryResponse, GetMarketCandlesRequest, GetMarketCandlesResponse,
    GetMarketTradesRequest, GetMarketTradesResponse, GetMarketsResponse, GetOrderBookResponse,
};

/// Cheap handle to the market data endpoints.
#[derive(Debug, Clone)]
pub struct MarketsApi {
    rest: Arc<RestClient>,
}

impl MarketsApi {
    pub(crate) fn new(rest: Arc<RestClient>) -> Self {
        Self { rest }
    }

    /// `GET /markets` - list all available trading markets.
    /// **Auth:** none.
    pub async fn get_markets(&self) -> Result<GetMarketsResponse> {
        self.rest.get("/markets", Auth::None).await
    }

    /// `GET /markets/{mkt_id}/orderbook` - current order book.
    /// **Auth:** none.
    pub async fn get_order_book(&self, mkt_id: &str) -> Result<GetOrderBookResponse> {
        let path = format!("/markets/{}/orderbook", percent_encode_segment(mkt_id));
        self.rest.get(&path, Auth::None).await
    }

    /// `GET /markets/{mkt_id}/trades` - recent trades for a market.
    /// **Auth:** none. `req.mkt_id` is consumed for the path; remaining
    /// fields go in the query string.
    pub async fn get_market_trades(
        &self,
        mut req: GetMarketTradesRequest,
    ) -> Result<GetMarketTradesResponse> {
        let mkt_id = std::mem::take(&mut req.mkt_id);
        let path = format!("/markets/{}/trades", percent_encode_segment(&mkt_id));
        self.rest.get_with_query(&path, &req, Auth::None).await
    }

    /// `GET /markets/{mkt_id}/candles` - historical OHLCV data.
    /// **Auth:** none.
    pub async fn get_market_candles(
        &self,
        mut req: GetMarketCandlesRequest,
    ) -> Result<GetMarketCandlesResponse> {
        let mkt_id = std::mem::take(&mut req.mkt_id);
        let path = format!("/markets/{}/candles", percent_encode_segment(&mkt_id));
        self.rest.get_with_query(&path, &req, Auth::None).await
    }

    /// `GET /markets/{mkt_id}/funding-rate-history` - funding rate history.
    /// **Auth:** none.
    pub async fn get_funding_rate_history(
        &self,
        mut req: GetFundingRateHistoryRequest,
    ) -> Result<GetFundingRateHistoryResponse> {
        let mkt_id = std::mem::take(&mut req.mkt_id);
        let path = format!(
            "/markets/{}/funding-rate-history",
            percent_encode_segment(&mkt_id)
        );
        self.rest.get_with_query(&path, &req, Auth::None).await
    }

    /// `GET /trade-history` - authenticated account's trade history.
    /// **Auth:** required (read-only allowed).
    pub async fn get_account_trade_history(
        &self,
        req: GetAccountTradeHistoryRequest,
    ) -> Result<GetAccountTradeHistoryResponse> {
        self.rest
            .get_with_query("/trade-history", &req, Auth::Required)
            .await
    }
}
