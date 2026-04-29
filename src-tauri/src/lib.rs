mod apex;
mod app_paths;
mod calibration;
mod capture;
mod commands;
pub mod diagnostics;
mod live_client;
pub mod models;
mod ocr;
mod orchestrator;
mod overlay;
mod settings;
mod state_machine;
mod telemetry;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(orchestrator::RuntimeOrchestratorHandle::default())
        .setup(|app| {
            diagnostics::initialize(app.handle())
                .map_err(|error| Box::<dyn std::error::Error>::from(error))?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_runtime_overview,
            commands::run_health_check,
            commands::export_diagnostic_package,
            commands::export_release_package,
            commands::write_structured_log,
            commands::capture_monitor_sample,
            commands::save_calibration_profile,
            commands::load_calibration_profile,
            commands::check_ocr_resources,
            commands::run_ocr_text_replay,
            commands::run_calibrated_name_ocr,
            commands::fetch_live_client_active_player,
            commands::evaluate_state_machine,
            commands::get_runtime_orchestrator_status,
            commands::trigger_runtime_orchestrator,
            commands::start_runtime_listener,
            commands::stop_runtime_listener,
            commands::lookup_apex_lol,
            commands::build_apex_cache_report,
            commands::show_overlay_test_card,
            commands::hide_overlay_test_card
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
