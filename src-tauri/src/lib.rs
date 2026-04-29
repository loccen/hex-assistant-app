mod apex;
mod app_paths;
mod calibration;
mod capture;
mod commands;
mod diagnostics;
mod live_client;
mod models;
mod ocr;
mod overlay;
mod settings;
mod state_machine;
mod telemetry;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
            commands::list_capture_monitors,
            commands::save_calibration_profile,
            commands::save_pixel_calibration_profile,
            commands::load_calibration_profile,
            commands::check_ocr_resources,
            commands::run_ocr_text_replay,
            commands::fetch_live_client_active_player,
            commands::evaluate_state_machine,
            commands::lookup_apex_lol,
            commands::build_apex_cache_report,
            commands::show_overlay_test_card,
            commands::hide_overlay_test_card
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
