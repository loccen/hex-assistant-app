#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

const ACTIVE_PLAYER_URL: &str = "https://127.0.0.1:2999/liveclientdata/activeplayer";
const ACTIVE_PLAYER_NAME_URL: &str = "https://127.0.0.1:2999/liveclientdata/activeplayername";
const ALL_GAME_DATA_URL: &str = "https://127.0.0.1:2999/liveclientdata/allgamedata";
const PLAYER_LIST_URL: &str = "https://127.0.0.1:2999/liveclientdata/playerlist";
const DEFAULT_TIMEOUT_MS: u64 = 1_500;
const RESPONSE_PREVIEW_CHAR_LIMIT: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivePlayerSnapshot {
    pub champion_name: String,
    pub level: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPlayerSnapshot {
    pub active_player_name: String,
    pub champion_name: String,
    pub level: u8,
    pub game_mode: Option<String>,
    pub game_time: Option<f64>,
    pub source_endpoint: String,
    pub fallback_used: bool,
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

    pub fn fetch_resolved_player_snapshot(
        &self,
    ) -> Result<ResolvedPlayerSnapshot, LiveClientError> {
        let client = self.build_client()?;
        let active_player_name =
            parse_active_player_name(&self.fetch_payload(&client, ACTIVE_PLAYER_NAME_URL)?)?;
        let all_game_data_payload = self.fetch_payload(&client, ALL_GAME_DATA_URL)?;
        let resolved_from_all_game_data =
            parse_resolved_player_from_all_game_data(&active_player_name, &all_game_data_payload);

        match resolved_from_all_game_data {
            Ok(snapshot) => Ok(snapshot),
            Err(primary_error) => {
                let player_list_payload = self.fetch_payload(&client, PLAYER_LIST_URL)?;
                parse_resolved_player_from_player_list(
                    &active_player_name,
                    &all_game_data_payload,
                    &player_list_payload,
                )
                .map_err(|fallback_error| merge_resolution_error(primary_error, fallback_error))
            }
        }
    }

    pub fn fetch_active_player(&self) -> Result<ActivePlayerSnapshot, LiveClientError> {
        let client = self.build_client()?;
        let payload = self.fetch_payload(&client, ACTIVE_PLAYER_URL)?;
        parse_active_player_from(ACTIVE_PLAYER_URL, &payload)
    }

    fn build_client(&self) -> Result<reqwest::blocking::Client, LiveClientError> {
        reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .danger_accept_invalid_certs(true)
            .no_proxy()
            .build()
            .map_err(|error| {
                LiveClientError::http(
                    ACTIVE_PLAYER_URL,
                    None,
                    None,
                    None,
                    format!("构建 HTTP 客户端失败: {error}"),
                )
            })
    }

    fn fetch_payload(
        &self,
        client: &reqwest::blocking::Client,
        url: &str,
    ) -> Result<String, LiveClientError> {
        let response = client.get(url).send().map_err(|error| {
            LiveClientError::http(url, None, None, None, format!("发送请求失败: {error}"))
        })?;
        let status = response.status();
        let payload = response.text().map_err(|error| {
            LiveClientError::http(
                url,
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
                url,
                Some(status.as_u16()),
                Some(response_body_len),
                Some(response_body_preview),
                format!("HTTP 状态码异常: {status}"),
            ));
        }

        Ok(payload)
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
        LiveClientError::invalid_payload(url, payload, format!("JSON 解析失败: {error}"))
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
    let mut preview = normalized
        .chars()
        .take(RESPONSE_PREVIEW_CHAR_LIMIT)
        .collect::<String>();
    if normalized.chars().count() > RESPONSE_PREVIEW_CHAR_LIMIT {
        preview.push_str("...");
    }
    preview
}

fn parse_active_player_name(payload: &str) -> Result<String, LiveClientError> {
    match serde_json::from_str::<Value>(payload).map_err(|error| {
        LiveClientError::invalid_payload(
            ACTIVE_PLAYER_NAME_URL,
            payload,
            format!("JSON 解析失败: {error}"),
        )
    })? {
        Value::String(value) => {
            let value = value.trim();
            if value.is_empty() {
                Err(LiveClientError::invalid_payload(
                    ACTIVE_PLAYER_NAME_URL,
                    payload,
                    "activePlayerName 为空字符串".to_string(),
                ))
            } else {
                Ok(value.to_string())
            }
        }
        other => Err(LiveClientError::invalid_payload(
            ACTIVE_PLAYER_NAME_URL,
            payload,
            format!(
                "activePlayerName 根节点类型错误: {}",
                json_type_name(&other)
            ),
        )),
    }
}

fn parse_resolved_player_from_all_game_data(
    active_player_name: &str,
    payload: &str,
) -> Result<ResolvedPlayerSnapshot, LiveClientError> {
    let value = parse_json_object(ALL_GAME_DATA_URL, payload)?;
    let all_players = value
        .get("allPlayers")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            LiveClientError::invalid_payload(
                ALL_GAME_DATA_URL,
                payload,
                "allgamedata 缺少 allPlayers 数组".to_string(),
            )
        })?;
    let matched_player =
        match_player_by_name(all_players, active_player_name).ok_or_else(|| {
            LiveClientError::invalid_payload(
                ALL_GAME_DATA_URL,
                payload,
                format!("allgamedata 未找到当前玩家: {active_player_name}"),
            )
        })?;
    let champion_name =
        extract_non_empty_string(matched_player, "championName", ALL_GAME_DATA_URL, payload)?;
    let level = extract_optional_u8(matched_player.get("level"), ALL_GAME_DATA_URL, payload)?
        .or_else(|| {
            value
                .get("activePlayer")
                .and_then(Value::as_object)
                .and_then(|player| {
                    extract_optional_u8(player.get("level"), ALL_GAME_DATA_URL, payload).ok()
                })
                .flatten()
        })
        .ok_or_else(|| {
            LiveClientError::invalid_payload(
                ALL_GAME_DATA_URL,
                payload,
                "allgamedata 未提供当前玩家等级".to_string(),
            )
        })?;

    Ok(ResolvedPlayerSnapshot {
        active_player_name: active_player_name.to_string(),
        champion_name,
        level,
        game_mode: extract_optional_trimmed_string(
            value.get("gameData"),
            "gameMode",
            ALL_GAME_DATA_URL,
            payload,
        )?,
        game_time: extract_optional_f64_from_object(
            value.get("gameData"),
            "gameTime",
            ALL_GAME_DATA_URL,
            payload,
        )?,
        source_endpoint: ALL_GAME_DATA_URL.to_string(),
        fallback_used: false,
    })
}

fn parse_resolved_player_from_player_list(
    active_player_name: &str,
    all_game_data_payload: &str,
    player_list_payload: &str,
) -> Result<ResolvedPlayerSnapshot, LiveClientError> {
    let all_game_data = parse_json_object(ALL_GAME_DATA_URL, all_game_data_payload)?;
    let player_list = serde_json::from_str::<Value>(player_list_payload).map_err(|error| {
        LiveClientError::invalid_payload(
            PLAYER_LIST_URL,
            player_list_payload,
            format!("JSON 解析失败: {error}"),
        )
    })?;
    let players = player_list.as_array().ok_or_else(|| {
        LiveClientError::invalid_payload(
            PLAYER_LIST_URL,
            player_list_payload,
            "playerlist 根节点不是数组".to_string(),
        )
    })?;
    let matched_player = match_player_by_name(players, active_player_name).ok_or_else(|| {
        LiveClientError::invalid_payload(
            PLAYER_LIST_URL,
            player_list_payload,
            format!("playerlist 未找到当前玩家: {active_player_name}"),
        )
    })?;
    let champion_name = extract_non_empty_string(
        matched_player,
        "championName",
        PLAYER_LIST_URL,
        player_list_payload,
    )?;
    let level = extract_optional_u8(
        matched_player.get("level"),
        PLAYER_LIST_URL,
        player_list_payload,
    )?
    .or_else(|| {
        all_game_data
            .get("activePlayer")
            .and_then(Value::as_object)
            .and_then(|player| {
                extract_optional_u8(
                    player.get("level"),
                    ALL_GAME_DATA_URL,
                    all_game_data_payload,
                )
                .ok()
            })
            .flatten()
    })
    .ok_or_else(|| {
        LiveClientError::invalid_payload(
            PLAYER_LIST_URL,
            player_list_payload,
            "playerlist 兜底时未能解析等级".to_string(),
        )
    })?;

    Ok(ResolvedPlayerSnapshot {
        active_player_name: active_player_name.to_string(),
        champion_name,
        level,
        game_mode: extract_optional_trimmed_string(
            all_game_data.get("gameData"),
            "gameMode",
            ALL_GAME_DATA_URL,
            all_game_data_payload,
        )?,
        game_time: extract_optional_f64_from_object(
            all_game_data.get("gameData"),
            "gameTime",
            ALL_GAME_DATA_URL,
            all_game_data_payload,
        )?,
        source_endpoint: PLAYER_LIST_URL.to_string(),
        fallback_used: true,
    })
}

fn parse_json_object(
    url: &str,
    payload: &str,
) -> Result<serde_json::Map<String, Value>, LiveClientError> {
    let value: Value = serde_json::from_str(payload).map_err(|error| {
        LiveClientError::invalid_payload(url, payload, format!("JSON 解析失败: {error}"))
    })?;
    value.as_object().cloned().ok_or_else(|| {
        LiveClientError::invalid_payload(url, payload, "根节点不是 JSON 对象".to_string())
    })
}

fn match_player_by_name<'a>(
    players: &'a [Value],
    active_player_name: &str,
) -> Option<&'a serde_json::Map<String, Value>> {
    let normalized_target = normalize_player_name(active_player_name);
    players.iter().find_map(|player| {
        let object = player.as_object()?;
        let candidate_fields = [
            object.get("summonerName"),
            object.get("riotId"),
            object.get("riotIdGameName"),
            object.get("displayName"),
            object.get("gameName"),
        ];

        let matched = candidate_fields.into_iter().flatten().any(|value| {
            value.as_str().map(normalize_player_name).as_deref() == Some(normalized_target.as_str())
        });

        matched.then_some(object)
    })
}

fn normalize_player_name(value: &str) -> String {
    value
        .split('#')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase()
}

fn extract_non_empty_string(
    object: &serde_json::Map<String, Value>,
    field: &str,
    url: &str,
    payload: &str,
) -> Result<String, LiveClientError> {
    match object.get(field) {
        Some(Value::String(value)) => {
            let value = value.trim();
            if value.is_empty() {
                Err(LiveClientError::invalid_payload(
                    url,
                    payload,
                    format!("{field} 为空字符串"),
                ))
            } else {
                Ok(value.to_string())
            }
        }
        Some(Value::Null) => Err(LiveClientError::invalid_payload(
            url,
            payload,
            format!("{field} 为空值"),
        )),
        Some(other) => Err(LiveClientError::invalid_payload(
            url,
            payload,
            format!("{field} 字段类型错误: {}", json_type_name(other)),
        )),
        None => Err(LiveClientError::invalid_payload(
            url,
            payload,
            format!("缺少 {field} 字段"),
        )),
    }
}

fn extract_optional_trimmed_string(
    object: Option<&Value>,
    field: &str,
    url: &str,
    payload: &str,
) -> Result<Option<String>, LiveClientError> {
    let Some(object) = object.and_then(Value::as_object) else {
        return Ok(None);
    };
    match object.get(field) {
        Some(Value::String(value)) => {
            let value = value.trim();
            if value.is_empty() {
                Ok(None)
            } else {
                Ok(Some(value.to_string()))
            }
        }
        Some(Value::Null) | None => Ok(None),
        Some(other) => Err(LiveClientError::invalid_payload(
            url,
            payload,
            format!("{field} 字段类型错误: {}", json_type_name(other)),
        )),
    }
}

fn extract_optional_f64_from_object(
    object: Option<&Value>,
    field: &str,
    url: &str,
    payload: &str,
) -> Result<Option<f64>, LiveClientError> {
    let Some(object) = object.and_then(Value::as_object) else {
        return Ok(None);
    };
    match object.get(field) {
        Some(Value::Number(value)) => value.as_f64().map(Some).ok_or_else(|| {
            LiveClientError::invalid_payload(url, payload, format!("{field} 不是数字"))
        }),
        Some(Value::Null) | None => Ok(None),
        Some(other) => Err(LiveClientError::invalid_payload(
            url,
            payload,
            format!("{field} 字段类型错误: {}", json_type_name(other)),
        )),
    }
}

fn extract_optional_u8(
    value: Option<&Value>,
    url: &str,
    payload: &str,
) -> Result<Option<u8>, LiveClientError> {
    match value {
        Some(Value::Number(level)) => {
            let raw = level.as_u64().ok_or_else(|| {
                LiveClientError::invalid_payload(url, payload, "level 不是非负整数".to_string())
            })?;
            let value = u8::try_from(raw).map_err(|_| {
                LiveClientError::invalid_payload(url, payload, format!("level 超出 u8 范围: {raw}"))
            })?;
            Ok(Some(value))
        }
        Some(Value::Null) | None => Ok(None),
        Some(other) => Err(LiveClientError::invalid_payload(
            url,
            payload,
            format!("level 字段类型错误: {}", json_type_name(other)),
        )),
    }
}

fn merge_resolution_error(
    primary_error: LiveClientError,
    fallback_error: LiveClientError,
) -> LiveClientError {
    match fallback_error {
        LiveClientError::InvalidPayload {
            url,
            response_body_len,
            response_body_preview,
            reason,
        } => LiveClientError::InvalidPayload {
            url,
            response_body_len,
            response_body_preview,
            reason: format!("主路径失败: {}; 兜底失败: {reason}", primary_error),
        },
        other => other,
    }
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

    #[test]
    fn resolves_player_from_all_game_data() {
        let snapshot = parse_resolved_player_from_all_game_data(
            "loccen#65238",
            r#"{
                "activePlayer": {
                    "level": 11
                },
                "allPlayers": [
                    {
                        "summonerName": "loccen",
                        "championName": "Ahri"
                    }
                ],
                "gameData": {
                    "gameMode": "KIWI",
                    "gameTime": 321.5
                }
            }"#,
        )
        .expect("allgamedata 主路径应能解析");

        assert_eq!(
            snapshot,
            ResolvedPlayerSnapshot {
                active_player_name: "loccen#65238".to_string(),
                champion_name: "Ahri".to_string(),
                level: 11,
                game_mode: Some("KIWI".to_string()),
                game_time: Some(321.5),
                source_endpoint: ALL_GAME_DATA_URL.to_string(),
                fallback_used: false,
            }
        );
    }

    #[test]
    fn resolves_player_from_player_list_fallback() {
        let snapshot = parse_resolved_player_from_player_list(
            "loccen#65238",
            r#"{
                "activePlayer": {
                    "level": 7
                },
                "gameData": {
                    "gameMode": "KIWI",
                    "gameTime": 88.0
                }
            }"#,
            r#"[
                {
                    "summonerName": "loccen",
                    "championName": "Ekko"
                }
            ]"#,
        )
        .expect("playerlist 兜底应能解析");

        assert_eq!(snapshot.champion_name, "Ekko");
        assert_eq!(snapshot.level, 7);
        assert_eq!(snapshot.game_mode.as_deref(), Some("KIWI"));
        assert_eq!(snapshot.source_endpoint, PLAYER_LIST_URL);
        assert!(snapshot.fallback_used);
    }
}
