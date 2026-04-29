use crate::diagnostics;
use crate::models::{
    DiagnosticExportResult, HealthCheckReport, RuntimeOverview, TelemetryEvent, TelemetryEventInput,
};
use crate::{app_paths::AppPaths, telemetry};
use tauri::AppHandle;

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
pub fn write_structured_log(
    app: AppHandle,
    input: TelemetryEventInput,
) -> Result<TelemetryEvent, String> {
    let paths = AppPaths::from_app(&app)?;
    paths.ensure_all()?;
    telemetry::write_event(&paths, input)
}
