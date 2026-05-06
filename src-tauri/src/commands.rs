use crate::apex::{self, ApexCacheReport, ApexLookupRequest, ApexLookupResult};
use crate::calibration::{
    self, CalibrationConfig, CalibrationProfileResult, PixelCalibrationInput,
};
use crate::capture::{self, CaptureSampleReport, MonitorDiagnostic};
use crate::diagnostics;
use crate::live_client::{ActivePlayerSnapshot, LiveClientDataApi};
use crate::models::{
    DiagnosticExportResult, HealthCheckReport, RuntimeOverview, TelemetryEvent, TelemetryEventInput,
};
use crate::ocr::{
    self, CalibratedNameOcrReport, CalibratedNameSlot, OfflineReplayReport, SlotReplayInput,
    AUGMENT_DICTIONARY_ZH_CN,
};
use crate::orchestrator::{RuntimeLoopSnapshot, RuntimeOrchestratorHandle, RuntimeTriggerRequest};
use crate::overlay::{
    self, OverlayOperationReport, OverlaySlotData, OverlaySlotUpdateReport, OverlayTestCardRequest,
};
use crate::resource_paths;
use crate::settings::load_or_create_settings;
use crate::state_machine::{AssistantState, AssistantStateMachine, StateMachineInput};
use crate::{app_paths::AppPaths, telemetry};
use base64::{engine::general_purpose, Engine};
#[cfg(not(test))]
use chrono::Utc;
use serde::{Deserialize, Serialize};
#[cfg(not(test))]
use std::path::PathBuf;
#[cfg(not(test))]
use std::time::Instant;
use tauri::{AppHandle, State};

#[tauri::command]
pub fn get_runtime_overview(app: AppHandle) -> Result<RuntimeOverview, String> {
    diagnostics::runtime_overview(&app)
}

#[tauri::command]
pub fn run_health_check(app: AppHandle) -> Result<HealthCheckReport, String> {
    diagnostics::health_check(&app)
}

#[tauri::command]
pub fn export_diagnostic_package(app: AppHandle) -> Result<DiagnosticExportResult, String> {
    diagnostics::export_diagnostic_package(&app)
}

#[tauri::command]
pub fn export_release_package(app: AppHandle) -> Result<DiagnosticExportResult, String> {
    diagnostics::export_release_package(&app)
}

#[tauri::command]
pub fn write_structured_log(
    app: AppHandle,
    input: TelemetryEventInput,
) -> Result<TelemetryEvent, String> {
    let paths = AppPaths::from_app(&app)?;
    paths.ensure_all()?;
    telemetry::write_event(&paths, input)
}

#[tauri::command]
pub fn capture_monitor_sample(
    app: AppHandle,
    preferred_monitor_id: Option<u32>,
) -> Result<CaptureSampleReport, String> {
    let paths = AppPaths::from_app(&app)?;
    paths.ensure_all()?;
    capture::capture_monitor_sample(&paths.root, preferred_monitor_id)
}

#[tauri::command]
pub fn load_latest_capture_sample(app: AppHandle) -> Result<CaptureSampleReport, String> {
    let paths = AppPaths::from_app(&app)?;
    paths.ensure_all()?;
    capture::load_latest_capture_sample(&paths.root)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PngDataUrlResult {
    pub path: std::path::PathBuf,
    pub data_url: String,
    pub bytes: usize,
}

#[tauri::command]
pub fn read_png_file_as_data_url(path: std::path::PathBuf) -> Result<PngDataUrlResult, String> {
    let bytes = std::fs::read(&path).map_err(|error| {
        format!(
            "HEX-CAPTURE-PREVIEW-READ: 无法读取截图 {}: {error}",
            path.display()
        )
    })?;
    let data_url = format!(
        "data:image/png;base64,{}",
        general_purpose::STANDARD.encode(&bytes)
    );
    Ok(PngDataUrlResult {
        path,
        bytes: bytes.len(),
        data_url,
    })
}

#[tauri::command]
pub fn list_capture_monitors() -> Result<Vec<MonitorDiagnostic>, String> {
    capture::list_monitor_diagnostics()
        .map_err(|error| format!("HEX-CAPTURE-MONITOR-LIST: {error}"))
}

#[tauri::command]
pub fn save_calibration_profile(
    app: AppHandle,
    config: CalibrationConfig,
) -> Result<String, String> {
    let paths = AppPaths::from_app(&app)?;
    paths.ensure_all()?;
    let path = calibration::save_calibration_config(&paths.root, &config)?;
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn save_pixel_calibration_profile(
    app: AppHandle,
    input: PixelCalibrationInput,
) -> Result<CalibrationProfileResult, String> {
    let paths = AppPaths::from_app(&app)?;
    paths.ensure_all()?;
    calibration::save_pixel_calibration_config(&paths.root, input)
        .map_err(|error| format!("HEX-CALIBRATION-SAVE: {error}"))
}

#[tauri::command]
pub fn load_calibration_profile(app: AppHandle) -> Result<CalibrationProfileResult, String> {
    let paths = AppPaths::from_app(&app)?;
    calibration::load_calibration_profile(&paths.root)
        .map_err(|error| format!("HEX-CALIBRATION-LOAD: {error}"))
}

#[tauri::command]
pub fn check_ocr_resources(app: AppHandle) -> Result<ocr::OcrResourceStatus, String> {
    Ok(ocr::check_ppocr_resources(resource_paths::resource_root(
        &app,
    )))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrReplayCommandInput {
    pub left_text: String,
    pub center_text: String,
    pub right_text: String,
    pub confidence: Option<f32>,
}

#[tauri::command]
pub fn run_ocr_text_replay(
    app: AppHandle,
    input: OcrReplayCommandInput,
) -> Result<OfflineReplayReport, String> {
    let paths = AppPaths::from_app(&app)?;
    paths.ensure_all()?;
    let settings = load_or_create_settings(&paths)?;
    let (dictionary, _) =
        load_runtime_augment_dictionary(&paths, &settings, &resource_paths::resource_root(&app))?;
    let confidence = input.confidence.unwrap_or(0.95);
    let report = ocr::replay_calibrated_name_slots(
        &dictionary,
        &[
            SlotReplayInput {
                slot: CalibratedNameSlot::Left,
                raw_text: input.left_text,
                confidence,
            },
            SlotReplayInput {
                slot: CalibratedNameSlot::Center,
                raw_text: input.center_text,
                confidence,
            },
            SlotReplayInput {
                slot: CalibratedNameSlot::Right,
                raw_text: input.right_text,
                confidence,
            },
        ],
        settings.ocr.min_confidence,
        settings.ocr.min_match_score,
    )
    .map_err(|error| error.to_string())?;
    ocr::write_offline_replay_report(&paths.ocr_replay, &report)
        .map_err(|error| error.to_string())?;
    Ok(report)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibratedNameOcrCommandInput {
    pub screenshot_path: Option<std::path::PathBuf>,
    pub preferred_monitor_id: Option<u32>,
}

#[cfg(not(test))]
#[tauri::command]
pub async fn run_calibrated_name_ocr(
    app: AppHandle,
    input: Option<CalibratedNameOcrCommandInput>,
) -> Result<CalibratedNameOcrReport, String> {
    let paths = AppPaths::from_app(&app)?;
    paths.ensure_all()?;
    let settings = load_or_create_settings(&paths)?;
    let calibration = calibration::load_calibration_config(&paths.root)?;
    let input = input.unwrap_or(CalibratedNameOcrCommandInput {
        screenshot_path: None,
        preferred_monitor_id: None,
    });
    let preferred_monitor_id = input.preferred_monitor_id;
    let screenshot_path = input.screenshot_path.clone();
    let trace_id = telemetry::new_trace_id("ocr-check");
    write_ocr_telemetry(
        &paths,
        &trace_id,
        "info",
        None,
        "ocr-check-start",
        format!(
            "开始执行 OCR 校验 command=run_calibrated_name_ocr screenshot={} preferred_monitor_id={}",
            screenshot_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "自动截图".to_string()),
            preferred_monitor_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "未指定".to_string())
        ),
        "等待后台线程完成 OCR 初始化与推理".to_string(),
        0,
    );
    let resource_root = resource_paths::resource_root(&app);
    let paths_for_task = paths.clone();
    let trace_id_for_task = trace_id.clone();
    let start = Instant::now();
    let join_result = tauri::async_runtime::spawn_blocking(move || {
        run_calibrated_ocr_task(
            &paths_for_task,
            &trace_id_for_task,
            resource_root,
            calibration,
            screenshot_path,
            preferred_monitor_id,
            settings.ocr.min_confidence,
            settings.ocr.min_match_score,
        )
    })
    .await;

    finalize_ocr_telemetry(
        &paths,
        &trace_id,
        "run_calibrated_name_ocr",
        start,
        join_result,
    )
}

#[cfg(test)]
#[tauri::command]
pub async fn run_calibrated_name_ocr(
    _app: AppHandle,
    _input: Option<CalibratedNameOcrCommandInput>,
) -> Result<CalibratedNameOcrReport, String> {
    Err("HEX-OCR-TEST-STUB: Tauri 命令测试编译不执行 OCR 运行时路径".to_string())
}

#[cfg(not(test))]
#[tauri::command]
pub async fn run_pixel_calibrated_name_ocr(
    app: AppHandle,
    input: PixelCalibrationInput,
    screenshot_path: std::path::PathBuf,
) -> Result<CalibratedNameOcrReport, String> {
    let paths = AppPaths::from_app(&app)?;
    paths.ensure_all()?;
    let settings = load_or_create_settings(&paths)?;
    let calibration = calibration::build_calibration_config_from_pixels(input)
        .map_err(|error| format!("HEX-CALIBRATION-OCR-CHECK: {error}"))?;
    let screenshot_summary = screenshot_path.display().to_string();
    let trace_id = telemetry::new_trace_id("ocr-check");
    write_ocr_telemetry(
        &paths,
        &trace_id,
        "info",
        None,
        "ocr-check-start",
        format!(
            "开始执行 OCR 校验 command=run_pixel_calibrated_name_ocr screenshot={screenshot_summary}"
        ),
        "等待后台线程完成 OCR 初始化与推理".to_string(),
        0,
    );
    let resource_root = resource_paths::resource_root(&app);
    let paths_for_task = paths.clone();
    let trace_id_for_task = trace_id.clone();
    let start = Instant::now();
    let join_result = tauri::async_runtime::spawn_blocking(move || {
        run_calibrated_ocr_task(
            &paths_for_task,
            &trace_id_for_task,
            resource_root,
            calibration,
            Some(screenshot_path),
            None,
            settings.ocr.min_confidence,
            settings.ocr.min_match_score,
        )
    })
    .await;

    finalize_ocr_telemetry(
        &paths,
        &trace_id,
        "run_pixel_calibrated_name_ocr",
        start,
        join_result,
    )
}

#[cfg(not(test))]
pub(crate) fn run_calibrated_ocr_task(
    paths: &AppPaths,
    trace_id: &str,
    resource_root: PathBuf,
    calibration: CalibrationConfig,
    screenshot_path: Option<PathBuf>,
    preferred_monitor_id: Option<u32>,
    min_confidence: f32,
    min_match_score: f32,
) -> Result<CalibratedNameOcrReport, String> {
    log_ocr_stage(
        paths,
        trace_id,
        "ocr-resource-check-start",
        format!("开始检查 OCR 资源 root={}", resource_root.display()),
        "准备校验资源目录并按需镜像网络路径资源".to_string(),
    );
    let prepared = resource_paths::prepare_runtime_resource_root(&resource_root, &paths.cache)?;
    let resource_status = ocr::check_ppocr_resources(&prepared.runtime_root);
    if !resource_status.ready {
        return Err(resource_status.message);
    }
    log_ocr_stage(
        paths,
        trace_id,
        "ocr-resource-check-success",
        format!(
            "OCR 资源检查完成 source={} runtime={} mirrored={} cache_hit={}",
            prepared.source_root.display(),
            prepared.runtime_root.display(),
            prepared.mirrored_from_network,
            prepared.cache_hit
        ),
        resource_status.message.clone(),
    );

    log_ocr_stage(
        paths,
        trace_id,
        "ocr-dictionary-load-start",
        "开始加载 OCR 词库".to_string(),
        "准备读取海克斯词库".to_string(),
    );
    let settings = load_or_create_settings(paths)?;
    let (dictionary, dictionary_summary) =
        load_runtime_augment_dictionary(paths, &settings, &prepared.runtime_root)?;
    log_ocr_stage(
        paths,
        trace_id,
        "ocr-dictionary-load-success",
        dictionary_summary,
        "海克斯词库加载成功".to_string(),
    );

    log_ocr_stage(
        paths,
        trace_id,
        "ocr-ort-init-start",
        format!(
            "开始初始化 ONNX Runtime 动态库 root={}",
            prepared.runtime_root.display()
        ),
        "准备显式加载 onnxruntime 动态库".to_string(),
    );
    let ort_report = ocr::ensure_ort_runtime_loaded(&prepared.runtime_root)
        .map_err(|error| error.to_string())?;
    let ort_stage = if ort_report.cold_start {
        "ocr-ort-init-success"
    } else {
        "ocr-ort-reuse-hit"
    };
    let ort_message = if ort_report.cold_start {
        format!(
            "ONNX Runtime 动态库冷启动完成 path={} cold_start_ms={}",
            ort_report.dylib_path.display(),
            ort_report.elapsed_ms
        )
    } else {
        format!(
            "ONNX Runtime 动态库复用命中 path={} reuse_hit_ms={}",
            ort_report.dylib_path.display(),
            ort_report.elapsed_ms
        )
    };
    log_ocr_stage(
        paths,
        trace_id,
        ort_stage,
        format!(
            "ONNX Runtime 动态库可用 path={}",
            ort_report.dylib_path.display()
        ),
        ort_message,
    );

    log_ocr_stage(
        paths,
        trace_id,
        "ocr-recognizer-init-start",
        format!(
            "开始初始化 OCR 识别器 resource_root={}",
            prepared.runtime_root.display()
        ),
        "准备创建 ORT 会话并加载模型".to_string(),
    );
    let screenshot_path = match screenshot_path {
        Some(path) => path,
        None => {
            let capture_report =
                capture::capture_monitor_sample(&paths.root, preferred_monitor_id)?;
            capture_report.png_path
        }
    };
    log_ocr_stage(
        paths,
        trace_id,
        "ocr-inference-start",
        format!(
            "开始执行 OCR 推理 screenshot={} report_dir={}",
            screenshot_path.display(),
            paths.reports.display()
        ),
        "准备裁剪校准区域并执行 OCR 推理".to_string(),
    );
    let inference_start = Instant::now();
    let (report, recognizer_report) =
        ocr::with_cached_ppocr_recognizer(&prepared.runtime_root, |recognizer| {
            ocr::recognize_calibrated_name_slots_from_image(
                recognizer,
                &dictionary,
                &calibration,
                &screenshot_path,
                &paths.reports,
                min_confidence,
                min_match_score,
            )
        })
        .map_err(|error| error.to_string())?;
    let recognizer_stage = if recognizer_report.cold_start {
        "ocr-recognizer-init-success"
    } else {
        "ocr-recognizer-reuse-hit"
    };
    let recognizer_message = if recognizer_report.cold_start {
        format!(
            "OCR 识别器冷启动完成 model={} cold_start_ms={}",
            recognizer_report.model_path.display(),
            recognizer_report.elapsed_ms
        )
    } else {
        format!(
            "OCR 识别器复用命中 model={} reuse_hit_ms={}",
            recognizer_report.model_path.display(),
            recognizer_report.elapsed_ms
        )
    };
    log_ocr_stage(
        paths,
        trace_id,
        recognizer_stage,
        format!(
            "OCR 识别器可用 resource_root={}",
            recognizer_report.resource_root.display()
        ),
        recognizer_message,
    );
    log_ocr_stage(
        paths,
        trace_id,
        "ocr-inference-finish",
        format!(
            "OCR 推理完成 screenshot={} report={}",
            screenshot_path.display(),
            report.report_path.display()
        ),
        format!(
            "识别完成 slot_count={} inference_ms={}",
            report.slot_count,
            inference_start.elapsed().as_millis()
        ),
    );

    Ok(report)
}

pub(crate) fn load_runtime_augment_dictionary(
    paths: &AppPaths,
    settings: &crate::models::AppSettings,
    resource_root: &std::path::Path,
) -> Result<(ocr::AugmentDictionary, String), String> {
    match apex::load_augment_dictionary_with_cache(
        &paths.cache,
        apex::ApexLookupSettings {
            cache_ttl_hours: settings.apex_lol.cache_ttl_hours,
            request_timeout_ms: settings.apex_lol.request_timeout_ms,
            failed_cache_ttl_minutes: settings.apex_lol.failed_cache_ttl_minutes,
        },
        false,
    ) {
        Ok(sync) => {
            let augment_count = sync.dictionary.augments.len();
            let dictionary = ocr::AugmentDictionary {
                locale: sync.dictionary.locale,
                version: sync.dictionary.version,
                augments: sync
                    .dictionary
                    .augments
                    .into_iter()
                    .map(|entry| ocr::AugmentEntry {
                        id: entry.id,
                        name: entry.name,
                        aliases: entry.aliases,
                    })
                    .collect(),
            };
            Ok((
                dictionary,
                format!(
                    "OCR 词库加载完成 source={} cache={} stale_fallback={} path={} entries={}",
                    apex::APEX_SOURCE_NAME,
                    sync.cache_hit,
                    sync.stale_fallback_used,
                    sync.cache_path.display(),
                    augment_count
                ),
            ))
        }
        Err(fetch_error) => {
            let dictionary_path = resource_root
                .join("dictionaries")
                .join(AUGMENT_DICTIONARY_ZH_CN);
            let dictionary = ocr::AugmentDictionary::load(&dictionary_path)
                .map_err(|error| error.to_string())?;
            Ok((
                dictionary.clone(),
                format!(
                    "OCR 词库加载完成 source=bundled-fallback reason={} path={} entries={}",
                    fetch_error,
                    dictionary_path.display(),
                    dictionary.augments.len()
                ),
            ))
        }
    }
}

#[cfg(test)]
#[tauri::command]
pub async fn run_pixel_calibrated_name_ocr(
    _app: AppHandle,
    _input: PixelCalibrationInput,
    _screenshot_path: std::path::PathBuf,
) -> Result<CalibratedNameOcrReport, String> {
    Err("HEX-OCR-TEST-STUB: Tauri 命令测试编译不执行 OCR 运行时路径".to_string())
}

#[tauri::command]
pub fn fetch_live_client_resolved_player_snapshot() -> Result<ActivePlayerSnapshot, String> {
    LiveClientDataApi::new()
        .fetch_active_player()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn fetch_live_client_active_player() -> Result<ActivePlayerSnapshot, String> {
    // 兼容旧命令名，避免前端或外部脚本在 live_client 新接口合入前后发生破坏。
    fetch_live_client_resolved_player_snapshot()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateMachineCommandResult {
    pub state: AssistantState,
    pub events: Vec<crate::state_machine::StateTransitionEvent>,
}

#[tauri::command]
pub fn evaluate_state_machine(
    input: StateMachineInput,
) -> Result<StateMachineCommandResult, String> {
    let mut machine = AssistantStateMachine::new();
    let events = machine.apply(input);
    Ok(StateMachineCommandResult {
        state: machine.state().clone(),
        events,
    })
}

#[tauri::command]
pub fn get_runtime_orchestrator_status(
    orchestrator: State<RuntimeOrchestratorHandle>,
) -> Result<RuntimeLoopSnapshot, String> {
    orchestrator.snapshot()
}

#[tauri::command]
pub fn trigger_runtime_orchestrator(
    app: AppHandle,
    orchestrator: State<RuntimeOrchestratorHandle>,
    request: RuntimeTriggerRequest,
) -> Result<RuntimeLoopSnapshot, String> {
    orchestrator.trigger_once(&app, request)
}

#[tauri::command]
pub fn start_runtime_listener(
    app: AppHandle,
    orchestrator: State<RuntimeOrchestratorHandle>,
    request: RuntimeTriggerRequest,
) -> Result<RuntimeLoopSnapshot, String> {
    orchestrator.start_listener(&app, request)
}

#[tauri::command]
pub fn stop_runtime_listener(
    app: AppHandle,
    orchestrator: State<RuntimeOrchestratorHandle>,
) -> Result<RuntimeLoopSnapshot, String> {
    orchestrator.stop_listener(&app)
}

#[tauri::command]
pub fn lookup_apex_lol(
    app: AppHandle,
    request: ApexLookupRequest,
) -> Result<ApexLookupResult, String> {
    let paths = AppPaths::from_app(&app)?;
    paths.ensure_all()?;
    let settings = load_or_create_settings(&paths)?;
    apex::lookup_with_cache(
        &paths.cache,
        request,
        apex::ApexLookupSettings {
            cache_ttl_hours: settings.apex_lol.cache_ttl_hours,
            request_timeout_ms: settings.apex_lol.request_timeout_ms,
            failed_cache_ttl_minutes: settings.apex_lol.failed_cache_ttl_minutes,
        },
    )
}

#[tauri::command]
pub fn build_apex_cache_report(app: AppHandle) -> Result<ApexCacheReport, String> {
    let paths = AppPaths::from_app(&app)?;
    paths.ensure_all()?;
    apex::build_and_write_cache_report(&paths.cache, &paths.reports)
}

#[tauri::command]
pub fn show_overlay_test_card(
    app: AppHandle,
    request: OverlayTestCardRequest,
) -> Result<OverlayOperationReport, String> {
    overlay::show_overlay_test_card_inner(&app, request).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn hide_overlay_test_card(app: AppHandle) -> Result<OverlayOperationReport, String> {
    overlay::hide_overlay_test_card_inner(&app).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn update_overlay_slots(
    app: AppHandle,
    slots: Vec<OverlaySlotData>,
) -> Result<OverlaySlotUpdateReport, String> {
    overlay::update_overlay_slots_inner(&app, slots).map_err(|error| error.to_string())
}

#[cfg(not(test))]
fn finalize_ocr_telemetry(
    paths: &AppPaths,
    trace_id: &str,
    command_name: &str,
    start: Instant,
    join_result: Result<Result<CalibratedNameOcrReport, String>, tauri::Error>,
) -> Result<CalibratedNameOcrReport, String> {
    let duration_ms = start.elapsed().as_millis();
    match join_result {
        Ok(Ok(report)) => {
            let slot_summary = report
                .slots
                .iter()
                .map(|slot| {
                    format!(
                        "{}:{}",
                        slot.slot.as_str(),
                        slot.final_name
                            .clone()
                            .unwrap_or_else(|| slot.raw_text.clone())
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            write_ocr_telemetry(
                paths,
                trace_id,
                "info",
                None,
                "ocr-check-success",
                format!("OCR 校验完成 command={command_name}"),
                format!("识别完成，slots={slot_summary}"),
                duration_ms,
            );
            Ok(report)
        }
        Ok(Err(error)) => {
            write_ocr_telemetry(
                paths,
                trace_id,
                "error",
                Some("HEX-OCR-CHECK-FAILED"),
                "ocr-check-failed",
                format!("OCR 校验失败 command={command_name}"),
                error.clone(),
                duration_ms,
            );
            Err(error)
        }
        Err(error) => {
            let message = format!("后台 OCR 任务执行失败: {error}");
            write_ocr_telemetry(
                paths,
                trace_id,
                "error",
                Some("HEX-OCR-CHECK-JOIN"),
                "ocr-check-failed",
                format!("OCR 校验线程失败 command={command_name}"),
                message.clone(),
                duration_ms,
            );
            Err(message)
        }
    }
}

#[cfg(not(test))]
fn log_ocr_stage(
    paths: &AppPaths,
    trace_id: &str,
    stage: &str,
    input_summary: String,
    message: String,
) {
    write_ocr_telemetry(
        paths,
        trace_id,
        "info",
        None,
        stage,
        input_summary,
        message,
        0,
    );
}

#[cfg(not(test))]
fn write_ocr_telemetry(
    paths: &AppPaths,
    trace_id: &str,
    level: &str,
    error_code: Option<&str>,
    stage: &str,
    input_summary: String,
    message: String,
    duration_ms: u128,
) {
    let event = TelemetryEvent {
        timestamp: Utc::now().to_rfc3339(),
        trace_id: trace_id.to_string(),
        stage: stage.to_string(),
        input_summary,
        output_summary: format!("日志文件 {}", paths.app_log_path().display()),
        duration_ms,
        level: level.to_string(),
        error_code: error_code.map(|value| value.to_string()),
        message,
    };

    let _ = telemetry::append_event(paths, &event);
}
