use crate::apex::{self, ApexCacheReport, ApexLookupRequest, ApexLookupResult};
use crate::calibration::{self, CalibrationConfig};
use crate::capture::{self, CaptureSampleReport};
use crate::diagnostics;
use crate::live_client::{ActivePlayerSnapshot, LiveClientDataApi};
use crate::models::{
    DiagnosticExportResult, HealthCheckReport, RuntimeOverview, TelemetryEvent, TelemetryEventInput,
};
use crate::ocr::{
    self, CalibratedNameOcrReport, CalibratedNameSlot, OfflineReplayReport, SlotReplayInput,
    AUGMENT_DICTIONARY_ZH_CN,
};
use crate::overlay::{self, OverlayOperationReport, OverlayTestCardRequest};
use crate::settings::load_or_create_settings;
use crate::state_machine::{AssistantState, AssistantStateMachine, StateMachineInput};
use crate::{app_paths::AppPaths, telemetry};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

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
pub fn load_calibration_profile(app: AppHandle) -> Result<CalibrationConfig, String> {
    let paths = AppPaths::from_app(&app)?;
    calibration::load_calibration_config(&paths.root)
}

#[tauri::command]
pub fn check_ocr_resources(app: AppHandle) -> Result<ocr::OcrResourceStatus, String> {
    Ok(ocr::check_ppocr_resources(resource_root(&app)))
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
    let dictionary_path = resource_root(&app)
        .join("dictionaries")
        .join(AUGMENT_DICTIONARY_ZH_CN);
    let dictionary =
        ocr::AugmentDictionary::load(&dictionary_path).map_err(|error| error.to_string())?;
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

#[tauri::command]
pub fn run_calibrated_name_ocr(
    app: AppHandle,
    input: Option<CalibratedNameOcrCommandInput>,
) -> Result<CalibratedNameOcrReport, String> {
    let paths = AppPaths::from_app(&app)?;
    paths.ensure_all()?;
    let settings = load_or_create_settings(&paths)?;
    let calibration = calibration::load_calibration_config(&paths.root)?;
    let resource_root = resource_root(&app);
    let resource_status = ocr::check_ppocr_resources(&resource_root);
    if !resource_status.ready {
        return Err(resource_status.message);
    }

    let dictionary_path = resource_root
        .join("dictionaries")
        .join(AUGMENT_DICTIONARY_ZH_CN);
    let dictionary =
        ocr::AugmentDictionary::load(&dictionary_path).map_err(|error| error.to_string())?;
    let mut recognizer = ocr::PpOcrV4RecRecognizer::from_resource_root(&resource_root)
        .map_err(|error| error.to_string())?;
    let input = input.unwrap_or(CalibratedNameOcrCommandInput {
        screenshot_path: None,
        preferred_monitor_id: None,
    });
    let screenshot_path = match input.screenshot_path {
        Some(path) => path,
        None => {
            let capture_report =
                capture::capture_monitor_sample(&paths.root, input.preferred_monitor_id)?;
            capture_report.png_path
        }
    };

    ocr::recognize_calibrated_name_slots_from_image(
        &mut recognizer,
        &dictionary,
        &calibration,
        &screenshot_path,
        &paths.reports,
        settings.ocr.min_confidence,
        settings.ocr.min_match_score,
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn fetch_live_client_active_player() -> Result<ActivePlayerSnapshot, String> {
    LiveClientDataApi::new()
        .fetch_active_player()
        .map_err(|error| error.to_string())
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
            failed_cache_ttl_minutes: 5,
        },
    )
}

#[tauri::command]
pub fn build_apex_cache_report(app: AppHandle) -> Result<ApexCacheReport, String> {
    let paths = AppPaths::from_app(&app)?;
    paths.ensure_all()?;
    apex::build_cache_report(&paths.cache)
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

fn resource_root(app: &AppHandle) -> std::path::PathBuf {
    app.path()
        .resource_dir()
        .ok()
        .filter(|path| path.exists())
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join("src-tauri")
                .join("resources")
        })
}
