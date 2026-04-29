mod app_paths;
mod commands;
mod diagnostics;
mod models;
mod settings;
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
            commands::write_structured_log
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
