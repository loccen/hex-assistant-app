use crate::apex;
use crate::app_paths::AppPaths;
use crate::calibration;
use crate::capture;
use crate::live_client::LiveClientDataApi;
use crate::models::{
    DiagnosticExportResult, HealthCheckItem, HealthCheckReport, HealthStatus, RuntimeOverview,
    TelemetryEvent, TelemetryEventInput,
};
use crate::ocr;
use crate::overlay::{self, OverlayAnchor, OverlayRect};
use crate::settings::load_or_create_settings;
use crate::state_machine::{
    AssistantStateMachine, AugmentChoice, LivePlayerSnapshot, PanelState, StateMachineInput,
};
use crate::telemetry::{append_event, new_trace_id, write_event};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tauri::{AppHandle, Manager};
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
    let capture_samples_dir = capture::capture_samples_dir(&paths.root);
    items.push(HealthCheckItem {
        key: "capture-command".to_string(),
        name: "截图样本采集".to_string(),
        status: HealthStatus::NotChecked,
        details: format!(
            "命令已接入；健康检查不主动截屏，样本目录 {}",
            capture_samples_dir.display()
        ),
        error_code: None,
    });
    let calibration_path = calibration::calibration_config_path(&paths.root);
    items.push(HealthCheckItem {
        key: "calibration".to_string(),
        name: "屏幕校准配置".to_string(),
        status: if calibration_path.exists() {
            HealthStatus::Pass
        } else {
            HealthStatus::Warn
        },
        details: if calibration_path.exists() {
            format!("已找到 {}", calibration_path.display())
        } else {
            format!("尚未保存校准配置 {}", calibration_path.display())
        },
        error_code: (!calibration_path.exists()).then(|| "HEX-CALIBRATION-MISSING".to_string()),
    });
    let ocr_status = ocr::check_ppocr_resources(resource_root(app));
    items.push(HealthCheckItem {
        key: "ocr-model".to_string(),
        name: "OCR 模型文件".to_string(),
        status: if ocr_status.ready {
            HealthStatus::Pass
        } else {
            HealthStatus::Warn
        },
        details: ocr_status.message,
        error_code: ocr_status.error_code,
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
        details: format!(
            "只读接口命令已接入；健康检查不主动访问 {}",
            LiveClientDataApi::new().active_player_url()
        ),
        error_code: None,
    });
    let state_machine_report = run_state_machine_self_check();
    items.push(state_machine_report);
    items.push(HealthCheckItem {
        key: "runtime-orchestrator".to_string(),
        name: "局内闭环编排器".to_string(),
        status: HealthStatus::NotChecked,
        details:
            "命令已接入；低频监听只读取 Live Client 本地接口并复用已校准面板状态，健康检查不主动访问游戏接口"
                .to_string(),
        error_code: None,
    });
    let apex_cache_report = apex::build_cache_report(&paths.cache);
    items.push(HealthCheckItem {
        key: "apex-lol".to_string(),
        name: "ApexLOL 缓存".to_string(),
        status: if apex_cache_report.is_ok() {
            HealthStatus::Pass
        } else {
            HealthStatus::Fail
        },
        details: match apex_cache_report {
            Ok(report) => format!(
                "缓存报告可生成；总条目 {}，失败条目 {}，健康检查不请求 ApexLOL 网络",
                report.total_entries, report.failed_entries
            ),
            Err(error) => format!("ApexLOL 缓存报告生成失败: {error}"),
        },
        error_code: None,
    });
    let overlay_plan = overlay::plan_overlay_bounds(
        OverlayRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        OverlayAnchor::TopRight,
        360,
        96,
        i32::try_from(settings.overlay.gap).unwrap_or(8),
    );
    items.push(HealthCheckItem {
        key: "overlay".to_string(),
        name: "Overlay 能力".to_string(),
        status: if overlay_plan.is_ok() {
            HealthStatus::Pass
        } else {
            HealthStatus::Fail
        },
        details: match overlay_plan {
            Ok(bounds) => format!(
                "几何规划可用 {}x{} @ {},{}；真实透明置顶与点击穿透需在 Windows 桌面验收",
                bounds.width, bounds.height, bounds.x, bounds.y
            ),
            Err(error) => format!("Overlay 几何规划失败: {error}"),
        },
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
        trace_id: trace_id.to_string(),
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

pub fn export_release_package(app: &AppHandle) -> Result<DiagnosticExportResult, String> {
    let start = Instant::now();
    let paths = AppPaths::from_app(app)?;
    paths.ensure_all()?;
    let trace_id = new_trace_id("release");
    let workspace_root = workspace_root();
    let result = build_release_package(&workspace_root, &trace_id)?;

    write_event(
        &paths,
        TelemetryEventInput {
            stage: "release-export".to_string(),
            input_summary: "生成 release 压缩包".to_string(),
            output_summary: format!(
                "{}，文件数 {}",
                result.zip_path.display(),
                result.included_files
            ),
            duration_ms: start.elapsed().as_millis(),
            level: "info".to_string(),
            error_code: None,
            message: format!("release 压缩包生成完成 trace_id={trace_id}"),
        },
    )?;

    Ok(result)
}

pub fn build_release_package(
    workspace_root: &Path,
    trace_id: &str,
) -> Result<DiagnosticExportResult, String> {
    let release_dir = workspace_root.join("release");
    fs::create_dir_all(&release_dir)
        .map_err(|error| format!("无法创建 release 目录 {}: {error}", release_dir.display()))?;
    let zip_path = release_dir.join(format!(
        "hex-assistant-release-{}.zip",
        timestamp_for_filename()
    ));

    let zip_file = File::create(&zip_path)
        .map_err(|error| format!("无法创建 release 压缩包 {}: {error}", zip_path.display()))?;
    let mut zip = zip::ZipWriter::new(zip_file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut included_files = 0usize;
    let mut written_entries = HashSet::new();
    let mut checksums = Vec::new();

    for dir in [
        workspace_root.join("dist"),
        workspace_root.join("docs"),
        workspace_root.join("release"),
        workspace_root.join("src-tauri").join("resources"),
    ] {
        included_files += add_dir_to_zip_with_checksums(
            &mut zip,
            workspace_root,
            &dir,
            &zip_path,
            options,
            &mut written_entries,
            &mut checksums,
        )?;
    }

    let artifacts = find_release_artifacts(workspace_root);
    for artifact in &artifacts {
        let entry_name = release_artifact_entry_name(workspace_root, artifact);
        included_files += add_file_to_zip_as(
            &mut zip,
            artifact,
            &entry_name,
            options,
            &mut written_entries,
            &mut checksums,
        )?;
    }

    let manifest = serde_json::json!({
        "traceId": trace_id,
        "generatedAt": Utc::now().to_rfc3339(),
        "packageKind": "development-release-bundle",
        "sourceCommit": current_git_commit(workspace_root),
        "hostOs": std::env::consts::OS,
        "hostArch": std::env::consts::ARCH,
        "artifacts": artifacts
            .iter()
            .map(|path| path.strip_prefix(workspace_root).unwrap_or(path).to_string_lossy().replace('\\', "/"))
            .collect::<Vec<_>>(),
        "windowsInstaller": find_windows_bundle(workspace_root),
        "resourceInventory": release_resource_inventory(workspace_root),
        "includedSections": [
            "dist",
            "docs",
            "release",
            "src-tauri/resources",
            "packages"
        ],
        "notes": [
            "当前 WSL 环境可生成 Linux 产物；Windows exe/msi 需要 Windows runner 或满足 Tauri 交叉编译条件。",
            "如果没有 Windows Tauri 打包产物，此压缩包会保留资源说明、README、已知限制、诊断包导出说明和验证记录，但不能写作 Windows 发布已验收。",
            "真实 Windows 截图、Overlay 点击穿透、OCR 模型加载和 ORT 动态库加载仍需在 Windows 桌面验收。"
        ]
    });
    let manifest_content = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("无法序列化 release manifest: {error}"))?;
    included_files += add_bytes_to_zip_as(
        &mut zip,
        b"release-manifest.json",
        &manifest_content,
        options,
        &mut written_entries,
        &mut checksums,
    )?;

    let checksum_content = checksums
        .iter()
        .map(|(entry, checksum)| format!("{checksum}  {entry}\n"))
        .collect::<String>();
    included_files += add_bytes_to_zip_as(
        &mut zip,
        b"checksums.txt",
        checksum_content.as_bytes(),
        options,
        &mut written_entries,
        &mut checksums,
    )?;

    zip.finish()
        .map_err(|error| format!("无法完成 release 压缩包写入: {error}"))?;

    let result = DiagnosticExportResult {
        trace_id: trace_id.to_string(),
        zip_path: zip_path.clone(),
        included_files,
    };

    Ok(result)
}

fn run_state_machine_self_check() -> HealthCheckItem {
    let mut machine = AssistantStateMachine::new();
    let events = machine.apply(StateMachineInput {
        player: Some(LivePlayerSnapshot {
            champion_name: "Ahri".to_string(),
            level: 7,
        }),
        panel_state: PanelState::Expanded,
        choices: vec![
            AugmentChoice {
                slot: 0,
                augment_id: "prismatic-ticket".to_string(),
            },
            AugmentChoice {
                slot: 1,
                augment_id: "build-a-bud".to_string(),
            },
            AugmentChoice {
                slot: 2,
                augment_id: "trade-sector".to_string(),
            },
        ],
        selected_slot: None,
        pause_reason: None,
    });

    HealthCheckItem {
        key: "state-machine".to_string(),
        name: "状态机离线模拟".to_string(),
        status: HealthStatus::Pass,
        details: format!(
            "状态 {:?}，待选阶段 {:?}，待处理档位 {:?}，事件数 {}",
            machine.state().status,
            machine.state().pending_tier,
            machine.state().pending_tiers,
            events.len()
        ),
        error_code: None,
    }
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

fn add_dir_to_zip_with_checksums(
    zip: &mut zip::ZipWriter<File>,
    root: &Path,
    dir: &Path,
    current_zip: &Path,
    options: SimpleFileOptions,
    written_entries: &mut HashSet<String>,
    checksums: &mut Vec<(String, String)>,
) -> Result<usize, String> {
    if !dir.exists() {
        return Ok(0);
    }

    let mut included = 0usize;
    for entry in WalkDir::new(dir).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() || should_skip_release_input(path, current_zip) {
            continue;
        }

        let relative = path
            .strip_prefix(root)
            .map_err(|error| format!("无法计算 release 相对路径 {}: {error}", path.display()))?;
        let relative_name = relative.to_string_lossy().replace('\\', "/");
        included += add_file_to_zip_as(
            zip,
            path,
            &relative_name,
            options,
            written_entries,
            checksums,
        )?;
    }

    Ok(included)
}

fn add_file_to_zip_as(
    zip: &mut zip::ZipWriter<File>,
    path: &Path,
    entry_name: &str,
    options: SimpleFileOptions,
    written_entries: &mut HashSet<String>,
    checksums: &mut Vec<(String, String)>,
) -> Result<usize, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("无法读取 release 文件 {}: {error}", path.display()))?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .map_err(|error| format!("无法读取 release 文件内容 {}: {error}", path.display()))?;
    add_bytes_to_zip_as(
        zip,
        entry_name.as_bytes(),
        &buffer,
        options,
        written_entries,
        checksums,
    )
}

fn add_bytes_to_zip_as(
    zip: &mut zip::ZipWriter<File>,
    entry_name: &[u8],
    content: &[u8],
    options: SimpleFileOptions,
    written_entries: &mut HashSet<String>,
    checksums: &mut Vec<(String, String)>,
) -> Result<usize, String> {
    let entry_name = String::from_utf8_lossy(entry_name).replace('\\', "/");
    if !written_entries.insert(entry_name.clone()) {
        return Ok(0);
    }

    zip.start_file(&entry_name, options)
        .map_err(|error| format!("无法添加 release 文件 {entry_name}: {error}"))?;
    zip.write_all(content)
        .map_err(|error| format!("无法写入 release 文件 {entry_name}: {error}"))?;
    checksums.push((entry_name, format!("{:x}", Sha256::digest(content))));
    Ok(1)
}

fn should_skip_release_input(path: &Path, current_zip: &Path) -> bool {
    if path == current_zip {
        return true;
    }
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    file_name.starts_with("hex-assistant-release-")
        && path.extension().and_then(|value| value.to_str()) == Some("zip")
}

fn find_release_artifacts(workspace_root: &Path) -> Vec<PathBuf> {
    let mut artifacts = Vec::new();
    let target_release = workspace_root
        .join("src-tauri")
        .join("target")
        .join("release");
    for candidate in [
        target_release.join("hex-assistant-app"),
        target_release.join("hex-assistant-app.exe"),
        target_release.join("LOL 海克斯助手.exe"),
    ] {
        if candidate.is_file() {
            artifacts.push(candidate);
        }
    }

    let bundle_dir = target_release.join("bundle");
    if bundle_dir.exists() {
        for entry in WalkDir::new(bundle_dir).into_iter().filter_map(Result::ok) {
            let path = entry.into_path();
            if path.is_file() && is_release_artifact(&path) {
                artifacts.push(path);
            }
        }
    }

    artifacts.sort();
    artifacts.dedup();
    artifacts
}

fn is_release_artifact(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
            .as_deref(),
        Some("exe" | "msi" | "msix" | "appx" | "deb" | "rpm" | "appimage" | "dmg" | "zip")
    )
}

fn release_artifact_entry_name(workspace_root: &Path, artifact: &Path) -> String {
    let bundle_dir = workspace_root
        .join("src-tauri")
        .join("target")
        .join("release")
        .join("bundle");
    if let Ok(relative) = artifact.strip_prefix(&bundle_dir) {
        return format!(
            "packages/bundle/{}",
            relative.to_string_lossy().replace('\\', "/")
        );
    }

    let file_name = artifact
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("artifact");
    format!("packages/{file_name}")
}

fn release_resource_inventory(workspace_root: &Path) -> serde_json::Value {
    let resources = workspace_root.join("src-tauri").join("resources");
    serde_json::json!({
        "dictionaries": list_relative_files(&resources, &resources.join("dictionaries")),
        "models": list_relative_files(&resources, &resources.join("models")),
        "onnxruntime": list_relative_files(&resources, &resources.join("onnxruntime")),
        "requiredRuntimeNotes": [
            "PP-OCRv4 rec ONNX 模型应随包放入 resources/models。",
            "Windows ORT 动态库应随包放入 resources/onnxruntime，至少包含 onnxruntime.dll。",
            "当前资源目录已包含真实 OCR 模型、Windows x64 ONNX Runtime 1.25.0 动态库和许可说明；Windows 真实加载仍需单独验收。"
        ]
    })
}

fn list_relative_files(root: &Path, dir: &Path) -> Vec<String> {
    if !dir.exists() {
        return Vec::new();
    }
    WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .map(|entry| entry.into_path())
        .filter(|path| path.is_file())
        .filter_map(|path| {
            path.strip_prefix(root)
                .ok()
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        })
        .collect()
}

fn current_git_commit(workspace_root: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace_root)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn timestamp_for_filename() -> String {
    Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

fn resource_root(app: &AppHandle) -> PathBuf {
    app.path()
        .resource_dir()
        .ok()
        .filter(|path| path.exists())
        .unwrap_or_else(|| workspace_root().join("src-tauri").join("resources"))
}

fn workspace_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn find_windows_bundle(workspace_root: &Path) -> Option<PathBuf> {
    let bundle_dir = workspace_root
        .join("src-tauri")
        .join("target")
        .join("release")
        .join("bundle");
    if !bundle_dir.exists() {
        return None;
    }
    WalkDir::new(bundle_dir)
        .into_iter()
        .filter_map(Result::ok)
        .map(|entry| entry.into_path())
        .find(|path| {
            path.is_file()
                && matches!(
                    path.extension().and_then(|value| value.to_str()),
                    Some("exe" | "msi" | "nsis")
                )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn release_package_contains_valid_manifest_json() {
        let workspace = temp_workspace("release-package");
        fs::create_dir_all(workspace.join("dist")).expect("应能创建 dist");
        fs::create_dir_all(workspace.join("docs")).expect("应能创建 docs");
        fs::create_dir_all(workspace.join("release")).expect("应能创建 release");
        fs::create_dir_all(workspace.join("src-tauri").join("resources").join("models"))
            .expect("应能创建 resources");
        fs::write(workspace.join("dist").join("index.html"), "<main></main>\n")
            .expect("应能写入前端产物");
        fs::write(workspace.join("docs").join("README.md"), "# docs\n").expect("应能写入文档");
        fs::write(workspace.join("release").join("README.md"), "# release\n")
            .expect("应能写入 release 文档");
        fs::write(
            workspace
                .join("src-tauri")
                .join("resources")
                .join("models")
                .join("README.md"),
            "# models\n",
        )
        .expect("应能写入资源说明");

        let result =
            build_release_package(&workspace, "release-test").expect("应能生成 release 包");
        assert!(result.included_files >= 4);

        let file = File::open(&result.zip_path).expect("应能打开 release 包");
        let mut archive = zip::ZipArchive::new(file).expect("应能读取 release zip");
        let mut manifest_file = archive
            .by_name("release-manifest.json")
            .expect("应包含 release manifest");
        let mut manifest_content = String::new();
        manifest_file
            .read_to_string(&mut manifest_content)
            .expect("应能读取 release manifest");
        drop(manifest_file);

        let manifest: serde_json::Value =
            serde_json::from_str(&manifest_content).expect("manifest 应为合法 JSON");
        assert_eq!(manifest["traceId"], "release-test");
        assert_eq!(manifest["packageKind"], "development-release-bundle");
        assert!(archive.by_name("dist/index.html").is_ok());
        assert!(archive
            .by_name("src-tauri/resources/models/README.md")
            .is_ok());
        assert!(archive.by_name("checksums.txt").is_ok());

        let _ = fs::remove_dir_all(workspace);
    }

    fn temp_workspace(label: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间应可用")
            .as_nanos();
        std::env::temp_dir().join(format!("hex-assistant-{label}-{suffix}"))
    }
}
