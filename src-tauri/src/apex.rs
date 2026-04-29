use chrono::{DateTime, Duration, Utc};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration as StdDuration, Instant};

const CACHE_VERSION: u32 = 1;
const INDEX_URLS: [&str; 2] = [
    "https://apexlol.info/zh/hextech/",
    "https://apexlol.info/en/hextech/",
];
pub const APEX_SOURCE_NAME: &str = "ApexLOL";
pub const NO_DATA_TEXT: &str = "暂无数据";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApexLookupSettings {
    pub cache_ttl_hours: u64,
    pub request_timeout_ms: u64,
    pub failed_cache_ttl_minutes: u64,
}

impl Default for ApexLookupSettings {
    fn default() -> Self {
        Self {
            cache_ttl_hours: 168,
            request_timeout_ms: 6000,
            failed_cache_ttl_minutes: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApexLookupRequest {
    pub champion_name: String,
    pub augment_name: String,
    #[serde(default)]
    pub force_refresh: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ApexParseStatus {
    Ok,
    NoData,
    RequestFailed,
    ParseFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApexLookupResult {
    pub cache_key: String,
    pub champion_name: String,
    pub augment_name: String,
    pub rating: Option<String>,
    pub summary: String,
    pub tip: Option<String>,
    pub source: String,
    pub source_url: String,
    pub fetched_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub cache_hit: bool,
    pub status: ApexParseStatus,
    pub error: Option<String>,
    pub request_log: ApexRequestLog,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApexRequestLog {
    pub request_url: String,
    pub duration_ms: u128,
    pub cache_hit: bool,
    pub parse_status: ApexParseStatus,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApexCacheFile {
    pub version: u32,
    pub entries: BTreeMap<String, ApexCacheEntry>,
}

impl Default for ApexCacheFile {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApexCacheEntry {
    pub champion_name: String,
    pub augment_name: String,
    pub rating: Option<String>,
    pub summary: String,
    pub tip: Option<String>,
    pub source: String,
    pub source_url: String,
    pub fetched_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub cache_hit: bool,
    pub status: ApexParseStatus,
    pub error: Option<String>,
    pub request_url: String,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApexCacheReport {
    pub cache_path: PathBuf,
    pub report_path: Option<PathBuf>,
    pub generated_at: DateTime<Utc>,
    pub total_entries: usize,
    pub ok_entries: usize,
    pub failed_entries: usize,
    pub expired_entries: usize,
    pub entries: Vec<ApexCacheReportEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApexCacheReportEntry {
    pub cache_key: String,
    pub champion_name: String,
    pub augment_name: String,
    pub rating: Option<String>,
    pub summary: String,
    pub tip: Option<String>,
    pub source: String,
    pub source_url: String,
    pub request_url: String,
    pub fetched_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub expired: bool,
    pub duration_ms: u128,
    pub status: ApexParseStatus,
    pub error: Option<String>,
}

pub fn lookup_with_cache(
    cache_dir: impl AsRef<Path>,
    request: ApexLookupRequest,
    settings: ApexLookupSettings,
) -> Result<ApexLookupResult, String> {
    let cache_path = cache_path(cache_dir.as_ref());
    let cache_key = cache_key(&request.champion_name, &request.augment_name);
    let mut cache = read_cache(&cache_path)?;

    if !request.force_refresh {
        if let Some(entry) = cache.entries.get(&cache_key) {
            if entry.expires_at > Utc::now() {
                return Ok(result_from_cache_entry(entry));
            }
        }
    }

    let mut fetched = fetch_and_parse(&request, &settings);
    let entry = cache_entry_from_result(&fetched, &settings);
    fetched.expires_at = entry.expires_at;
    cache.entries.insert(cache_key, entry);
    write_cache(&cache_path, &cache)?;
    Ok(fetched)
}

pub fn build_cache_report(cache_dir: impl AsRef<Path>) -> Result<ApexCacheReport, String> {
    let cache_path = cache_path(cache_dir.as_ref());
    let cache = read_cache(&cache_path)?;
    let now = Utc::now();
    let entries: Vec<ApexCacheReportEntry> = cache
        .entries
        .into_iter()
        .map(|(cache_key, entry)| ApexCacheReportEntry {
            cache_key,
            champion_name: entry.champion_name,
            augment_name: entry.augment_name,
            rating: entry.rating,
            summary: entry.summary,
            tip: entry.tip,
            source: entry.source,
            source_url: entry.source_url,
            request_url: entry.request_url,
            fetched_at: entry.fetched_at,
            expires_at: entry.expires_at,
            expired: entry.expires_at <= now,
            duration_ms: entry.duration_ms,
            status: entry.status,
            error: entry.error,
        })
        .collect();

    Ok(ApexCacheReport {
        cache_path,
        report_path: None,
        generated_at: now,
        total_entries: entries.len(),
        ok_entries: entries
            .iter()
            .filter(|entry| entry.status == ApexParseStatus::Ok)
            .count(),
        failed_entries: entries
            .iter()
            .filter(|entry| entry.status != ApexParseStatus::Ok)
            .count(),
        expired_entries: entries.iter().filter(|entry| entry.expired).count(),
        entries,
    })
}

pub fn build_and_write_cache_report(
    cache_dir: impl AsRef<Path>,
    reports_dir: impl AsRef<Path>,
) -> Result<ApexCacheReport, String> {
    let mut report = build_cache_report(cache_dir)?;
    fs::create_dir_all(reports_dir.as_ref()).map_err(|error| {
        format!(
            "无法创建 ApexLOL 缓存报告目录 {}: {error}",
            reports_dir.as_ref().display()
        )
    })?;
    let report_path = reports_dir.as_ref().join(format!(
        "apex-cache-report-{}.json",
        Utc::now().format("%Y%m%d-%H%M%S")
    ));
    report.report_path = Some(report_path.clone());
    let content = serde_json::to_string_pretty(&report)
        .map_err(|error| format!("无法序列化 ApexLOL 缓存报告: {error}"))?;
    fs::write(&report_path, format!("{content}\n")).map_err(|error| {
        format!(
            "无法写入 ApexLOL 缓存报告 {}: {error}",
            report_path.display()
        )
    })?;
    Ok(report)
}

pub fn parse_apex_detail_page(
    html: &str,
    source_url: &str,
    champion_name: &str,
    augment_name: &str,
    request_url: &str,
    duration_ms: u128,
) -> ApexLookupResult {
    let document = Html::parse_document(html);
    let text = visible_text(&document);
    let now = Utc::now();

    let Some(rating) = parse_champion_rating(&text, champion_name) else {
        return no_data_result(
            champion_name,
            augment_name,
            source_url,
            request_url,
            duration_ms,
            ApexParseStatus::ParseFailed,
            "未在来源页面中解析到当前英雄与海克斯的联动评级",
        );
    };

    ApexLookupResult {
        cache_key: cache_key(champion_name, augment_name),
        champion_name: champion_name.to_string(),
        augment_name: augment_name.to_string(),
        rating: Some(rating.rating),
        summary: rating.summary.unwrap_or_else(|| NO_DATA_TEXT.to_string()),
        tip: parse_effect_description(&text),
        source: APEX_SOURCE_NAME.to_string(),
        source_url: source_url.to_string(),
        fetched_at: now,
        expires_at: now,
        cache_hit: false,
        status: ApexParseStatus::Ok,
        error: None,
        request_log: ApexRequestLog {
            request_url: request_url.to_string(),
            duration_ms,
            cache_hit: false,
            parse_status: ApexParseStatus::Ok,
            failure_reason: None,
        },
    }
}

fn fetch_and_parse(request: &ApexLookupRequest, settings: &ApexLookupSettings) -> ApexLookupResult {
    let started = Instant::now();
    let timeout = StdDuration::from_millis(settings.request_timeout_ms);

    match find_augment_source_url(&request.augment_name, timeout) {
        Ok(source_url) => {
            let request_url = source_url.clone();
            match http_get(&source_url, timeout) {
                Ok(html) => parse_apex_detail_page(
                    &html,
                    &source_url,
                    &request.champion_name,
                    &request.augment_name,
                    &request_url,
                    started.elapsed().as_millis(),
                ),
                Err(error) => no_data_result(
                    &request.champion_name,
                    &request.augment_name,
                    &source_url,
                    &request_url,
                    started.elapsed().as_millis(),
                    ApexParseStatus::RequestFailed,
                    &error,
                ),
            }
        }
        Err(error) => no_data_result(
            &request.champion_name,
            &request.augment_name,
            INDEX_URLS[0],
            INDEX_URLS[0],
            started.elapsed().as_millis(),
            error.status,
            &error.message,
        ),
    }
}

fn find_augment_source_url(
    augment_name: &str,
    timeout: StdDuration,
) -> Result<String, ApexSourceLookupError> {
    let mut request_failures = Vec::new();
    let mut parsed_any_index = false;

    for index_url in INDEX_URLS {
        let html = match http_get(index_url, timeout) {
            Ok(html) => html,
            Err(error) => {
                request_failures.push(format!("{index_url}: {error}"));
                continue;
            }
        };
        parsed_any_index = true;
        if let Some(url) = find_augment_link(index_url, &html, augment_name) {
            return Ok(url);
        }
    }

    if parsed_any_index {
        Err(ApexSourceLookupError {
            status: ApexParseStatus::NoData,
            message: format!("未在 ApexLOL 海克斯索引中找到「{augment_name}」"),
        })
    } else {
        Err(ApexSourceLookupError {
            status: ApexParseStatus::RequestFailed,
            message: format!(
                "ApexLOL 海克斯索引请求失败: {}",
                request_failures.join("；")
            ),
        })
    }
}

fn find_augment_link(index_url: &str, html: &str, augment_name: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("a").ok()?;
    let target = normalize_lookup_text(augment_name);

    document.select(&selector).find_map(|element| {
        let label = normalize_lookup_text(&element.text().collect::<Vec<_>>().join(""));
        if label == target {
            element
                .value()
                .attr("href")
                .map(|href| absolute_url(index_url, href))
        } else {
            None
        }
    })
}

fn http_get(url: &str, timeout: StdDuration) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .user_agent("hex-assistant-app/0.1")
        .build()
        .map_err(|error| format!("无法创建 ApexLOL 请求客户端: {error}"))?;
    let response = client
        .get(url)
        .send()
        .map_err(|error| format!("请求 ApexLOL 失败: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("ApexLOL 返回 HTTP {status}"));
    }
    response
        .text()
        .map_err(|error| format!("读取 ApexLOL 响应失败: {error}"))
}

fn visible_text(document: &Html) -> Vec<String> {
    let body_selector = Selector::parse("body").expect("body selector 应合法");
    document
        .select(&body_selector)
        .flat_map(|body| body.text())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn parse_effect_description(text: &[String]) -> Option<String> {
    text.windows(2)
        .find(|pair| pair[0].contains("效果描述") || pair[0].contains("Description"))
        .and_then(|pair| non_noise_text(&pair[1]))
}

fn parse_champion_rating(text: &[String], champion_name: &str) -> Option<ParsedChampionRating> {
    let target = normalize_lookup_text(champion_name);
    let rating_values = ["SS", "S", "A", "B", "C", "D"];

    for (index, item) in text.iter().enumerate() {
        if normalize_lookup_text(item) != target {
            continue;
        }
        let rating_index = text
            .iter()
            .enumerate()
            .skip(index + 1)
            .take(4)
            .find(|(_, value)| rating_values.contains(&value.trim()))?
            .0;
        let summary = text
            .iter()
            .skip(rating_index + 1)
            .take(5)
            .find_map(|value| non_noise_text(value));
        return Some(ParsedChampionRating {
            rating: text[rating_index].trim().to_string(),
            summary,
        });
    }

    None
}

fn non_noise_text(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value == "0"
        || value == "投稿"
        || value == "Contribute"
        || value.contains("Image:")
        || value.contains("Assist Me")
        || value.contains("Enemy Missing")
    {
        None
    } else {
        Some(value.to_string())
    }
}

fn no_data_result(
    champion_name: &str,
    augment_name: &str,
    source_url: &str,
    request_url: &str,
    duration_ms: u128,
    status: ApexParseStatus,
    reason: &str,
) -> ApexLookupResult {
    ApexLookupResult {
        cache_key: cache_key(champion_name, augment_name),
        champion_name: champion_name.to_string(),
        augment_name: augment_name.to_string(),
        rating: None,
        summary: NO_DATA_TEXT.to_string(),
        tip: None,
        source: APEX_SOURCE_NAME.to_string(),
        source_url: source_url.to_string(),
        fetched_at: Utc::now(),
        expires_at: Utc::now(),
        cache_hit: false,
        status: status.clone(),
        error: Some(reason.to_string()),
        request_log: ApexRequestLog {
            request_url: request_url.to_string(),
            duration_ms,
            cache_hit: false,
            parse_status: status,
            failure_reason: Some(reason.to_string()),
        },
    }
}

fn result_from_cache_entry(entry: &ApexCacheEntry) -> ApexLookupResult {
    ApexLookupResult {
        cache_key: cache_key(&entry.champion_name, &entry.augment_name),
        champion_name: entry.champion_name.clone(),
        augment_name: entry.augment_name.clone(),
        rating: entry.rating.clone(),
        summary: entry.summary.clone(),
        tip: entry.tip.clone(),
        source: entry.source.clone(),
        source_url: entry.source_url.clone(),
        fetched_at: entry.fetched_at,
        expires_at: entry.expires_at,
        cache_hit: true,
        status: entry.status.clone(),
        error: entry.error.clone(),
        request_log: ApexRequestLog {
            request_url: entry.request_url.clone(),
            duration_ms: entry.duration_ms,
            cache_hit: true,
            parse_status: entry.status.clone(),
            failure_reason: entry.error.clone(),
        },
    }
}

fn cache_entry_from_result(
    result: &ApexLookupResult,
    settings: &ApexLookupSettings,
) -> ApexCacheEntry {
    let ttl = if result.status == ApexParseStatus::Ok {
        Duration::hours(settings.cache_ttl_hours as i64)
    } else {
        Duration::minutes(settings.failed_cache_ttl_minutes as i64)
    };

    ApexCacheEntry {
        champion_name: result.champion_name.clone(),
        augment_name: result.augment_name.clone(),
        rating: result.rating.clone(),
        summary: result.summary.clone(),
        tip: result.tip.clone(),
        source: result.source.clone(),
        source_url: result.source_url.clone(),
        fetched_at: result.fetched_at,
        expires_at: result.fetched_at + ttl,
        cache_hit: false,
        status: result.status.clone(),
        error: result.error.clone(),
        request_url: result.request_log.request_url.clone(),
        duration_ms: result.request_log.duration_ms,
    }
}

fn read_cache(cache_path: &Path) -> Result<ApexCacheFile, String> {
    if !cache_path.exists() {
        return Ok(ApexCacheFile::default());
    }

    let content = fs::read_to_string(cache_path)
        .map_err(|error| format!("无法读取 ApexLOL 缓存 {}: {error}", cache_path.display()))?;
    serde_json::from_str(&content)
        .map_err(|error| format!("无法解析 ApexLOL 缓存 {}: {error}", cache_path.display()))
}

fn write_cache(cache_path: &Path, cache: &ApexCacheFile) -> Result<(), String> {
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建 ApexLOL 缓存目录 {}: {error}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(cache)
        .map_err(|error| format!("无法序列化 ApexLOL 缓存: {error}"))?;
    fs::write(cache_path, format!("{content}\n"))
        .map_err(|error| format!("无法写入 ApexLOL 缓存 {}: {error}", cache_path.display()))
}

fn cache_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("apex-cache").join("cache.json")
}

fn cache_key(champion_name: &str, augment_name: &str) -> String {
    format!(
        "{}::{}",
        normalize_lookup_text(champion_name),
        normalize_lookup_text(augment_name)
    )
}

fn normalize_lookup_text(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '\'' && *ch != '’')
        .flat_map(char::to_lowercase)
        .collect()
}

fn absolute_url(base_url: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }

    let base = base_url.trim_end_matches('/');
    if href.starts_with('/') {
        format!("https://apexlol.info{href}")
    } else {
        format!("{base}/{href}")
    }
}

struct ParsedChampionRating {
    rating: String,
    summary: Option<String>,
}

struct ApexSourceLookupError {
    status: ApexParseStatus,
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_champion_rating_without_faking_success() {
        let html = r#"
            <main>
              <h1>灵魂虹吸</h1>
              <h2>效果描述</h2>
              <p>你的暴击会为你提供治疗。</p>
              <h2>关联英雄及联动分析</h2>
              <article><span>放逐之刃</span><strong>SS</strong><p>暴击和吸血都有收益。</p></article>
            </main>
        "#;

        let result = parse_apex_detail_page(
            html,
            "https://apexlol.info/zh/hextech/77",
            "放逐之刃",
            "灵魂虹吸",
            "https://apexlol.info/zh/hextech/77",
            12,
        );

        assert_eq!(result.status, ApexParseStatus::Ok);
        assert_eq!(result.rating.as_deref(), Some("SS"));
        assert_eq!(result.summary, "暴击和吸血都有收益。");
        assert_eq!(result.tip.as_deref(), Some("你的暴击会为你提供治疗。"));
        assert!(!result.cache_hit);
    }

    #[test]
    fn parse_failure_returns_no_data_status() {
        let html = r#"<main><h1>灵魂虹吸</h1><p>没有对应英雄。</p></main>"#;

        let result = parse_apex_detail_page(
            html,
            "https://apexlol.info/zh/hextech/77",
            "放逐之刃",
            "灵魂虹吸",
            "https://apexlol.info/zh/hextech/77",
            8,
        );

        assert_eq!(result.status, ApexParseStatus::ParseFailed);
        assert_eq!(result.summary, NO_DATA_TEXT);
        assert!(result.rating.is_none());
        assert!(result.error.is_some());
    }

    #[test]
    fn cache_key_separates_champion_and_augment() {
        assert_ne!(cache_key("Vi", "吞噬灵魂"), cache_key("Viego", "吞噬灵魂"));
        assert_eq!(
            cache_key("Cho'Gath", "灵魂虹吸"),
            cache_key("chogath", "灵魂虹吸")
        );
    }

    #[test]
    fn builds_cache_report_without_network() {
        let root = temp_dir("apex-report");
        let cache_dir = root.join("cache");
        let settings = ApexLookupSettings::default();
        let now = Utc::now();
        let ok_result = ApexLookupResult {
            cache_key: cache_key("放逐之刃", "灵魂虹吸"),
            champion_name: "放逐之刃".to_string(),
            augment_name: "灵魂虹吸".to_string(),
            rating: Some("SS".to_string()),
            summary: "暴击和吸血都有收益。".to_string(),
            tip: Some("你的暴击会为你提供治疗。".to_string()),
            source: APEX_SOURCE_NAME.to_string(),
            source_url: "https://apexlol.info/zh/hextech/77".to_string(),
            fetched_at: now,
            expires_at: now + Duration::hours(settings.cache_ttl_hours as i64),
            cache_hit: false,
            status: ApexParseStatus::Ok,
            error: None,
            request_log: ApexRequestLog {
                request_url: "https://apexlol.info/zh/hextech/77".to_string(),
                duration_ms: 18,
                cache_hit: false,
                parse_status: ApexParseStatus::Ok,
                failure_reason: None,
            },
        };
        let failed_result = no_data_result(
            "德玛西亚之力",
            "不存在的海克斯",
            "https://apexlol.info/zh/hextech/",
            "https://apexlol.info/zh/hextech/",
            20,
            ApexParseStatus::RequestFailed,
            "测试失败",
        );

        let mut cache = ApexCacheFile::default();
        cache.entries.insert(
            cache_key(&ok_result.champion_name, &ok_result.augment_name),
            cache_entry_from_result(&ok_result, &settings),
        );
        cache.entries.insert(
            cache_key(&failed_result.champion_name, &failed_result.augment_name),
            cache_entry_from_result(&failed_result, &settings),
        );
        write_cache(&cache_path(&cache_dir), &cache).expect("应能写入测试缓存");

        let report = build_cache_report(&cache_dir).expect("应能离线解析缓存报告");

        assert_eq!(report.total_entries, 2);
        assert_eq!(report.ok_entries, 1);
        assert_eq!(report.failed_entries, 1);
        assert!(report
            .entries
            .iter()
            .any(|entry| entry.source_url.contains("apexlol.info")));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cache_hit_returns_cached_metadata_without_network() {
        let root = temp_dir("apex-cache-hit");
        let cache_dir = root.join("cache");
        let fetched_at = Utc::now();
        let entry = ApexCacheEntry {
            champion_name: "放逐之刃".to_string(),
            augment_name: "灵魂虹吸".to_string(),
            rating: Some("SS".to_string()),
            summary: "缓存命中摘要".to_string(),
            tip: Some("缓存命中提醒".to_string()),
            source: APEX_SOURCE_NAME.to_string(),
            source_url: "https://apexlol.info/zh/hextech/77".to_string(),
            fetched_at,
            expires_at: fetched_at + Duration::hours(1),
            cache_hit: false,
            status: ApexParseStatus::Ok,
            error: None,
            request_url: "https://apexlol.info/zh/hextech/77".to_string(),
            duration_ms: 31,
        };
        let mut cache = ApexCacheFile::default();
        cache
            .entries
            .insert(cache_key("放逐之刃", "灵魂虹吸"), entry);
        write_cache(&cache_path(&cache_dir), &cache).expect("应能写入测试缓存");

        let result = lookup_with_cache(
            &cache_dir,
            ApexLookupRequest {
                champion_name: "放逐之刃".to_string(),
                augment_name: "灵魂虹吸".to_string(),
                force_refresh: false,
            },
            ApexLookupSettings {
                request_timeout_ms: 1,
                ..ApexLookupSettings::default()
            },
        )
        .expect("应命中缓存，不依赖网络");

        assert!(result.cache_hit);
        assert!(result.request_log.cache_hit);
        assert_eq!(result.summary, "缓存命中摘要");
        assert_eq!(result.request_log.duration_ms, 31);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failed_result_uses_short_ttl() {
        let settings = ApexLookupSettings {
            cache_ttl_hours: 168,
            request_timeout_ms: 6000,
            failed_cache_ttl_minutes: 3,
        };
        let failed_result = no_data_result(
            "德玛西亚之力",
            "不存在的海克斯",
            INDEX_URLS[0],
            INDEX_URLS[0],
            20,
            ApexParseStatus::RequestFailed,
            "测试失败",
        );

        let entry = cache_entry_from_result(&failed_result, &settings);
        let ttl = entry.expires_at - entry.fetched_at;

        assert_eq!(ttl.num_minutes(), 3);
        assert_eq!(entry.summary, NO_DATA_TEXT);
        assert!(entry.rating.is_none());
    }

    #[test]
    fn writes_offline_cache_report_file() {
        let root = temp_dir("apex-report-file");
        let cache_dir = root.join("cache");
        let reports_dir = root.join("reports");
        let failed_result = no_data_result(
            "德玛西亚之力",
            "不存在的海克斯",
            INDEX_URLS[0],
            INDEX_URLS[0],
            20,
            ApexParseStatus::NoData,
            "测试无数据",
        );
        let mut cache = ApexCacheFile::default();
        cache.entries.insert(
            cache_key(&failed_result.champion_name, &failed_result.augment_name),
            cache_entry_from_result(&failed_result, &ApexLookupSettings::default()),
        );
        write_cache(&cache_path(&cache_dir), &cache).expect("应能写入测试缓存");

        let report =
            build_and_write_cache_report(&cache_dir, &reports_dir).expect("应能写入离线报告");
        let report_path = report.report_path.expect("报告路径应返回");

        assert!(report_path.exists());
        assert_eq!(report.failed_entries, 1);

        let _ = fs::remove_dir_all(root);
    }

    fn temp_dir(label: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间应可用")
            .as_micros();
        std::env::temp_dir().join(format!("hex-assistant-{label}-{suffix}"))
    }
}
