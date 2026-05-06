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
mod resource_paths;
mod runtime_panel;
mod settings;
mod state_machine;
mod telemetry;

use crate::app_paths::AppPaths;
use serde::Serialize;
use tauri::menu::MenuBuilder;
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, WindowEvent};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrayExportStatus {
    status: String,
    zip_path: Option<String>,
    included_files: Option<usize>,
    message: String,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(orchestrator::RuntimeOrchestratorHandle::default())
        .setup(|app| {
            diagnostics::initialize(app.handle())
                .map_err(|error| Box::<dyn std::error::Error>::from(error))?;
            setup_tray(app)?;
            hide_main_window_when_calibrated(app);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_runtime_overview,
            commands::run_health_check,
            commands::export_diagnostic_package,
            commands::export_release_package,
            commands::write_structured_log,
            commands::capture_monitor_sample,
            commands::load_latest_capture_sample,
            commands::read_png_file_as_data_url,
            commands::list_capture_monitors,
            commands::save_calibration_profile,
            commands::save_pixel_calibration_profile,
            commands::load_calibration_profile,
            commands::check_ocr_resources,
            commands::run_ocr_text_replay,
            commands::run_calibrated_name_ocr,
            commands::run_pixel_calibrated_name_ocr,
            commands::run_name_region_ocr_precheck,
            commands::fetch_live_client_resolved_player_snapshot,
            commands::fetch_live_client_active_player,
            commands::evaluate_state_machine,
            commands::get_runtime_orchestrator_status,
            commands::trigger_runtime_orchestrator,
            commands::start_runtime_listener,
            commands::stop_runtime_listener,
            commands::lookup_apex_lol,
            commands::build_apex_cache_report,
            commands::show_overlay_test_card,
            commands::hide_overlay_test_card,
            commands::update_overlay_slots
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let menu = MenuBuilder::new(app)
        .text("open-assistant", "打开助手")
        .text("recalibrate", "重新校准")
        .text("export-diagnostic", "导出诊断")
        .separator()
        .text("quit", "退出")
        .build()?;
    let mut tray = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Northlight Panel")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open-assistant" => show_player_window(app),
            "recalibrate" => {
                show_player_window(app);
                let _ = app.emit("hex-assistant://recalibrate", ());
            }
            "export-diagnostic" => export_diagnostic_from_tray(app.clone()),
            "quit" => app.exit(0),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn show_player_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn export_diagnostic_from_tray(app: tauri::AppHandle) {
    let _ = app.emit(
        "hex-assistant://export-status",
        TrayExportStatus {
            status: "started".to_string(),
            zip_path: None,
            included_files: None,
            message: "正在导出资料包...".to_string(),
        },
    );
    std::thread::spawn(move || {
        let payload = match diagnostics::export_diagnostic_package(&app) {
            Ok(result) => TrayExportStatus {
                status: "completed".to_string(),
                zip_path: Some(result.zip_path.display().to_string()),
                included_files: Some(result.included_files),
                message: format!("资料包已导出：{}", result.zip_path.display()),
            },
            Err(error) => TrayExportStatus {
                status: "failed".to_string(),
                zip_path: None,
                included_files: None,
                message: format!("资料包导出失败：{error}"),
            },
        };
        let _ = app.emit("hex-assistant://export-status", payload);
        show_player_window(&app);
    });
}

fn hide_main_window_when_calibrated(app: &tauri::App) {
    let has_calibration = AppPaths::from_app(app.handle())
        .ok()
        .and_then(|paths| calibration::load_calibration_config(&paths.root).ok())
        .is_some();
    if has_calibration {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.hide();
        }
    }
}
