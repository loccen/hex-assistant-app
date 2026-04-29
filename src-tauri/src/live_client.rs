#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::time::Duration;

const ACTIVE_PLAYER_URL: &str = "https://127.0.0.1:2999/liveclientdata/activeplayer";
const DEFAULT_TIMEOUT_MS: u64 = 1_500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivePlayerSnapshot {
    pub champion_name: String,
    pub level: u8,
}

#[derive(Debug)]
pub enum LiveClientError {
    Http(String),
    InvalidPayload(String),
}

impl std::fmt::Display for LiveClientError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(message) => write!(formatter, "live client request failed: {message}"),
            Self::InvalidPayload(message) => {
                write!(formatter, "live client payload is invalid: {message}")
            }
        }
    }
}

impl std::error::Error for LiveClientError {}

#[derive(Debug, Clone)]
pub struct LiveClientDataApi {
    timeout: Duration,
}

impl Default for LiveClientDataApi {
    fn default() -> Self {
        Self {
            timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS),
        }
    }
}

impl LiveClientDataApi {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn active_player_url(&self) -> &'static str {
        ACTIVE_PLAYER_URL
    }

    pub fn fetch_active_player(&self) -> Result<ActivePlayerSnapshot, LiveClientError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .danger_accept_invalid_certs(true)
            .no_proxy()
            .build()
            .map_err(|error| LiveClientError::Http(error.to_string()))?;

        let payload = client
            .get(ACTIVE_PLAYER_URL)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|error| LiveClientError::Http(error.to_string()))?
            .text()
            .map_err(|error| LiveClientError::Http(error.to_string()))?;

        parse_active_player(&payload)
    }
}

pub fn parse_active_player(payload: &str) -> Result<ActivePlayerSnapshot, LiveClientError> {
    let response: ActivePlayerResponse = serde_json::from_str(payload)
        .map_err(|error| LiveClientError::InvalidPayload(error.to_string()))?;

    if response.champion_name.trim().is_empty() {
        return Err(LiveClientError::InvalidPayload(
            "championName must not be empty".to_string(),
        ));
    }

    Ok(ActivePlayerSnapshot {
        champion_name: response.champion_name,
        level: response.level,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActivePlayerResponse {
    champion_name: String,
    level: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_active_player_champion_and_level() {
        let snapshot = parse_active_player(
            r#"{
                "summonerName": "local player",
                "championName": "Ahri",
                "level": 11,
                "currentGold": 999
            }"#,
        )
        .expect("active player payload should parse");

        assert_eq!(
            snapshot,
            ActivePlayerSnapshot {
                champion_name: "Ahri".to_string(),
                level: 11,
            }
        );
    }

    #[test]
    fn uses_fixed_local_active_player_endpoint() {
        assert_eq!(
            LiveClientDataApi::new().active_player_url(),
            "https://127.0.0.1:2999/liveclientdata/activeplayer"
        );
    }
}
