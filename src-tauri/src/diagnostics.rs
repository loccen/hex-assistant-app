use crate::app_paths::AppPaths;
use crate::models::{
    DiagnosticExportResult, HealthCheckItem, HealthCheckReport, HealthStatus, RuntimeOverview,
    TelemetryEvent, TelemetryEventInput,
};
use crate::settings::load_or_create_settings;
use crate::telemetry::{append_event, new_trace_id, write_event};
use chrono::Utc;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Instant;
use tauri::AppHandle;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;

pub fn initialize(app: &AppHandle) -> Result<RuntimeOverview, String> {
    let start = Instant::now();
    let paths = AppPaths::from_app(app)?;
    paths.ensure_all()?;
    let settings = load_or_create_settings(&paths)?;

    let overview = RuntimeOverview {
        app_data_dir: paths.root.clone(),
        settings_path: paths.settings_path(),
        settings,
        directories: paths.status_list(),
        latest_log_path: paths.app_log_path(),
    };

    let event = TelemetryEvent {
        timestamp: Utc::now().to_rfc3339(),
        trace_id: new_trace_id("startup"),
        stage: "runtime-init".to_string(),
        input_summary: "应用启动".to_string(),
        output_summary: format!("应用数据目录 {}", overview.app_data_dir.display()),
        duration_ms: start.elapsed().as_millis(),
        level: "info".to_string(),
        error_code: None,
        message: "阶段 1 基础设施初始化完成".to_string(),
    };
    append_event(&paths, &event)?;

    Ok(overview)
}

pub fn runtime_overview(app: &AppHandle) -> Result<RuntimeOverview, String> {
    initialize(app)
}

pub fn health_check(app: &AppHandle) -> Result<HealthCheckReport, String> {
    let start = Instant::now();
    let paths = AppPaths::from_app(app)?;
    paths.ensure_all()?;
    let settings = load_or_create_settings(&paths)?;
    let trace_id = new_trace_id("health");

    let mut items = Vec::new();
    items.push(HealthCheckItem {
        key: "app-data".to_string(),
        name: "应用数据目录".to_string(),
        status: HealthStatus::Pass,
        details: format!("已准备 {}", paths.root.display()),
        error_code: None,
    });
    items.push(HealthCheckItem {
        key: "settings".to_string(),
        name: "默认配置".to_string(),
        status: HealthStatus::Pass,
        details: format!(
            "语言 {}，默认显示模式 {}",
            settings.language, settings.capture.default_display_mode
        ),
        error_code: None,
    });
    items.push(HealthCheckItem {
        key: "logs".to_string(),
        name: "结构化日志".to_string(),
        status: HealthStatus::Pass,
        details: format!("写入 {}", paths.app_log_path().display()),
        error_code: None,
    });
    items.push(HealthCheckItem {
        key: "ocr-model".to_string(),
        name: "OCR 模型文件".to_string(),
        status: HealthStatus::Warn,
        details: "阶段 4 接入 PP-OCRv4 rec ONNX；当前只检查资源目录规划".to_string(),
        error_code: Some("HEX-OCR-RESOURCE-PENDING".to_string()),
    });
    items.push(HealthCheckItem {
        key: "ort-runtime".to_string(),
        name: "ORT 动态库".to_string(),
        status: HealthStatus::Warn,
        details: "阶段 4 接入 ORT 动态库加载检查；当前未加载推理运行时".to_string(),
        error_code: Some("HEX-ORT-PENDING".to_string()),
    });
    items.push(HealthCheckItem {
        key: "live-client".to_string(),
        name: "Live Client Data API".to_string(),
        status: HealthStatus::NotChecked,
        details: "阶段 5 接入本地只读接口健康检查；阶段 1 不访问游戏接口".to_string(),
        error_code: None,
    });
    items.push(HealthCheckItem {
        key: "apex-lol".to_string(),
        name: "ApexLOL 网络可达性".to_string(),
        status: HealthStatus::NotChecked,
        details: "阶段 8 接入来源查询健康检查；阶段 1 不请求 ApexLOL".to_string(),
        error_code: None,
    });
    items.push(HealthCheckItem {
        key: "overlay".to_string(),
        name: "Overlay 能力".to_string(),
        status: HealthStatus::NotChecked,
        details: "阶段 7 接入透明置顶点击穿透窗口检查；真实验收必须在 Windows 桌面完成".to_string(),
        error_code: None,
    });

    let report = HealthCheckReport {
        trace_id: trace_id.clone(),
        generated_at: Utc::now().to_rfc3339(),
        items,
    };

    let report_path = paths
        .reports
        .join(format!("health-check-{}.json", timestamp_for_filename()));
    let content = serde_json::to_string_pretty(&report)
        .map_err(|error| format!("无法序列化健康检查报告: {error}"))?;
    fs::write(&report_path, format!("{content}\n"))
        .map_err(|error| format!("无法写入健康检查报告 {}: {error}", report_path.display()))?;

    let event = TelemetryEventInput {
        stage: "health-check".to_string(),
        input_summary: "阶段 1 基础健康检查".to_string(),
        output_summary: format!("报告 {}", report_path.display()),
        duration_ms: start.elapsed().as_millis(),
        level: "info".to_string(),
        error_code: None,
        message: format!("健康检查完成 trace_id={trace_id}"),
    };
    write_event(&paths, event)?;

    Ok(report)
}

pub fn export_diagnostic_package(app: &AppHandle) -> Result<DiagnosticExportResult, String> {
    let start = Instant::now();
    let paths = AppPaths::from_app(app)?;
    paths.ensure_all()?;
    let trace_id = new_trace_id("diag");
    let zip_path = paths.reports.join(format!(
        "diagnostic-package-{}.zip",
        timestamp_for_filename()
    ));

    write_environment_report(&paths, &trace_id)?;

    let zip_file = File::create(&zip_path)
        .map_err(|error| format!("无法创建诊断包 {}: {error}", zip_path.display()))?;
    let mut zip = zip::ZipWriter::new(zip_file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut included_files = 0usize;

    for dir in [
        &paths.config,
        &paths.calibration,
        &paths.logs,
        &paths.samples,
        &paths.ocr_replay,
        &paths.captures,
        &paths.reports,
        &paths.cache,
    ] {
        included_files += add_dir_to_zip(&mut zip, &paths.root, dir, &zip_path, options)?;
    }

    zip.finish()
        .map_err(|error| format!("无法完成诊断包写入: {error}"))?;

    let result = DiagnosticExportResult {
        trace_id: trace_id.clone(),
        zip_path: zip_path.clone(),
        included_files,
    };

    let event = TelemetryEventInput {
        stage: "diagnostic-export".to_string(),
        input_summary: format!("应用数据目录 {}", paths.root.display()),
        output_summary: format!("诊断包 {}，文件数 {}", zip_path.display(), included_files),
        duration_ms: start.elapsed().as_millis(),
        level: "info".to_string(),
        error_code: None,
        message: format!("诊断包导出完成 trace_id={trace_id}"),
    };
    write_event(&paths, event)?;

    Ok(result)
}

fn write_environment_report(paths: &AppPaths, trace_id: &str) -> Result<(), String> {
    let report = serde_json::json!({
        "traceId": trace_id,
        "generatedAt": Utc::now().to_rfc3339(),
        "appDataDir": paths.root,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "currentExe": std::env::current_exe().ok(),
        "diagnosticScope": [
            "config",
            "calibration",
            "logs",
            "samples",
            "ocr-replay",
            "captures",
            "reports",
            "cache"
        ],
        "securityBoundary": {
            "processInjection": false,
            "hooking": false,
            "memoryRead": false,
            "autoClick": false,
            "autoSelect": false,
            "keyboardMouseSimulation": false
        }
    });
    let path = paths.reports.join("environment.json");
    let content = serde_json::to_string_pretty(&report)
        .map_err(|error| format!("无法序列化环境报告: {error}"))?;
    fs::write(&path, format!("{content}\n"))
        .map_err(|error| format!("无法写入环境报告 {}: {error}", path.display()))
}

fn add_dir_to_zip(
    zip: &mut zip::ZipWriter<File>,
    root: &Path,
    dir: &Path,
    current_zip: &Path,
    options: SimpleFileOptions,
) -> Result<usize, String> {
    if !dir.exists() {
        return Ok(0);
    }

    let mut included = 0usize;
    for entry in WalkDir::new(dir).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() || path == current_zip {
            continue;
        }

        let relative = path
            .strip_prefix(root)
            .map_err(|error| format!("无法计算诊断包相对路径 {}: {error}", path.display()))?;
        let relative_name = relative.to_string_lossy().replace('\\', "/");
        zip.start_file(relative_name, options)
            .map_err(|error| format!("无法添加诊断文件 {}: {error}", path.display()))?;

        let mut file = File::open(path)
            .map_err(|error| format!("无法读取诊断文件 {}: {error}", path.display()))?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .map_err(|error| format!("无法读取诊断文件内容 {}: {error}", path.display()))?;
        zip.write_all(&buffer)
            .map_err(|error| format!("无法写入诊断包文件 {}: {error}", path.display()))?;
        included += 1;
    }

    Ok(included)
}

fn timestamp_for_filename() -> String {
    Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}
