use async_trait::async_trait;
use reqwest::{
    header::{HeaderMap, HeaderValue, ACCEPT},
    Client,
};
use serde_json::Value;
use std::time::Duration;

#[async_trait]
pub trait FinanceProvider {
    fn symbol_url(&self, symbol: &str, currency: &str, from: u64, to: u64) -> String;

    async fn retrieve_symbol_data(
        &self,
        symbol: &str,
        currency: &str,
        from: u64,
        to: u64,
    ) -> Result<f64, String>;
}

pub struct CoinGeckoFinance {
    client: Client,
}

impl CoinGeckoFinance {
    pub fn new(api_key: String) -> Result<Self, String> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(
            "x-cg-demo-api-key",
            HeaderValue::from_str(&api_key)
                .map_err(|e| format!("Invalid CoinGecko API key header: {e}"))?,
        );

        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .default_headers(headers)
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

        Ok(Self { client })
    }
}

#[async_trait]
impl FinanceProvider for CoinGeckoFinance {
    fn symbol_url(&self, symbol: &str, currency: &str, from: u64, to: u64) -> String {
        format!(
            "https://api.coingecko.com/api/v3/coins/{}/market_chart/range?vs_currency={}&from={}&to={}",
            symbol, currency, from, to
        )
    }

    async fn retrieve_symbol_data(
        &self,
        symbol: &str,
        currency: &str,
        from: u64,
        to: u64,
    ) -> Result<f64, String> {
        let url = self.symbol_url(symbol, currency, from, to);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("CoinGecko request failed for {symbol}: {e}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("CoinGecko returned HTTP {} for {}: {}", status, symbol, body))
        }

        let json: Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse CoinGecko response for {symbol}: {e}"))?;

        json.get("prices")
            .and_then(|v| v.as_array())
            .and_then(|prices| prices.last())
            .and_then(|entry| entry.get(1))
            .and_then(|v| v.as_f64())
            .ok_or_else(|| format!("Failed to retrieve CoinGecko price for {symbol}"))
    }
}
