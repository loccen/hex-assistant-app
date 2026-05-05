#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

const ACTIVE_PLAYER_URL: &str = "https://127.0.0.1:2999/liveclientdata/activeplayer";
const DEFAULT_TIMEOUT_MS: u64 = 1_500;
const RESPONSE_PREVIEW_CHAR_LIMIT: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivePlayerSnapshot {
    pub champion_name: String,
    pub level: u8,
}

#[derive(Debug)]
pub enum LiveClientError {
    Http {
        url: String,
        status_code: Option<u16>,
        response_body_len: Option<usize>,
        response_body_preview: Option<String>,
        reason: String,
    },
    InvalidPayload {
        url: String,
        response_body_len: usize,
        response_body_preview: String,
        reason: String,
    },
}

impl std::fmt::Display for LiveClientError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http {
                url,
                status_code,
                response_body_len,
                response_body_preview,
                reason,
            } => {
                write!(
                    formatter,
                    "live client 请求失败: url={url}, status_code={status_code:?}, response_body_len={response_body_len:?}, response_body_preview={response_body_preview:?}, reason={reason}"
                )
            }
            Self::InvalidPayload {
                url,
                response_body_len,
                response_body_preview,
                reason,
            } => {
                write!(
                    formatter,
                    "live client 响应无效: url={url}, response_body_len={response_body_len}, response_body_preview={response_body_preview:?}, reason={reason}"
                )
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
            .map_err(|error| LiveClientError::http(
                ACTIVE_PLAYER_URL,
                None,
                None,
                None,
                format!("构建 HTTP 客户端失败: {error}"),
            ))?;

        let response = client
            .get(ACTIVE_PLAYER_URL)
            .send()
            .map_err(|error| LiveClientError::http(
                ACTIVE_PLAYER_URL,
                None,
                None,
                None,
                format!("发送请求失败: {error}"),
            ))?;
        let status = response.status();
        let payload = response.text().map_err(|error| {
            LiveClientError::http(
                ACTIVE_PLAYER_URL,
                Some(status.as_u16()),
                None,
                None,
                format!("读取响应体失败: {error}"),
            )
        })?;
        let response_body_len = payload.len();
        let response_body_preview = summarize_payload(&payload);

        if !status.is_success() {
            return Err(LiveClientError::http(
                ACTIVE_PLAYER_URL,
                Some(status.as_u16()),
                Some(response_body_len),
                Some(response_body_preview),
                format!("HTTP 状态码异常: {status}"),
            ));
        }

        parse_active_player_from(ACTIVE_PLAYER_URL, &payload)
    }
}

pub fn parse_active_player(payload: &str) -> Result<ActivePlayerSnapshot, LiveClientError> {
    parse_active_player_from(ACTIVE_PLAYER_URL, payload)
}

fn parse_active_player_from(
    url: &str,
    payload: &str,
) -> Result<ActivePlayerSnapshot, LiveClientError> {
    let value: Value = serde_json::from_str(payload).map_err(|error| {
        LiveClientError::invalid_payload(
            url,
            payload,
            format!("JSON 解析失败: {error}"),
        )
    })?;
    let object = value.as_object().ok_or_else(|| {
        LiveClientError::invalid_payload(url, payload, "根节点不是 JSON 对象".to_string())
    })?;

    let champion_name = match object.get("championName") {
        Some(Value::String(champion_name)) => {
            let champion_name = champion_name.trim();
            if champion_name.is_empty() {
                return Err(LiveClientError::invalid_payload(
                    url,
                    payload,
                    "championName 为空字符串".to_string(),
                ));
            }
            champion_name.to_string()
        }
        Some(Value::Null) => {
            return Err(LiveClientError::invalid_payload(
                url,
                payload,
                "championName 为空值".to_string(),
            ));
        }
        Some(other) => {
            return Err(LiveClientError::invalid_payload(
                url,
                payload,
                format!("championName 字段类型错误: {}", json_type_name(other)),
            ));
        }
        None => {
            return Err(LiveClientError::invalid_payload(
                url,
                payload,
                "缺少 championName 字段".to_string(),
            ));
        }
    };

    let level = match object.get("level") {
        Some(Value::Number(level)) => level.as_u64().ok_or_else(|| {
            LiveClientError::invalid_payload(url, payload, "level 不是非负整数".to_string())
        })?,
        Some(Value::Null) => {
            return Err(LiveClientError::invalid_payload(
                url,
                payload,
                "level 为空值".to_string(),
            ));
        }
        Some(other) => {
            return Err(LiveClientError::invalid_payload(
                url,
                payload,
                format!("level 字段类型错误: {}", json_type_name(other)),
            ));
        }
        None => {
            return Err(LiveClientError::invalid_payload(
                url,
                payload,
                "缺少 level 字段".to_string(),
            ));
        }
    };
    let level = u8::try_from(level).map_err(|_| {
        LiveClientError::invalid_payload(url, payload, format!("level 超出 u8 范围: {level}"))
    })?;

    Ok(ActivePlayerSnapshot {
        champion_name,
        level,
    })
}

fn summarize_payload(payload: &str) -> String {
    let normalized = payload.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut preview = normalized.chars().take(RESPONSE_PREVIEW_CHAR_LIMIT).collect::<String>();
    if normalized.chars().count() > RESPONSE_PREVIEW_CHAR_LIMIT {
        preview.push_str("...");
    }
    preview
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

impl LiveClientError {
    fn http(
        url: &str,
        status_code: Option<u16>,
        response_body_len: Option<usize>,
        response_body_preview: Option<String>,
        reason: String,
    ) -> Self {
        Self::Http {
            url: url.to_string(),
            status_code,
            response_body_len,
            response_body_preview,
            reason,
        }
    }

    fn invalid_payload(url: &str, payload: &str, reason: String) -> Self {
        Self::InvalidPayload {
            url: url.to_string(),
            response_body_len: payload.len(),
            response_body_preview: summarize_payload(payload),
            reason,
        }
    }

    pub fn error_code(&self) -> &'static str {
        match self {
            Self::Http { .. } => "HEX-LIVE-CLIENT-UNAVAILABLE",
            Self::InvalidPayload { .. } => "HEX-LIVE-CLIENT-PAYLOAD",
        }
    }

    pub fn pause_reason(&self) -> &'static str {
        match self {
            Self::Http { .. } => "LiveClientUnavailable",
            Self::InvalidPayload { .. } => "InvalidLiveClientData",
        }
    }

    pub fn result_category(&self) -> &'static str {
        match self {
            Self::Http { .. } => "http_error",
            Self::InvalidPayload { .. } => "invalid_payload",
        }
    }

    pub fn diagnostic_payload(&self) -> Value {
        match self {
            Self::Http {
                url,
                status_code,
                response_body_len,
                response_body_preview,
                reason,
            } => json!({
                "url": url,
                "statusCode": status_code,
                "responseBodyLen": response_body_len,
                "responseBodyPreview": response_body_preview,
                "reason": reason,
            }),
            Self::InvalidPayload {
                url,
                response_body_len,
                response_body_preview,
                reason,
            } => json!({
                "url": url,
                "responseBodyLen": response_body_len,
                "responseBodyPreview": response_body_preview,
                "reason": reason,
            }),
        }
    }
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

    #[test]
    fn reports_missing_champion_field_reason() {
        let error = parse_active_player(r#"{"level":11}"#).expect_err("应识别缺失字段");
        let message = error.to_string();

        assert!(message.contains("缺少 championName 字段"));
        assert!(message.contains("response_body_len=12"));
    }

    #[test]
    fn reports_json_parse_failure_reason() {
        let error = parse_active_player("{").expect_err("应识别 JSON 解析失败");

        assert!(error.to_string().contains("JSON 解析失败"));
        assert_eq!(error.result_category(), "invalid_payload");
    }
}
