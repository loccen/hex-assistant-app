use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub version: u32,
    pub language: String,
    pub capture: CaptureSettings,
    pub ocr: OcrSettings,
    pub overlay: OverlaySettings,
    pub apex_lol: ApexLolSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            version: 1,
            language: "zh-CN".to_string(),
            capture: CaptureSettings {
                preferred_monitor_id: None,
                poll_interval_ms: 1000,
                retry_delay_ms: 200,
                default_display_mode: "borderless".to_string(),
            },
            ocr: OcrSettings {
                engine: "ppocr-v4-rec-onnx".to_string(),
                min_confidence: 0.85,
                min_match_score: 0.9,
            },
            overlay: OverlaySettings {
                enabled: true,
                click_through: true,
                gap: 8,
                max_height: 120,
            },
            apex_lol: ApexLolSettings {
                cache_ttl_hours: 168,
                request_timeout_ms: 6000,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSettings {
    pub preferred_monitor_id: Option<String>,
    pub poll_interval_ms: u64,
    pub retry_delay_ms: u64,
    pub default_display_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrSettings {
    pub engine: String,
    pub min_confidence: f32,
    pub min_match_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySettings {
    pub enabled: bool,
    pub click_through: bool,
    pub gap: u32,
    pub max_height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApexLolSettings {
    pub cache_ttl_hours: u64,
    pub request_timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryStatus {
    pub key: String,
    pub path: PathBuf,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeOverview {
    pub app_data_dir: PathBuf,
    pub settings_path: PathBuf,
    pub settings: AppSettings,
    pub directories: Vec<DirectoryStatus>,
    pub latest_log_path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckReport {
    pub trace_id: String,
    pub generated_at: String,
    pub items: Vec<HealthCheckItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckItem {
    pub key: String,
    pub name: String,
    pub status: HealthStatus,
    pub details: String,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub enum HealthStatus {
    Pass,
    Warn,
    Fail,
    NotChecked,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticExportResult {
    pub trace_id: String,
    pub zip_path: PathBuf,
    pub included_files: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryEventInput {
    pub stage: String,
    pub input_summary: String,
    pub output_summary: String,
    pub duration_ms: u128,
    pub level: String,
    pub error_code: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryEvent {
    pub timestamp: String,
    pub trace_id: String,
    pub stage: String,
    pub input_summary: String,
    pub output_summary: String,
    pub duration_ms: u128,
    pub level: String,
    pub error_code: Option<String>,
    pub message: String,
}
