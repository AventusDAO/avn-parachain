// Copyright 2026 Aventus DAO Ltd

//! The node's own country, for display in the oracle report events: `GET /node/country`.
//!
//! Resolved once from the node's public address through a geo-IP lookup and cached for the
//! life of the process. It is self-reported and reflects the egress address (a VPN or a cloud
//! load balancer shows the provider's location), so nothing may depend on it. A failed lookup
//! answers [`UNKNOWN_COUNTRY`] and is retried on the next request. `AVN_NODE_COUNTRY` in the
//! environment overrides the lookup, which local networks use so each node shows a different
//! flag.

use alloy::transports::http::reqwest;
use anyhow::{anyhow, Context, Result};
use sp_floor_oracle::{CountryCode, UNKNOWN_COUNTRY};
use std::time::Duration;
use tokio::sync::Mutex;

pub const GEOIP_URL: &str = "https://ipinfo.io/json";
pub const OVERRIDE_ENV: &str = "AVN_NODE_COUNTRY";
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct NodeCountry {
    cached: Mutex<Option<CountryCode>>,
}

impl NodeCountry {
    /// The cached code, or a fresh lookup. Never fails: unknown is a valid answer.
    pub async fn get(&self) -> CountryCode {
        let mut cached = self.cached.lock().await;
        if let Some(country) = *cached {
            return country
        }
        match resolve().await {
            Ok(country) => {
                log::info!("node country: {}", String::from_utf8_lossy(&country));
                *cached = Some(country);
                country
            },
            Err(e) => {
                log::warn!("node country: lookup failed, reporting unknown: {e:#}");
                UNKNOWN_COUNTRY
            },
        }
    }
}

async fn resolve() -> Result<CountryCode> {
    if let Ok(value) = std::env::var(OVERRIDE_ENV) {
        return parse_country(&value).with_context(|| format!("{OVERRIDE_ENV}={value}"))
    }
    let body = reqwest::Client::builder()
        .timeout(LOOKUP_TIMEOUT)
        .build()?
        .get(GEOIP_URL)
        .send()
        .await
        .context("geo-ip request")?
        .error_for_status()
        .context("geo-ip status")?
        .text()
        .await
        .context("geo-ip body")?;
    let json: serde_json::Value = serde_json::from_str(&body).context("geo-ip json")?;
    let code = json
        .get("country")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("geo-ip answer has no country: {body}"))?;
    parse_country(code)
}

/// Two ASCII letters, upper-cased. Anything else is rejected rather than guessed.
pub fn parse_country(value: &str) -> Result<CountryCode> {
    let trimmed = value.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() != 2 || !bytes.iter().all(u8::is_ascii_alphabetic) {
        return Err(anyhow!("not a two letter country code: {value:?}"))
    }
    Ok([bytes[0].to_ascii_uppercase(), bytes[1].to_ascii_uppercase()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn country_codes_are_two_letters_upper_cased() {
        assert_eq!(parse_country("GB").unwrap(), *b"GB");
        assert_eq!(parse_country(" de\n").unwrap(), *b"DE");
        assert!(parse_country("").is_err());
        assert!(parse_country("GBR").is_err());
        assert!(parse_country("1A").is_err());
        assert!(parse_country("??").is_err(), "unknown is reported, never parsed");
    }
}
