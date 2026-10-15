// Copyright 2026 Aventus DAO Ltd

//! The node's own country, for display in watchtower vote and oracle report events:
//! `GET /node/country`.
//!
//! Resolved once from the node's public address through a geo-IP lookup and cached for the
//! life of the process. It is self-reported and reflects the egress address (a VPN or a cloud
//! load balancer shows the provider's location), so nothing may depend on it. A failed lookup
//! answers [`UNKNOWN_COUNTRY`], and so does every request for the next [`RETRY_AFTER`]: votes
//! ask for the country before signing, and a node that cannot reach the geo-IP service must not
//! make each of them wait for the lookup to time out again. `AVN_NODE_COUNTRY` in the
//! environment overrides the lookup, which local networks use so each node shows a different
//! flag.

use alloy::transports::http::reqwest;
use anyhow::{anyhow, Context, Result};
use sp_watchtower::{CountryCode, UNKNOWN_COUNTRY};
use std::{
    future::Future,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

pub const GEOIP_URL: &str = "https://ipinfo.io/json";
pub const OVERRIDE_ENV: &str = "AVN_NODE_COUNTRY";
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);
/// A lookup still marked in flight after this is taken as abandoned: the lookup itself times
/// out well before.
const IN_FLIGHT_EXPIRY: Duration = Duration::from_secs(30);
/// How long a failed lookup is remembered before the next request tries again.
pub const RETRY_AFTER: Duration = Duration::from_secs(10 * 60);

#[derive(Default)]
pub struct NodeCountry {
    state: Mutex<State>,
}

#[derive(Default)]
enum State {
    #[default]
    Unresolved,
    /// A lookup started at this moment. Callers meanwhile answer unknown rather than wait for
    /// it. If the lookup's future is dropped (a client disconnecting drops its handler), nothing
    /// resets this, so it is treated as abandoned once older than [`IN_FLIGHT_EXPIRY`].
    InFlight(Instant),
    Resolved(CountryCode),
    /// The last lookup failed at this moment.
    Failed(Instant),
}

impl NodeCountry {
    /// The cached code, or a fresh lookup. Never fails: unknown is a valid answer.
    pub async fn get(&self) -> CountryCode {
        self.get_with(Instant::now(), resolve).await
    }

    /// [`NodeCountry::get`] with the clock and the lookup supplied, for tests.
    async fn get_with<F, Fut>(&self, now: Instant, lookup: F) -> CountryCode
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<CountryCode>>,
    {
        {
            // The lock is only held to read and mark the state, never across the lookup.
            let mut state = self.state.lock().await;
            match *state {
                State::Resolved(country) => return country,
                State::InFlight(since) if now.duration_since(since) < IN_FLIGHT_EXPIRY =>
                    return UNKNOWN_COUNTRY,
                State::Failed(at) if now.duration_since(at) < RETRY_AFTER => return UNKNOWN_COUNTRY,
                _ => *state = State::InFlight(now),
            }
        }
        let result = lookup().await;
        let mut state = self.state.lock().await;
        match result {
            Ok(country) => {
                log::info!("node country: {}", String::from_utf8_lossy(&country));
                *state = State::Resolved(country);
                country
            },
            Err(e) => {
                log::warn!(
                    "node country: lookup failed, reporting unknown for the next {}s: {e:#}",
                    RETRY_AFTER.as_secs()
                );
                *state = State::Failed(now);
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

    #[tokio::test]
    async fn a_failed_lookup_is_retried_only_after_a_while() {
        let node = NodeCountry::default();
        let start = Instant::now();
        let fail = || async { Err(anyhow!("unreachable")) };
        let never = || -> std::future::Ready<Result<CountryCode>> {
            panic!("must not look up while the result is remembered")
        };

        assert_eq!(node.get_with(start, fail).await, UNKNOWN_COUNTRY);
        let soon = start + RETRY_AFTER - Duration::from_secs(1);
        assert_eq!(node.get_with(soon, never).await, UNKNOWN_COUNTRY);

        let later = start + RETRY_AFTER;
        assert_eq!(node.get_with(later, || async { Ok(*b"GB") }).await, *b"GB");
        assert_eq!(node.get_with(later, never).await, *b"GB", "a success is kept for good");
    }

    #[tokio::test]
    async fn callers_do_not_wait_for_a_lookup_in_flight() {
        let node = NodeCountry::default();
        let now = Instant::now();
        let (release, released) = tokio::sync::oneshot::channel::<()>();
        let never = || -> std::future::Ready<Result<CountryCode>> {
            panic!("a second lookup must not start while one is in flight")
        };

        let slow = node.get_with(now, || async move {
            let _ = released.await;
            Ok(*b"GB")
        });
        let other = async {
            // Let the slow lookup start first.
            tokio::task::yield_now().await;
            let answer = node.get_with(now, never).await;
            release.send(()).unwrap();
            answer
        };
        let (first, second) = tokio::join!(slow, other);
        assert_eq!(first, *b"GB");
        assert_eq!(second, UNKNOWN_COUNTRY, "answered at once, not after the lookup");
        assert_eq!(node.get_with(now, never).await, *b"GB");
    }

    /// A lookup whose future was dropped must not leave the country unknown for good.
    #[tokio::test]
    async fn an_abandoned_lookup_is_retried() {
        let node = NodeCountry::default();
        let start = Instant::now();
        let never = || -> std::future::Ready<Result<CountryCode>> {
            panic!("must not look up while the lookup still counts as in flight")
        };

        // Start a lookup and drop it before it completes.
        {
            let pending = node.get_with(start, std::future::pending::<Result<CountryCode>>);
            futures::pin_mut!(pending);
            assert!(futures::poll!(pending.as_mut()).is_pending());
        }
        assert_eq!(node.get_with(start, never).await, UNKNOWN_COUNTRY, "still in flight");

        let later = start + IN_FLIGHT_EXPIRY;
        assert_eq!(node.get_with(later, || async { Ok(*b"GB") }).await, *b"GB");
    }
}
