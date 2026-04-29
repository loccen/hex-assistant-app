#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, Monitor, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

const OVERLAY_LABEL: &str = "hex-assistant-overlay";
const DEFAULT_WIDTH: u32 = 360;
const DEFAULT_HEIGHT: u32 = 96;
const DEFAULT_GAP: i32 = 24;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayTestCardRequest {
    pub monitor_name: Option<String>,
    pub anchor: OverlayAnchor,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub gap: Option<i32>,
    pub click_through: bool,
}

impl Default for OverlayTestCardRequest {
    fn default() -> Self {
        Self {
            monitor_name: None,
            anchor: OverlayAnchor::TopRight,
            width: Some(DEFAULT_WIDTH),
            height: Some(DEFAULT_HEIGHT),
            gap: Some(DEFAULT_GAP),
            click_through: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OverlayAnchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
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
    pub visibility_changes: Vec<OverlayVisibilityChange>,
    pub click_through: OverlayClickThroughReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayCreationParams {
    pub transparent: bool,
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
            transparent: true,
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
            Self::NoMonitor => write!(formatter, "未找到可用显示器"),
            Self::MonitorNotFound(name) => write!(formatter, "未找到目标显示器: {name}"),
            Self::InvalidSize { width, height } => {
                write!(formatter, "Overlay 尺寸无效: {width}x{height}")
            }
            Self::OutOfBounds { bounds, work_area } => write!(
                formatter,
                "Overlay 坐标越界: bounds=({}, {}, {}x{}), workArea=({}, {}, {}x{})",
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

pub fn show_overlay_test_card_inner(
    app: &AppHandle,
    request: OverlayTestCardRequest,
) -> Result<OverlayOperationReport, OverlayError> {
    let monitor = select_monitor(app, request.monitor_name.as_deref())?;
    let monitor_report = monitor_to_report(&monitor);
    let width = request.width.unwrap_or(DEFAULT_WIDTH);
    let height = request.height.unwrap_or(DEFAULT_HEIGHT);
    let gap = request.gap.unwrap_or(DEFAULT_GAP);
    let bounds = plan_overlay_bounds(monitor_report.work_area, request.anchor, width, height, gap)?;

    let window = match app.get_webview_window(OVERLAY_LABEL) {
        Some(existing) => {
            existing.close().map_err(|error| {
                OverlayError::Tauri(format!("关闭旧 Overlay 窗口失败: {error}"))
            })?;
            build_overlay_window(app, &monitor, bounds)?
        }
        None => build_overlay_window(app, &monitor, bounds)?,
    };

    apply_overlay_geometry(&window, &monitor, bounds)?;
    #[cfg(windows)]
    align_window_client_to_requested_bounds(&window);
    let click_through = apply_click_through(&window, request.click_through);
    window
        .show()
        .map_err(|error| OverlayError::Tauri(format!("显示 Overlay 失败: {error}")))?;

    Ok(OverlayOperationReport {
        label: OVERLAY_LABEL.to_string(),
        created: true,
        visible: true,
        creation: OverlayCreationParams::default(),
        monitor: monitor_report,
        bounds,
        visibility_changes: vec![OverlayVisibilityChange {
            from: false,
            to: true,
            reason: "createAndShow".to_string(),
        }],
        click_through,
    })
}

pub fn hide_overlay_test_card_inner(
    app: &AppHandle,
) -> Result<OverlayOperationReport, OverlayError> {
    let monitor = select_monitor(app, None)?;
    let monitor_report = monitor_to_report(&monitor);
    let bounds = plan_overlay_bounds(
        monitor_report.work_area,
        OverlayAnchor::TopRight,
        DEFAULT_WIDTH,
        DEFAULT_HEIGHT,
        DEFAULT_GAP,
    )?;
    let window = app.get_webview_window(OVERLAY_LABEL);
    if let Some(window) = window {
        window
            .hide()
            .map_err(|error| OverlayError::Tauri(format!("隐藏 Overlay 失败: {error}")))?;
    }

    Ok(OverlayOperationReport {
        label: OVERLAY_LABEL.to_string(),
        created: false,
        visible: false,
        creation: OverlayCreationParams::default(),
        monitor: monitor_report,
        bounds,
        visibility_changes: vec![OverlayVisibilityChange {
            from: true,
            to: false,
            reason: "hide".to_string(),
        }],
        click_through: platform_pending_click_through(false),
    })
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
    bounds: OverlayBounds,
) -> Result<WebviewWindow, OverlayError> {
    let url = WebviewUrl::External(
        "about:blank"
            .parse()
            .map_err(|error| OverlayError::Tauri(format!("解析 Overlay URL 失败: {error}")))?,
    );
    let (logical_x, logical_y, logical_width, logical_height) = logical_geometry(monitor, bounds);

    let mut builder = WebviewWindowBuilder::new(app, OVERLAY_LABEL, url)
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
        .position(logical_x, logical_y)
        .inner_size(logical_width, logical_height)
        .visible(false)
        .initialization_script(test_card_script());

    // 旧 POC 验证过：Windows WebView2 需要在 controller 创建前设置透明背景，
    // 否则 build 后再改背景色仍可能留下白底或边缘闪烁。
    #[cfg(not(target_os = "macos"))]
    {
        builder = builder
            .transparent(true)
            .background_color(tauri::window::Color(0, 0, 0, 0));
    }

    builder
        .build()
        .map_err(|error| OverlayError::Tauri(format!("创建 Overlay 窗口失败: {error}")))
}

fn apply_overlay_geometry(
    window: &WebviewWindow,
    monitor: &Monitor,
    bounds: OverlayBounds,
) -> Result<(), OverlayError> {
    let (logical_x, logical_y, logical_width, logical_height) = logical_geometry(monitor, bounds);

    window
        .set_position(LogicalPosition::new(logical_x, logical_y))
        .map_err(|error| OverlayError::Tauri(format!("设置 Overlay 坐标失败: {error}")))?;
    window
        .set_size(LogicalSize::new(logical_width, logical_height))
        .map_err(|error| OverlayError::Tauri(format!("设置 Overlay 尺寸失败: {error}")))?;

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

#[cfg(windows)]
fn apply_click_through(window: &WebviewWindow, requested: bool) -> OverlayClickThroughReport {
    match window.set_ignore_cursor_events(requested) {
        Ok(()) => {
            let child_count = if requested {
                if let Some(hwnd) = window_hwnd(window) {
                    let count = enumerate_children_transparent(hwnd).len();
                    schedule_click_through_retries(window.app_handle().clone(), hwnd);
                    count
                } else {
                    0
                }
            } else {
                0
            };
            OverlayClickThroughReport {
                requested,
                platform: std::env::consts::OS.to_string(),
                status: OverlayClickThroughStatus::Applied,
                message: format!(
                    "set_ignore_cursor_events 已执行；WebView2 子窗口立即补穿透 {} 个，后续会延迟重试。",
                    child_count
                ),
            }
        }
        Err(error) => OverlayClickThroughReport {
            requested,
            platform: std::env::consts::OS.to_string(),
            status: OverlayClickThroughStatus::Failed,
            message: format!("set_ignore_cursor_events 执行失败: {error}"),
        },
    }
}

#[cfg(not(windows))]
fn apply_click_through(_window: &WebviewWindow, requested: bool) -> OverlayClickThroughReport {
    platform_pending_click_through(requested)
}

fn platform_pending_click_through(requested: bool) -> OverlayClickThroughReport {
    OverlayClickThroughReport {
        requested,
        platform: std::env::consts::OS.to_string(),
        status: OverlayClickThroughStatus::PendingManualAcceptance,
        message: "非 Windows 平台暂不执行 set_ignore_cursor_events，待真实环境验收".to_string(),
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

fn test_card_script() -> &'static str {
    r#"
window.addEventListener('DOMContentLoaded', () => {
  document.documentElement.style.background = 'transparent';
  document.body.style.margin = '0';
  document.body.style.background = 'transparent';
  document.body.innerHTML = `
    <div style="box-sizing:border-box;width:100vw;height:100vh;padding:12px 16px;border:1px solid rgba(94,234,212,.65);border-radius:8px;background:rgba(12,18,24,.78);box-shadow:0 10px 28px rgba(0,0,0,.28);font-family:system-ui,-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;color:#f8fafc;">
      <div style="font-size:13px;font-weight:650;line-height:18px;">Hex Assistant Overlay</div>
      <div style="margin-top:6px;font-size:12px;line-height:16px;color:#cbd5e1;">静态测试卡片 · 透明 · 置顶 · 点击穿透候选</div>
    </div>
  `;
});
"#
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
fn align_window_client_to_requested_bounds(window: &WebviewWindow) {
    let Some(hwnd) = window_hwnd(window) else {
        return;
    };
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
            return;
        }
        let nc_left = origin.x - outer.left;
        let nc_top = origin.y - outer.top;
        let outer_width = outer.right - outer.left;
        let outer_height = outer.bottom - outer.top;
        let _ = win32_ffi::SetWindowPos(
            hwnd,
            0,
            outer.left - nc_left,
            outer.top - nc_top,
            outer_width,
            outer_height,
            win32_ffi::SWP_NOZORDER | win32_ffi::SWP_NOACTIVATE,
        );
    }
}

#[cfg(windows)]
unsafe extern "system" fn make_child_transparent(child_hwnd: isize, lparam: isize) -> i32 {
    let counter = unsafe { &mut *(lparam as *mut usize) };
    let style = unsafe { win32_ffi::GetWindowLongPtrW(child_hwnd, win32_ffi::GWL_EXSTYLE) };
    unsafe {
        win32_ffi::SetWindowLongPtrW(
            child_hwnd,
            win32_ffi::GWL_EXSTYLE,
            style | win32_ffi::WS_EX_TRANSPARENT,
        );
    }
    *counter += 1;
    1
}

#[cfg(windows)]
fn enumerate_children_transparent(hwnd: isize) -> Vec<String> {
    let mut count = 0usize;
    let lparam = (&mut count as *mut usize) as isize;
    unsafe {
        win32_ffi::EnumChildWindows(hwnd, make_child_transparent, lparam);
    }
    (0..count).map(|index| format!("child-{index}")).collect()
}

#[cfg(windows)]
fn schedule_click_through_retries(app: AppHandle, hwnd: isize) {
    std::thread::spawn(move || {
        let _ = app;
        for delay_ms in [200u64, 700, 2000] {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            let alive = unsafe { win32_ffi::IsWindow(hwnd) != 0 };
            if !alive {
                return;
            }
            let _ = enumerate_children_transparent(hwnd);
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
}
