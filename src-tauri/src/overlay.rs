#![allow(dead_code)]

#[cfg(not(test))]
use crate::app_paths::AppPaths;
#[cfg(not(test))]
use crate::calibration::{self, CalibrationConfig, NormalizedPoint};
#[cfg(test)]
#[path = "calibration.rs"]
mod calibration;
#[cfg(test)]
use calibration::{CalibrationConfig, NormalizedPoint};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, Monitor, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

#[cfg(test)]
#[derive(Debug, Clone)]
struct AppPaths {
    root: PathBuf,
    logs: PathBuf,
}

#[cfg(test)]
impl AppPaths {
    fn from_app(_app: &AppHandle) -> Result<Self, String> {
        Err("测试环境不创建 Tauri 应用数据目录".to_string())
    }

    fn ensure_all(&self) -> Result<(), String> {
        Ok(())
    }
}

const OVERLAY_LABEL: &str = "hex-assistant-overlay";
const OVERLAY_URL: &str = "index.html?view=overlay";
const DEFAULT_CARD_WIDTH: u32 = 260;
const DEFAULT_CARD_HEIGHT: u32 = 118;
const DEFAULT_GAP: i32 = 18;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayTestCardRequest {
    pub monitor_name: Option<String>,
    #[serde(default = "default_anchor")]
    pub anchor: OverlayAnchor,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub gap: Option<i32>,
    #[serde(default = "default_click_through")]
    pub click_through: bool,
    #[serde(default)]
    pub slots: Vec<OverlaySlotData>,
}

impl Default for OverlayTestCardRequest {
    fn default() -> Self {
        Self {
            monitor_name: None,
            anchor: default_anchor(),
            width: Some(DEFAULT_CARD_WIDTH),
            height: Some(DEFAULT_CARD_HEIGHT),
            gap: Some(DEFAULT_GAP),
            click_through: true,
            slots: Vec::new(),
        }
    }
}

fn default_anchor() -> OverlayAnchor {
    OverlayAnchor::BottomRight
}

fn default_click_through() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OverlayAnchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySlotData {
    pub slot: u8,
    pub title: String,
    pub body: Option<String>,
    pub augment_id: Option<String>,
    pub rank: Option<String>,
    pub score: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayOperationReport {
    pub label: String,
    pub created: bool,
    pub visible: bool,
    pub creation: OverlayCreationParams,
    pub monitor: OverlayMonitorReport,
    pub bounds: OverlayBounds,
    pub logical_bounds: OverlayBounds,
    pub cards: Vec<OverlayCardInfo>,
    pub visibility_changes: Vec<OverlayVisibilityChange>,
    pub click_through: OverlayClickThroughReport,
    pub log_path: Option<PathBuf>,
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySlotUpdateReport {
    pub label: String,
    pub visible: bool,
    pub updated_slots: Vec<OverlaySlotData>,
    pub log_path: Option<PathBuf>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeOverlaySyncAction {
    Created,
    Updated,
    UpdatedAndShown,
    Hidden,
    AlreadyHidden,
    NoWindow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeOverlaySyncReport {
    pub label: String,
    pub action: RuntimeOverlaySyncAction,
    pub visible: bool,
    pub window_exists: bool,
    pub slot_count: usize,
    pub reason: String,
    pub log_path: Option<PathBuf>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayCreationParams {
    pub url: String,
    pub transparent: bool,
    pub background_color: String,
    pub always_on_top: bool,
    pub decorations: bool,
    pub skip_taskbar: bool,
    pub focused: bool,
    pub focusable: bool,
    pub resizable: bool,
    pub visible_on_create: bool,
}

impl Default for OverlayCreationParams {
    fn default() -> Self {
        Self {
            url: OVERLAY_URL.to_string(),
            transparent: cfg!(not(target_os = "macos")),
            background_color: "rgba(0,0,0,0)".to_string(),
            always_on_top: true,
            decorations: false,
            skip_taskbar: true,
            focused: false,
            focusable: false,
            resizable: false,
            visible_on_create: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayMonitorReport {
    pub name: Option<String>,
    pub scale_factor: String,
    pub position: OverlayPoint,
    pub size: OverlaySize,
    pub work_area: OverlayRect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayCardInfo {
    pub slot: u8,
    pub title: String,
    pub body: String,
    pub augment_id: Option<String>,
    pub rank: Option<String>,
    pub score: Option<String>,
    pub bounds: OverlayBounds,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayVisibilityChange {
    pub from: bool,
    pub to: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayClickThroughReport {
    pub requested: bool,
    pub platform: String,
    pub status: OverlayClickThroughStatus,
    pub message: String,
    pub child_window_results: Vec<OverlayChildWindowResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayChildWindowResult {
    pub phase: String,
    pub delay_ms: Option<u64>,
    pub applied_count: usize,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OverlayClickThroughStatus {
    Applied,
    PendingManualAcceptance,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayError {
    NoMonitor,
    MonitorNotFound(String),
    InvalidSize {
        width: u32,
        height: u32,
    },
    OutOfBounds {
        bounds: OverlayBounds,
        work_area: OverlayRect,
    },
    Tauri(String),
}

impl std::fmt::Display for OverlayError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoMonitor => write!(formatter, "HEX-OVERLAY-NO-MONITOR: 未找到可用显示器"),
            Self::MonitorNotFound(name) => {
                write!(
                    formatter,
                    "HEX-OVERLAY-MONITOR-MISSING: 未找到目标显示器 {name}"
                )
            }
            Self::InvalidSize { width, height } => {
                write!(
                    formatter,
                    "HEX-OVERLAY-BAD-SIZE: Overlay 尺寸无效 {width}x{height}"
                )
            }
            Self::OutOfBounds { bounds, work_area } => write!(
                formatter,
                "HEX-OVERLAY-OUT-OF-BOUNDS: bounds=({}, {}, {}x{}), workArea=({}, {}, {}x{})",
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
                work_area.x,
                work_area.y,
                work_area.width,
                work_area.height
            ),
            Self::Tauri(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for OverlayError {}

pub fn show_overlay_test_card(
    app: AppHandle,
    request: OverlayTestCardRequest,
) -> Result<OverlayOperationReport, String> {
    show_overlay_test_card_inner(&app, request).map_err(|error| error.to_string())
}

pub fn hide_overlay_test_card(app: AppHandle) -> Result<OverlayOperationReport, String> {
    hide_overlay_test_card_inner(&app).map_err(|error| error.to_string())
}

pub fn update_overlay_slots(
    app: AppHandle,
    slots: Vec<OverlaySlotData>,
) -> Result<OverlaySlotUpdateReport, String> {
    update_overlay_slots_inner(&app, slots).map_err(|error| error.to_string())
}

pub fn sync_runtime_overlay_inner(
    app: &AppHandle,
    slots: Vec<OverlaySlotData>,
    reason: &str,
) -> Result<RuntimeOverlaySyncReport, OverlayError> {
    let paths = prepare_paths(app)?;
    overlay_debug_log(
        &paths,
        &format!(
            "[runtime_sync] requested action=show reason={} slots={}",
            reason,
            slots
                .iter()
                .map(|slot| format!("{}:{}", slot.slot, slot.title))
                .collect::<Vec<_>>()
                .join("; ")
        ),
    );

    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        overlay_debug_log(&paths, "[runtime_sync] existing_window dispatch_start");
        dispatch_overlay_slots(&window, &slots)?;
        overlay_debug_log(&paths, "[runtime_sync] existing_window dispatch_done");
        overlay_debug_log(&paths, "[runtime_sync] existing_window visibility_check_start");
        let was_visible = window.is_visible().map_err(|error| {
            OverlayError::Tauri(format!("HEX-OVERLAY-VISIBILITY-FAILED: {error}"))
        })?;
        overlay_debug_log(
            &paths,
            &format!("[runtime_sync] existing_window visibility_check_done visible={was_visible}"),
        );
        if was_visible {
            overlay_debug_log(
                &paths,
                &format!(
                    "[runtime_sync] action=updated reason={} slot_count={}",
                    reason,
                    slots.len()
                ),
            );
            return Ok(RuntimeOverlaySyncReport {
                label: OVERLAY_LABEL.to_string(),
                action: RuntimeOverlaySyncAction::Updated,
                visible: true,
                window_exists: true,
                slot_count: slots.len(),
                reason: reason.to_string(),
                log_path: Some(paths.overlay_debug_log_path()),
                message: "Overlay 已更新实时 slot 数据。".to_string(),
            });
        }

        overlay_debug_log(&paths, "[runtime_sync] existing_window show_start");
        window.show().map_err(|error| {
            OverlayError::Tauri(format!(
                "HEX-OVERLAY-SHOW-FAILED: 显示 Overlay 失败: {error}"
            ))
        })?;
        overlay_debug_log(&paths, "[runtime_sync] existing_window show_done");
        overlay_debug_log(
            &paths,
            &format!(
                "[runtime_sync] action=updated_and_shown reason={} slot_count={}",
                reason,
                slots.len()
            ),
        );
        return Ok(RuntimeOverlaySyncReport {
            label: OVERLAY_LABEL.to_string(),
            action: RuntimeOverlaySyncAction::UpdatedAndShown,
            visible: true,
            window_exists: true,
            slot_count: slots.len(),
            reason: reason.to_string(),
            log_path: Some(paths.overlay_debug_log_path()),
            message: "Overlay 已恢复显示并更新实时 slot 数据。".to_string(),
        });
    }

    let report = show_overlay_test_card_inner(
        app,
        OverlayTestCardRequest {
            slots,
            ..OverlayTestCardRequest::default()
        },
    )?;
    overlay_debug_log(
        &paths,
        &format!(
            "[runtime_sync] action=created reason={} slot_count={}",
            reason,
            report.cards.len()
        ),
    );
    Ok(RuntimeOverlaySyncReport {
        label: OVERLAY_LABEL.to_string(),
        action: RuntimeOverlaySyncAction::Created,
        visible: true,
        window_exists: true,
        slot_count: report.cards.len(),
        reason: reason.to_string(),
        log_path: Some(paths.overlay_debug_log_path()),
        message: "Overlay 已按实时数据自动创建。".to_string(),
    })
}

pub fn hide_runtime_overlay_inner(
    app: &AppHandle,
    reason: &str,
) -> Result<RuntimeOverlaySyncReport, OverlayError> {
    let paths = prepare_paths(app)?;
    overlay_debug_log(
        &paths,
        &format!("[runtime_sync] requested action=hide reason={reason}"),
    );

    let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
        overlay_debug_log(
            &paths,
            &format!("[runtime_sync] action=no_window reason={reason}"),
        );
        return Ok(RuntimeOverlaySyncReport {
            label: OVERLAY_LABEL.to_string(),
            action: RuntimeOverlaySyncAction::NoWindow,
            visible: false,
            window_exists: false,
            slot_count: 0,
            reason: reason.to_string(),
            log_path: Some(paths.overlay_debug_log_path()),
            message: "Overlay 窗口不存在，无需隐藏。".to_string(),
        });
    };

    let was_visible = window
        .is_visible()
        .map_err(|error| OverlayError::Tauri(format!("HEX-OVERLAY-VISIBILITY-FAILED: {error}")))?;
    if !was_visible {
        overlay_debug_log(
            &paths,
            &format!("[runtime_sync] action=already_hidden reason={reason}"),
        );
        return Ok(RuntimeOverlaySyncReport {
            label: OVERLAY_LABEL.to_string(),
            action: RuntimeOverlaySyncAction::AlreadyHidden,
            visible: false,
            window_exists: true,
            slot_count: 0,
            reason: reason.to_string(),
            log_path: Some(paths.overlay_debug_log_path()),
            message: "Overlay 已处于隐藏状态。".to_string(),
        });
    }

    window
        .hide()
        .map_err(|error| OverlayError::Tauri(format!("HEX-OVERLAY-HIDE-FAILED: {error}")))?;
    overlay_debug_log(
        &paths,
        &format!("[runtime_sync] action=hidden reason={reason}"),
    );
    Ok(RuntimeOverlaySyncReport {
        label: OVERLAY_LABEL.to_string(),
        action: RuntimeOverlaySyncAction::Hidden,
        visible: false,
        window_exists: true,
        slot_count: 0,
        reason: reason.to_string(),
        log_path: Some(paths.overlay_debug_log_path()),
        message: "Overlay 已按运行时状态自动隐藏。".to_string(),
    })
}

pub fn show_overlay_test_card_inner(
    app: &AppHandle,
    request: OverlayTestCardRequest,
) -> Result<OverlayOperationReport, OverlayError> {
    let start = std::time::Instant::now();
    let paths = prepare_paths(app)?;
    let mut messages = Vec::new();
    overlay_debug_log(&paths, "[show] 开始创建 Overlay");

    let monitor = select_monitor(app, request.monitor_name.as_deref())?;
    let monitor_report = monitor_to_report(&monitor);
    let physical_bounds = monitor_bounds(&monitor);
    let logical_bounds = monitor_logical_bounds(&monitor, physical_bounds);
    overlay_debug_log(
        &paths,
        &format!(
            "[show] target_monitor name={:?} scale={} physical={}x{}@{},{} workArea={}x{}@{},{} logical={}x{}@{},{}",
            monitor_report.name,
            monitor_report.scale_factor,
            physical_bounds.width,
            physical_bounds.height,
            physical_bounds.x,
            physical_bounds.y,
            monitor_report.work_area.width,
            monitor_report.work_area.height,
            monitor_report.work_area.x,
            monitor_report.work_area.y,
            logical_bounds.width,
            logical_bounds.height,
            logical_bounds.x,
            logical_bounds.y,
        ),
    );

    let calibration = match calibration::load_calibration_config(&paths.root) {
        Ok(config) => {
            messages.push("已读取校准配置，使用底部锚点生成三张 Overlay 卡片。".to_string());
            overlay_debug_log(&paths, "[show] calibration=loaded");
            Some(config)
        }
        Err(error) => {
            messages.push(format!("未读取到校准配置，使用静态三列测试位置：{error}"));
            overlay_debug_log(
                &paths,
                &format!("[show] calibration=fallback error={error}"),
            );
            None
        }
    };

    let card_width = request.width.unwrap_or(DEFAULT_CARD_WIDTH);
    let card_height = request.height.unwrap_or(DEFAULT_CARD_HEIGHT);
    let gap = request.gap.unwrap_or(DEFAULT_GAP);
    let cards = plan_overlay_cards(
        logical_bounds,
        calibration.as_ref(),
        &request.slots,
        card_width,
        card_height,
        gap,
    )?;
    overlay_debug_log(
        &paths,
        &format!(
            "[show] cards={}",
            cards
                .iter()
                .map(|card| format!(
                    "slot{}:{}x{}@{},{}:{}",
                    card.slot,
                    card.bounds.width,
                    card.bounds.height,
                    card.bounds.x,
                    card.bounds.y,
                    card.source
                ))
                .collect::<Vec<_>>()
                .join("; ")
        ),
    );

    if let Some(existing) = app.get_webview_window(OVERLAY_LABEL) {
        overlay_debug_log(&paths, "[show] 关闭旧 Overlay 窗口");
        existing
            .close()
            .map_err(|error| OverlayError::Tauri(format!("HEX-OVERLAY-CLOSE-FAILED: {error}")))?;
    }

    let payload = OverlayPagePayload {
        generated_at: Utc::now().to_rfc3339(),
        mode: if request.slots.is_empty() {
            "static".to_string()
        } else {
            "slotData".to_string()
        },
        cards: cards.clone(),
    };
    let window = match build_overlay_window(app, &monitor, logical_bounds, &payload) {
        Ok(window) => window,
        Err(error) => {
            overlay_debug_log(&paths, &format!("[show] build_failed {error}"));
            return Err(error);
        }
    };

    if let Err(error) = apply_overlay_geometry(&window, &monitor, physical_bounds) {
        hide_after_error(&paths, &window, "geometry_failed");
        return Err(error);
    }

    if let Err(error) = window.set_focusable(false) {
        messages.push(format!("设置 Overlay 不抢焦点失败：{error}"));
        overlay_debug_log(&paths, &format!("[show] set_focusable_failed {error}"));
    }

    #[cfg(windows)]
    {
        if let Some(result) = align_window_client_to_requested_bounds(&window) {
            overlay_debug_log(
                &paths,
                &format!(
                    "[show] non_client_align new_outer={},{} offset={},{}",
                    result.0, result.1, result.2, result.3
                ),
            );
        } else {
            overlay_debug_log(&paths, "[show] non_client_align skipped_or_failed");
        }
    }

    let click_through = apply_click_through(&paths, &window, request.click_through);
    overlay_debug_log(
        &paths,
        &format!(
            "[show] click_through requested={} status={:?} message={}",
            click_through.requested, click_through.status, click_through.message
        ),
    );

    overlay_debug_log(&paths, "[show] visibility false->true reason=geometryReady");
    if let Err(error) = window.show() {
        hide_after_error(&paths, &window, "show_failed");
        return Err(OverlayError::Tauri(format!(
            "HEX-OVERLAY-SHOW-FAILED: 显示 Overlay 失败: {error}"
        )));
    }

    overlay_debug_log(
        &paths,
        &format!("[show] done duration_ms={}", start.elapsed().as_millis()),
    );

    Ok(OverlayOperationReport {
        label: OVERLAY_LABEL.to_string(),
        created: true,
        visible: true,
        creation: OverlayCreationParams::default(),
        monitor: monitor_report,
        bounds: physical_bounds,
        logical_bounds,
        cards,
        visibility_changes: vec![OverlayVisibilityChange {
            from: false,
            to: true,
            reason: "geometryReady".to_string(),
        }],
        click_through,
        log_path: Some(paths.overlay_debug_log_path()),
        messages,
    })
}

pub fn hide_overlay_test_card_inner(
    app: &AppHandle,
) -> Result<OverlayOperationReport, OverlayError> {
    let paths = prepare_paths(app)?;
    overlay_debug_log(&paths, "[hide] requested");
    let monitor = select_monitor(app, None)?;
    let monitor_report = monitor_to_report(&monitor);
    let physical_bounds = monitor_bounds(&monitor);
    let logical_bounds = monitor_logical_bounds(&monitor, physical_bounds);
    let mut messages = Vec::new();

    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        window
            .hide()
            .map_err(|error| OverlayError::Tauri(format!("HEX-OVERLAY-HIDE-FAILED: {error}")))?;
        overlay_debug_log(&paths, "[hide] visibility true->false reason=manualHide");
        messages.push("Overlay 已隐藏。".to_string());
    } else {
        overlay_debug_log(&paths, "[hide] window_not_found");
        messages.push("Overlay 窗口不存在，无需隐藏。".to_string());
    }

    Ok(OverlayOperationReport {
        label: OVERLAY_LABEL.to_string(),
        created: false,
        visible: false,
        creation: OverlayCreationParams::default(),
        monitor: monitor_report,
        bounds: physical_bounds,
        logical_bounds,
        cards: Vec::new(),
        visibility_changes: vec![OverlayVisibilityChange {
            from: true,
            to: false,
            reason: "manualHide".to_string(),
        }],
        click_through: platform_pending_click_through(false),
        log_path: Some(paths.overlay_debug_log_path()),
        messages,
    })
}

pub fn update_overlay_slots_inner(
    app: &AppHandle,
    slots: Vec<OverlaySlotData>,
) -> Result<OverlaySlotUpdateReport, OverlayError> {
    let paths = prepare_paths(app)?;
    overlay_debug_log(
        &paths,
        &format!(
            "[update_slots] requested slots={}",
            slots
                .iter()
                .map(|slot| format!("{}:{}", slot.slot, slot.title))
                .collect::<Vec<_>>()
                .join("; ")
        ),
    );

    let window = app.get_webview_window(OVERLAY_LABEL).ok_or_else(|| {
        OverlayError::Tauri(
            "HEX-OVERLAY-NOT-VISIBLE: Overlay 窗口不存在，请先显示静态测试卡片。".to_string(),
        )
    })?;
    dispatch_overlay_slots(&window, &slots)?;
    overlay_debug_log(&paths, "[update_slots] eval_dispatched");

    Ok(OverlaySlotUpdateReport {
        label: OVERLAY_LABEL.to_string(),
        visible: true,
        updated_slots: slots,
        log_path: Some(paths.overlay_debug_log_path()),
        message: "已向 Overlay 窗口发送真实 slot 数据更新。".to_string(),
    })
}

fn dispatch_overlay_slots(
    window: &WebviewWindow,
    slots: &[OverlaySlotData],
) -> Result<(), OverlayError> {
    let payload = serde_json::to_string(slots)
        .map_err(|error| OverlayError::Tauri(format!("HEX-OVERLAY-SERIALIZE-FAILED: {error}")))?;
    let script = format!(
        "window.dispatchEvent(new CustomEvent('hex-overlay-slots', {{ detail: {} }}));",
        payload
    );
    window.eval(script).map_err(|error| {
        OverlayError::Tauri(format!(
            "HEX-OVERLAY-UPDATE-FAILED: 更新 slot 数据失败: {error}"
        ))
    })?;
    Ok(())
}

pub fn plan_overlay_bounds(
    work_area: OverlayRect,
    anchor: OverlayAnchor,
    width: u32,
    height: u32,
    gap: i32,
) -> Result<OverlayBounds, OverlayError> {
    if width == 0 || height == 0 {
        return Err(OverlayError::InvalidSize { width, height });
    }

    let width_i32 =
        i32::try_from(width).map_err(|_| OverlayError::InvalidSize { width, height })?;
    let height_i32 =
        i32::try_from(height).map_err(|_| OverlayError::InvalidSize { width, height })?;
    let work_width = i32::try_from(work_area.width).map_err(|_| OverlayError::OutOfBounds {
        bounds: OverlayBounds {
            x: work_area.x,
            y: work_area.y,
            width,
            height,
        },
        work_area,
    })?;
    let work_height = i32::try_from(work_area.height).map_err(|_| OverlayError::OutOfBounds {
        bounds: OverlayBounds {
            x: work_area.x,
            y: work_area.y,
            width,
            height,
        },
        work_area,
    })?;

    let (x, y) = match anchor {
        OverlayAnchor::TopLeft => (work_area.x + gap, work_area.y + gap),
        OverlayAnchor::TopRight => (
            work_area.x + work_width - width_i32 - gap,
            work_area.y + gap,
        ),
        OverlayAnchor::BottomLeft => (
            work_area.x + gap,
            work_area.y + work_height - height_i32 - gap,
        ),
        OverlayAnchor::BottomRight => (
            work_area.x + work_width - width_i32 - gap,
            work_area.y + work_height - height_i32 - gap,
        ),
    };

    let bounds = OverlayBounds {
        x,
        y,
        width,
        height,
    };
    ensure_bounds_inside_work_area(bounds, work_area)?;
    Ok(bounds)
}

fn plan_overlay_cards(
    target: OverlayBounds,
    calibration: Option<&CalibrationConfig>,
    slot_data: &[OverlaySlotData],
    requested_width: u32,
    requested_height: u32,
    gap: i32,
) -> Result<Vec<OverlayCardInfo>, OverlayError> {
    if requested_width == 0 || requested_height == 0 {
        return Err(OverlayError::InvalidSize {
            width: requested_width,
            height: requested_height,
        });
    }

    let width = requested_width.min(target.width.max(1));
    let height = requested_height.min(target.height.max(1));
    let anchors = calibration
        .map(|config| config.bottom_anchors.to_vec())
        .unwrap_or_else(default_bottom_anchors);
    let source = if calibration.is_some() {
        "calibration.bottomAnchors"
    } else {
        "fallback.staticAnchors"
    };

    anchors
        .iter()
        .enumerate()
        .map(|(index, anchor)| {
            let slot = u8::try_from(index + 1).unwrap_or(3);
            let slot_payload = slot_data.iter().find(|candidate| candidate.slot == slot);
            let bounds = card_bounds_from_anchor(target, *anchor, width, height, gap)?;
            Ok(OverlayCardInfo {
                slot,
                title: slot_payload
                    .map(|payload| payload.title.clone())
                    .unwrap_or_else(|| format!("测试卡片 {slot}")),
                body: slot_payload
                    .and_then(|payload| payload.body.clone())
                    .unwrap_or_else(|| "透明置顶点击穿透验证卡片".to_string()),
                augment_id: slot_payload.and_then(|payload| payload.augment_id.clone()),
                rank: slot_payload.and_then(|payload| payload.rank.clone()),
                score: slot_payload.and_then(|payload| payload.score.clone()),
                bounds,
                source: source.to_string(),
            })
        })
        .collect()
}

fn card_bounds_from_anchor(
    target: OverlayBounds,
    anchor: NormalizedPoint,
    width: u32,
    height: u32,
    gap: i32,
) -> Result<OverlayBounds, OverlayError> {
    let target_width = f64::from(target.width);
    let target_height = f64::from(target.height);
    let anchor_x = (anchor.x * target_width).round() as i32;
    let anchor_y = (anchor.y * target_height).round() as i32;
    let width_i32 =
        i32::try_from(width).map_err(|_| OverlayError::InvalidSize { width, height })?;
    let height_i32 =
        i32::try_from(height).map_err(|_| OverlayError::InvalidSize { width, height })?;
    let max_x = i32::try_from(target.width.saturating_sub(width)).unwrap_or(i32::MAX);
    let max_y = i32::try_from(target.height.saturating_sub(height)).unwrap_or(i32::MAX);

    let x = (anchor_x - width_i32 / 2).clamp(0, max_x);
    let y = (anchor_y - height_i32 - gap).clamp(0, max_y);
    let bounds = OverlayBounds {
        x,
        y,
        width,
        height,
    };
    ensure_bounds_inside_work_area(
        bounds,
        OverlayRect {
            x: 0,
            y: 0,
            width: target.width,
            height: target.height,
        },
    )?;
    Ok(bounds)
}

fn default_bottom_anchors() -> Vec<NormalizedPoint> {
    vec![
        NormalizedPoint { x: 0.28, y: 0.84 },
        NormalizedPoint { x: 0.50, y: 0.84 },
        NormalizedPoint { x: 0.72, y: 0.84 },
    ]
}

fn ensure_bounds_inside_work_area(
    bounds: OverlayBounds,
    work_area: OverlayRect,
) -> Result<(), OverlayError> {
    let right = bounds.x + i32::try_from(bounds.width).unwrap_or(i32::MAX);
    let bottom = bounds.y + i32::try_from(bounds.height).unwrap_or(i32::MAX);
    let work_right = work_area.x + i32::try_from(work_area.width).unwrap_or(i32::MAX);
    let work_bottom = work_area.y + i32::try_from(work_area.height).unwrap_or(i32::MAX);

    if bounds.x < work_area.x
        || bounds.y < work_area.y
        || right > work_right
        || bottom > work_bottom
    {
        return Err(OverlayError::OutOfBounds { bounds, work_area });
    }

    Ok(())
}

fn select_monitor(app: &AppHandle, monitor_name: Option<&str>) -> Result<Monitor, OverlayError> {
    let monitors = app
        .available_monitors()
        .map_err(|error| OverlayError::Tauri(format!("读取显示器列表失败: {error}")))?;

    if let Some(name) = monitor_name {
        return monitors
            .into_iter()
            .find(|monitor| monitor.name().is_some_and(|candidate| candidate == name))
            .ok_or_else(|| OverlayError::MonitorNotFound(name.to_string()));
    }

    app.primary_monitor()
        .map_err(|error| OverlayError::Tauri(format!("读取主显示器失败: {error}")))?
        .or_else(|| monitors.into_iter().next())
        .ok_or(OverlayError::NoMonitor)
}

fn build_overlay_window(
    app: &AppHandle,
    monitor: &Monitor,
    logical_bounds: OverlayBounds,
    payload: &OverlayPagePayload,
) -> Result<WebviewWindow, OverlayError> {
    let bootstrap = serde_json::to_string(payload)
        .map_err(|error| OverlayError::Tauri(format!("序列化 Overlay 初始数据失败: {error}")))?;
    let mut builder =
        WebviewWindowBuilder::new(app, OVERLAY_LABEL, WebviewUrl::App(OVERLAY_URL.into()))
            .title("Hex Assistant Overlay")
            .always_on_top(true)
            .decorations(false)
            .skip_taskbar(true)
            .focused(false)
            .focusable(false)
            .resizable(false)
            .visible(false)
            .shadow(false)
            .accept_first_mouse(false)
            .position(f64::from(logical_bounds.x), f64::from(logical_bounds.y))
            .inner_size(
                f64::from(logical_bounds.width),
                f64::from(logical_bounds.height),
            )
            .initialization_script(format!("window.__HEX_OVERLAY_BOOTSTRAP__ = {};", bootstrap));

    #[cfg(not(target_os = "macos"))]
    {
        builder = builder
            .transparent(true)
            .background_color(tauri::window::Color(0, 0, 0, 0));
    }

    let _ = monitor;
    builder
        .build()
        .map_err(|error| OverlayError::Tauri(format!("HEX-OVERLAY-BUILD-FAILED: {error}")))
}

fn apply_overlay_geometry(
    window: &WebviewWindow,
    monitor: &Monitor,
    physical_bounds: OverlayBounds,
) -> Result<(), OverlayError> {
    let (logical_x, logical_y, logical_width, logical_height) =
        logical_geometry(monitor, physical_bounds);
    window
        .set_position(LogicalPosition::new(logical_x, logical_y))
        .map_err(|error| OverlayError::Tauri(format!("HEX-OVERLAY-POSITION-FAILED: {error}")))?;
    window
        .set_size(LogicalSize::new(logical_width, logical_height))
        .map_err(|error| OverlayError::Tauri(format!("HEX-OVERLAY-SIZE-FAILED: {error}")))?;
    Ok(())
}

fn logical_geometry(monitor: &Monitor, bounds: OverlayBounds) -> (f64, f64, f64, f64) {
    let scale_factor = monitor.scale_factor();
    (
        f64::from(bounds.x) / scale_factor,
        f64::from(bounds.y) / scale_factor,
        f64::from(bounds.width) / scale_factor,
        f64::from(bounds.height) / scale_factor,
    )
}

fn monitor_bounds(monitor: &Monitor) -> OverlayBounds {
    OverlayBounds {
        x: monitor.position().x,
        y: monitor.position().y,
        width: monitor.size().width,
        height: monitor.size().height,
    }
}

fn monitor_logical_bounds(monitor: &Monitor, physical_bounds: OverlayBounds) -> OverlayBounds {
    let (x, y, width, height) = logical_geometry(monitor, physical_bounds);
    OverlayBounds {
        x: x.round() as i32,
        y: y.round() as i32,
        width: width.round().max(1.0) as u32,
        height: height.round().max(1.0) as u32,
    }
}

#[cfg(windows)]
fn apply_click_through(
    paths: &AppPaths,
    window: &WebviewWindow,
    requested: bool,
) -> OverlayClickThroughReport {
    match window.set_ignore_cursor_events(requested) {
        Ok(()) => {
            let mut child_window_results = Vec::new();
            if requested {
                if let Some(hwnd) = window_hwnd(window) {
                    let immediate = enumerate_children_transparent(hwnd);
                    overlay_debug_log(
                        paths,
                        &format!(
                            "[click_through] outer=0x{:x} immediate_count={} details=[{}]",
                            hwnd,
                            immediate.len(),
                            immediate.join(", ")
                        ),
                    );
                    child_window_results.push(OverlayChildWindowResult {
                        phase: "immediate".to_string(),
                        delay_ms: None,
                        applied_count: immediate.len(),
                        details: immediate,
                    });
                    schedule_click_through_retries(paths.clone(), hwnd);
                } else {
                    overlay_debug_log(paths, "[click_through] window_hwnd_missing");
                }
            }
            OverlayClickThroughReport {
                requested,
                platform: std::env::consts::OS.to_string(),
                status: OverlayClickThroughStatus::Applied,
                message:
                    "set_ignore_cursor_events 已执行；WebView2 子窗口立即补穿透并安排延迟重试。"
                        .to_string(),
                child_window_results,
            }
        }
        Err(error) => OverlayClickThroughReport {
            requested,
            platform: std::env::consts::OS.to_string(),
            status: OverlayClickThroughStatus::Failed,
            message: format!("set_ignore_cursor_events 执行失败: {error}"),
            child_window_results: Vec::new(),
        },
    }
}

#[cfg(not(windows))]
fn apply_click_through(
    _paths: &AppPaths,
    _window: &WebviewWindow,
    requested: bool,
) -> OverlayClickThroughReport {
    platform_pending_click_through(requested)
}

fn platform_pending_click_through(requested: bool) -> OverlayClickThroughReport {
    OverlayClickThroughReport {
        requested,
        platform: std::env::consts::OS.to_string(),
        status: OverlayClickThroughStatus::PendingManualAcceptance,
        message: "非 Windows 平台暂不执行 set_ignore_cursor_events，待 Windows 桌面验收"
            .to_string(),
        child_window_results: Vec::new(),
    }
}

fn monitor_to_report(monitor: &Monitor) -> OverlayMonitorReport {
    OverlayMonitorReport {
        name: monitor.name().cloned(),
        scale_factor: monitor.scale_factor().to_string(),
        position: OverlayPoint {
            x: monitor.position().x,
            y: monitor.position().y,
        },
        size: OverlaySize {
            width: monitor.size().width,
            height: monitor.size().height,
        },
        work_area: OverlayRect {
            x: monitor.work_area().position.x,
            y: monitor.work_area().position.y,
            width: monitor.work_area().size.width,
            height: monitor.work_area().size.height,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct OverlayPagePayload {
    generated_at: String,
    mode: String,
    cards: Vec<OverlayCardInfo>,
}

fn prepare_paths(app: &AppHandle) -> Result<AppPaths, OverlayError> {
    let paths = AppPaths::from_app(app).map_err(OverlayError::Tauri)?;
    paths.ensure_all().map_err(OverlayError::Tauri)?;
    Ok(paths)
}

fn hide_after_error(paths: &AppPaths, window: &WebviewWindow, reason: &str) {
    let _ = window.hide();
    overlay_debug_log(
        paths,
        &format!("[error] visibility true->false reason={reason}"),
    );
}

fn overlay_debug_log(paths: &AppPaths, line: &str) {
    let path = paths.overlay_debug_log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut file = match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => file,
        Err(_) => return,
    };
    let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
    let _ = writeln!(file, "[{timestamp}] {line}");
}

trait OverlayLogPath {
    fn overlay_debug_log_path(&self) -> PathBuf;
}

impl OverlayLogPath for AppPaths {
    fn overlay_debug_log_path(&self) -> PathBuf {
        self.logs.join("overlay-debug.log")
    }
}

#[cfg(windows)]
mod win32_ffi {
    extern "system" {
        pub fn GetWindowLongPtrW(hwnd: isize, n_index: i32) -> isize;
        pub fn SetWindowLongPtrW(hwnd: isize, n_index: i32, dw_new_long: isize) -> isize;
        pub fn EnumChildWindows(
            hwnd_parent: isize,
            lp_enum_func: unsafe extern "system" fn(isize, isize) -> i32,
            l_param: isize,
        ) -> i32;
        pub fn IsWindow(hwnd: isize) -> i32;
        pub fn SetWindowPos(
            hwnd: isize,
            hwnd_insert_after: isize,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            u_flags: u32,
        ) -> i32;
        pub fn GetWindowRect(hwnd: isize, lp_rect: *mut Rect) -> i32;
        pub fn ClientToScreen(hwnd: isize, lp_point: *mut Point) -> i32;
        pub fn GetClassNameW(hwnd: isize, lp_class_name: *mut u16, n_max_count: i32) -> i32;
    }

    #[repr(C)]
    pub struct Rect {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }

    #[repr(C)]
    pub struct Point {
        pub x: i32,
        pub y: i32,
    }

    pub const GWL_EXSTYLE: i32 = -20;
    pub const WS_EX_TRANSPARENT: isize = 0x00000020;
    pub const SWP_NOZORDER: u32 = 0x0004;
    pub const SWP_NOACTIVATE: u32 = 0x0010;
}

#[cfg(windows)]
fn window_hwnd(window: &WebviewWindow) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return None;
    };
    Some(handle.hwnd.get() as isize)
}

#[cfg(windows)]
fn align_window_client_to_requested_bounds(window: &WebviewWindow) -> Option<(i32, i32, i32, i32)> {
    let hwnd = window_hwnd(window)?;
    let mut outer = win32_ffi::Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let mut origin = win32_ffi::Point { x: 0, y: 0 };
    unsafe {
        if win32_ffi::GetWindowRect(hwnd, &mut outer) == 0
            || win32_ffi::ClientToScreen(hwnd, &mut origin) == 0
        {
            return None;
        }
        let nc_left = origin.x - outer.left;
        let nc_top = origin.y - outer.top;
        let outer_width = outer.right - outer.left;
        let outer_height = outer.bottom - outer.top;
        let new_x = outer.left - nc_left;
        let new_y = outer.top - nc_top;
        let _ = win32_ffi::SetWindowPos(
            hwnd,
            0,
            new_x,
            new_y,
            outer_width,
            outer_height,
            win32_ffi::SWP_NOZORDER | win32_ffi::SWP_NOACTIVATE,
        );
        Some((new_x, new_y, nc_left, nc_top))
    }
}

#[cfg(windows)]
fn is_window_alive(hwnd: isize) -> bool {
    unsafe { win32_ffi::IsWindow(hwnd) != 0 }
}

#[cfg(windows)]
fn get_class_name(hwnd: isize) -> String {
    use std::os::windows::ffi::OsStringExt;
    let mut buffer = [0u16; 256];
    let length =
        unsafe { win32_ffi::GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    if length <= 0 {
        return "unknown".to_string();
    }
    std::ffi::OsString::from_wide(&buffer[..length as usize])
        .to_string_lossy()
        .into_owned()
}

#[cfg(windows)]
unsafe extern "system" fn make_child_transparent(child_hwnd: isize, lparam: isize) -> i32 {
    let collector = unsafe { &mut *(lparam as *mut Vec<String>) };
    let style = unsafe { win32_ffi::GetWindowLongPtrW(child_hwnd, win32_ffi::GWL_EXSTYLE) };
    unsafe {
        win32_ffi::SetWindowLongPtrW(
            child_hwnd,
            win32_ffi::GWL_EXSTYLE,
            style | win32_ffi::WS_EX_TRANSPARENT,
        );
    }
    collector.push(format!("{}:0x{:x}", get_class_name(child_hwnd), child_hwnd));
    1
}

#[cfg(windows)]
fn enumerate_children_transparent(hwnd: isize) -> Vec<String> {
    let mut collector: Vec<String> = Vec::new();
    let lparam = (&mut collector as *mut Vec<String>) as isize;
    unsafe {
        win32_ffi::EnumChildWindows(hwnd, make_child_transparent, lparam);
    }
    collector
}

#[cfg(windows)]
fn schedule_click_through_retries(paths: AppPaths, hwnd: isize) {
    std::thread::spawn(move || {
        for delay_ms in [200u64, 700, 2000] {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            if !is_window_alive(hwnd) {
                overlay_debug_log(
                    &paths,
                    &format!("[click_through_retry] delay_ms={delay_ms} skipped window_closed"),
                );
                return;
            }
            let details = enumerate_children_transparent(hwnd);
            overlay_debug_log(
                &paths,
                &format!(
                    "[click_through_retry] delay_ms={} applied_count={} details=[{}]",
                    delay_ms,
                    details.len(),
                    details.join(", ")
                ),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_overlay_bounds_generates_top_right_coordinates() {
        let work_area = OverlayRect {
            x: 100,
            y: 50,
            width: 1920,
            height: 1040,
        };

        let bounds = plan_overlay_bounds(work_area, OverlayAnchor::TopRight, 360, 96, 24).unwrap();

        assert_eq!(
            bounds,
            OverlayBounds {
                x: 1636,
                y: 74,
                width: 360,
                height: 96,
            }
        );
    }

    #[test]
    fn plan_overlay_bounds_generates_bottom_left_coordinates() {
        let work_area = OverlayRect {
            x: -1280,
            y: 0,
            width: 1280,
            height: 720,
        };

        let bounds =
            plan_overlay_bounds(work_area, OverlayAnchor::BottomLeft, 320, 88, 12).unwrap();

        assert_eq!(
            bounds,
            OverlayBounds {
                x: -1268,
                y: 620,
                width: 320,
                height: 88,
            }
        );
    }

    #[test]
    fn plan_overlay_bounds_rejects_out_of_bounds_card() {
        let work_area = OverlayRect {
            x: 0,
            y: 0,
            width: 300,
            height: 200,
        };

        let error =
            plan_overlay_bounds(work_area, OverlayAnchor::TopRight, 360, 96, 24).unwrap_err();

        assert!(matches!(error, OverlayError::OutOfBounds { .. }));
    }

    #[test]
    fn plan_overlay_bounds_rejects_zero_size() {
        let work_area = OverlayRect {
            x: 0,
            y: 0,
            width: 300,
            height: 200,
        };

        let error = plan_overlay_bounds(work_area, OverlayAnchor::TopLeft, 0, 96, 8).unwrap_err();

        assert_eq!(
            error,
            OverlayError::InvalidSize {
                width: 0,
                height: 96
            }
        );
    }

    #[test]
    fn plan_overlay_cards_uses_calibration_bottom_anchors() {
        let config = CalibrationConfig::new(
            calibration::ScreenshotSize {
                width: 1920,
                height: 1080,
            },
            [
                calibration::NormalizedRect {
                    x: 0.1,
                    y: 0.1,
                    width: 0.1,
                    height: 0.1,
                },
                calibration::NormalizedRect {
                    x: 0.2,
                    y: 0.1,
                    width: 0.1,
                    height: 0.1,
                },
                calibration::NormalizedRect {
                    x: 0.3,
                    y: 0.1,
                    width: 0.1,
                    height: 0.1,
                },
            ],
            [
                NormalizedPoint { x: 0.25, y: 0.8 },
                NormalizedPoint { x: 0.50, y: 0.8 },
                NormalizedPoint { x: 0.75, y: 0.8 },
            ],
            calibration::NormalizedRect {
                x: 0.4,
                y: 0.9,
                width: 0.2,
                height: 0.05,
            },
        );

        let cards = plan_overlay_cards(
            OverlayBounds {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            Some(&config),
            &[],
            260,
            118,
            18,
        )
        .unwrap();

        assert_eq!(cards.len(), 3);
        assert_eq!(cards[0].bounds.x, 350);
        assert_eq!(cards[0].bounds.y, 728);
        assert_eq!(cards[1].source, "calibration.bottomAnchors");
    }

    #[test]
    fn plan_overlay_cards_applies_slot_payload() {
        let cards = plan_overlay_cards(
            OverlayBounds {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            },
            None,
            &[OverlaySlotData {
                slot: 2,
                title: "棱彩门票".to_string(),
                body: Some("Apex 推荐".to_string()),
                augment_id: Some("prismatic-ticket".to_string()),
                rank: Some("S".to_string()),
                score: Some("4.55".to_string()),
            }],
            240,
            110,
            16,
        )
        .unwrap();

        assert_eq!(cards[1].title, "棱彩门票");
        assert_eq!(cards[1].rank.as_deref(), Some("S"));
        assert_eq!(cards[0].source, "fallback.staticAnchors");
    }
}
