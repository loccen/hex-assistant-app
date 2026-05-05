#![allow(dead_code)]

use crate::app_paths::AppPaths;
use crate::calibration::{self, denormalize_rect, CalibrationConfig, PixelRect, ScreenshotSize};
#[cfg(not(test))]
use crate::capture;
use crate::models::AppSettings;
#[cfg(not(test))]
use crate::models::TelemetryEvent;
#[cfg(not(test))]
use crate::ocr::{self, AUGMENT_DICTIONARY_ZH_CN};
use crate::ocr::{CalibratedNameOcrReport, CalibratedNameOcrSlotReport};
use crate::state_machine::{AugmentChoice, PanelState};
#[cfg(not(test))]
use crate::telemetry;
#[cfg(not(test))]
use chrono::Utc;
use image::DynamicImage;
#[cfg(not(test))]
use ort::init_from as init_ort_from;
use serde::Serialize;
#[cfg(not(test))]
use std::path::Path;
use std::path::PathBuf;
#[cfg(not(test))]
use std::time::Instant;
use tauri::AppHandle;

const BUTTON_VISIBLE_STDDEV_THRESHOLD: f64 = 12.0;
const BUTTON_VISIBLE_EDGE_THRESHOLD: f64 = 0.08;
const BUTTON_VISIBLE_BRIGHT_THRESHOLD: f64 = 0.025;
const SLOT_VISIBLE_STDDEV_THRESHOLD: f64 = 16.0;
const SLOT_VISIBLE_EDGE_THRESHOLD: f64 = 0.12;
const SLOT_VISIBLE_BRIGHT_THRESHOLD: f64 = 0.035;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePanelSnapshotReport {
    pub captured_at: String,
    pub panel_state: PanelState,
    pub choice_count: usize,
    pub matched_slots: usize,
    pub source_image_path: PathBuf,
    pub capture_report_path: PathBuf,
    pub ocr_report_path: Option<PathBuf>,
    pub capture_black_screen: bool,
    pub capture_stale_frame: bool,
    pub button_region: RegionActivity,
    pub slot_regions: Vec<SlotRegionActivity>,
    pub choices: Vec<AugmentChoice>,
}

#[derive(Debug, Clone)]
pub struct RuntimePanelSnapshot {
    pub panel_state: PanelState,
    pub choices: Vec<AugmentChoice>,
    pub report: RuntimePanelSnapshotReport,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimePanelUpdate {
    pub captured_at: String,
    pub panel_state: PanelState,
    pub choices: Vec<AugmentChoice>,
    pub capture_path: PathBuf,
    pub capture_report_path: PathBuf,
    pub capture_black_screen: bool,
    pub capture_stale_frame: bool,
    pub ocr_report_path: Option<PathBuf>,
    pub button_region: RegionActivity,
    pub slot_regions: Vec<SlotRegionActivity>,
    pub recognized_slot_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotRegionActivity {
    pub slot: u8,
    pub visible: bool,
    pub mean_luma: f64,
    pub stddev_luma: f64,
    pub bright_pixel_ratio: f64,
    pub edge_density: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionActivity {
    pub visible: bool,
    pub mean_luma: f64,
    pub stddev_luma: f64,
    pub bright_pixel_ratio: f64,
    pub edge_density: f64,
}

pub fn detect_runtime_panel_snapshot(
    #[cfg(not(test))] app: &AppHandle,
    #[cfg(not(test))] paths: &AppPaths,
    #[cfg(not(test))] settings: &AppSettings,
    #[cfg(test)] _app: &AppHandle,
    #[cfg(test)] _paths: &AppPaths,
    #[cfg(test)] _settings: &AppSettings,
) -> Result<RuntimePanelSnapshot, String> {
    #[cfg(test)]
    {
        Err("HEX-RUNTIME-PANEL-TEST-STUB: 测试编译不执行运行时截图/OCR 路径".to_string())
    }

    #[cfg(not(test))]
    {
        let trace_id = telemetry::new_trace_id("runtime-panel");
        let update = capture_runtime_panel(app, paths, settings, &trace_id)?;
        Ok(RuntimePanelSnapshot {
            panel_state: update.panel_state,
            choices: update.choices.clone(),
            report: RuntimePanelSnapshotReport {
                captured_at: update.captured_at,
                panel_state: update.panel_state,
                choice_count: update.choices.len(),
                matched_slots: update.recognized_slot_count,
                source_image_path: update.capture_path,
                capture_report_path: update.capture_report_path,
                ocr_report_path: update.ocr_report_path,
                capture_black_screen: update.capture_black_screen,
                capture_stale_frame: update.capture_stale_frame,
                button_region: update.button_region,
                slot_regions: update.slot_regions,
                choices: update.choices,
            },
        })
    }
}

#[cfg(not(test))]
fn capture_runtime_panel(
    app: &AppHandle,
    paths: &AppPaths,
    settings: &AppSettings,
    trace_id: &str,
) -> Result<RuntimePanelUpdate, String> {
    let calibration = calibration::load_calibration_config(&paths.root)?;
    let preferred_monitor_id =
        parse_preferred_monitor_id(settings.capture.preferred_monitor_id.as_deref())?;
    let capture_report = capture::capture_monitor_sample(&paths.root, preferred_monitor_id)?;
    let image = image::open(&capture_report.png_path).map_err(|error| {
        format!(
            "HEX-RUNTIME-PANEL-IMAGE: 无法读取运行时截图 {}: {error}",
            capture_report.png_path.display()
        )
    })?;
    let screenshot_size = ScreenshotSize {
        width: image.width(),
        height: image.height(),
    };
    let button_region = analyze_region(
        &image,
        calibration.bottom_button_region,
        screenshot_size,
        BUTTON_VISIBLE_STDDEV_THRESHOLD,
        BUTTON_VISIBLE_EDGE_THRESHOLD,
        BUTTON_VISIBLE_BRIGHT_THRESHOLD,
    )?;
    let slot_regions = calibration
        .name_regions
        .iter()
        .enumerate()
        .map(|(index, rect)| {
            analyze_region(
                &image,
                *rect,
                screenshot_size,
                SLOT_VISIBLE_STDDEV_THRESHOLD,
                SLOT_VISIBLE_EDGE_THRESHOLD,
                SLOT_VISIBLE_BRIGHT_THRESHOLD,
            )
            .map(|activity| SlotRegionActivity {
                slot: u8::try_from(index + 1).unwrap_or(0),
                visible: activity.visible,
                mean_luma: activity.mean_luma,
                stddev_luma: activity.stddev_luma,
                bright_pixel_ratio: activity.bright_pixel_ratio,
                edge_density: activity.edge_density,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    if capture_report.image.black_screen {
        return Ok(RuntimePanelUpdate {
            captured_at: capture_report.captured_at,
            panel_state: PanelState::Collapsed,
            choices: Vec::new(),
            capture_path: capture_report.png_path,
            capture_report_path: capture_report.json_path,
            capture_black_screen: true,
            capture_stale_frame: capture_report.stale_frame,
            ocr_report_path: None,
            button_region,
            slot_regions,
            recognized_slot_count: 0,
        });
    }

    let should_run_ocr =
        button_region.visible || slot_regions.iter().filter(|slot| slot.visible).count() >= 2;
    let ocr_report = if should_run_ocr {
        Some(run_calibrated_ocr_task(
            paths,
            trace_id,
            crate::resource_paths::resource_root(app),
            calibration,
            Some(capture_report.png_path.clone()),
            preferred_monitor_id,
            settings.ocr.min_confidence,
            settings.ocr.min_match_score,
        )?)
    } else {
        None
    };

    let (panel_state, choices) =
        build_panel_result(&button_region, &slot_regions, ocr_report.as_ref());
    let recognized_slot_count = choices.len();

    Ok(RuntimePanelUpdate {
        captured_at: capture_report.captured_at,
        panel_state,
        choices,
        capture_path: capture_report.png_path,
        capture_report_path: capture_report.json_path,
        capture_black_screen: false,
        capture_stale_frame: capture_report.stale_frame,
        ocr_report_path: ocr_report.as_ref().map(|report| report.report_path.clone()),
        button_region,
        slot_regions,
        recognized_slot_count,
    })
}

#[cfg(test)]
fn capture_runtime_panel(
    _app: &AppHandle,
    _paths: &AppPaths,
    _settings: &AppSettings,
    _trace_id: &str,
) -> Result<RuntimePanelUpdate, String> {
    Err("HEX-RUNTIME-PANEL-TEST-STUB: 测试编译不执行运行时截图/OCR 路径".to_string())
}

fn build_panel_result(
    button_region: &RegionActivity,
    slot_regions: &[SlotRegionActivity],
    ocr_report: Option<&CalibratedNameOcrReport>,
) -> (PanelState, Vec<AugmentChoice>) {
    let choices = ocr_report
        .map(|report| build_choices_from_ocr(report.slots.as_slice()))
        .unwrap_or_default();
    let visible_slot_count = slot_regions.iter().filter(|slot| slot.visible).count();
    let recognized_slot_count = choices.len();
    let panel_state = if recognized_slot_count >= 2
        || visible_slot_count >= 2
        || (button_region.visible && (recognized_slot_count >= 1 || visible_slot_count >= 1))
    {
        PanelState::Expanded
    } else {
        PanelState::Collapsed
    };

    (panel_state, choices)
}

fn build_choices_from_ocr(slots: &[CalibratedNameOcrSlotReport]) -> Vec<AugmentChoice> {
    slots
        .iter()
        .filter_map(|slot| {
            slot.augment_id.as_ref().map(|augment_id| AugmentChoice {
                slot: slot_index(slot),
                augment_id: augment_id.clone(),
            })
        })
        .collect()
}

fn slot_index(slot: &CalibratedNameOcrSlotReport) -> u8 {
    match slot.slot.as_str() {
        "left" => 1,
        "center" => 2,
        "right" => 3,
        _ => 0,
    }
}

fn analyze_region(
    image: &DynamicImage,
    normalized_rect: calibration::NormalizedRect,
    screenshot_size: ScreenshotSize,
    stddev_threshold: f64,
    edge_threshold: f64,
    bright_threshold: f64,
) -> Result<RegionActivity, String> {
    let rect = denormalize_rect(normalized_rect, screenshot_size)?;
    let rect = clamp_rect(rect, image.width(), image.height())
        .ok_or_else(|| "HEX-RUNTIME-PANEL-RECT: 运行时面板判定区域超出截图范围".to_string())?;
    let crop = image
        .crop_imm(rect.x, rect.y, rect.width, rect.height)
        .to_luma8();
    let pixel_count = usize::try_from(rect.width.saturating_mul(rect.height))
        .map_err(|_| "HEX-RUNTIME-PANEL-PIXELS: 运行时区域像素数溢出".to_string())?;
    if pixel_count == 0 {
        return Err("HEX-RUNTIME-PANEL-RECT: 运行时面板判定区域为空".to_string());
    }

    let mut luma_sum = 0.0;
    let mut luma_sum_sq = 0.0;
    let mut bright_pixels = 0usize;
    let mut edge_hits = 0usize;
    let mut edge_checks = 0usize;
    for y in 0..crop.height() {
        for x in 0..crop.width() {
            let current = f64::from(crop.get_pixel(x, y)[0]);
            luma_sum += current;
            luma_sum_sq += current * current;
            if current >= 160.0 {
                bright_pixels += 1;
            }
            if x > 0 {
                let previous = f64::from(crop.get_pixel(x - 1, y)[0]);
                if (current - previous).abs() >= 24.0 {
                    edge_hits += 1;
                }
                edge_checks += 1;
            }
            if y > 0 {
                let previous = f64::from(crop.get_pixel(x, y - 1)[0]);
                if (current - previous).abs() >= 24.0 {
                    edge_hits += 1;
                }
                edge_checks += 1;
            }
        }
    }

    let mean_luma = luma_sum / pixel_count as f64;
    let variance = (luma_sum_sq / pixel_count as f64) - (mean_luma * mean_luma);
    let stddev_luma = variance.max(0.0).sqrt();
    let bright_pixel_ratio = bright_pixels as f64 / pixel_count as f64;
    let edge_density = if edge_checks == 0 {
        0.0
    } else {
        edge_hits as f64 / edge_checks as f64
    };

    Ok(RegionActivity {
        visible: stddev_luma >= stddev_threshold
            || edge_density >= edge_threshold
            || bright_pixel_ratio >= bright_threshold,
        mean_luma,
        stddev_luma,
        bright_pixel_ratio,
        edge_density,
    })
}

fn clamp_rect(rect: PixelRect, image_width: u32, image_height: u32) -> Option<PixelRect> {
    let x = rect.x.min(image_width);
    let y = rect.y.min(image_height);
    let right = rect.x.saturating_add(rect.width).min(image_width);
    let bottom = rect.y.saturating_add(rect.height).min(image_height);
    let width = right.saturating_sub(x);
    let height = bottom.saturating_sub(y);
    if width == 0 || height == 0 {
        None
    } else {
        Some(PixelRect {
            x,
            y,
            width,
            height,
        })
    }
}

fn parse_preferred_monitor_id(value: Option<&str>) -> Result<Option<u32>, String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value.parse::<u32>().map(Some).map_err(|error| {
                format!("HEX-RUNTIME-PANEL-MONITOR-ID: 无法解析显示器 ID {value}: {error}")
            })
        })
        .unwrap_or(Ok(None))
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
    let prepared =
        crate::resource_paths::prepare_runtime_resource_root(&resource_root, &paths.cache)?;
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

    let dictionary_path = prepared
        .runtime_root
        .join("dictionaries")
        .join(AUGMENT_DICTIONARY_ZH_CN);
    log_ocr_stage(
        paths,
        trace_id,
        "ocr-dictionary-load-start",
        format!("开始加载 OCR 词库 path={}", dictionary_path.display()),
        "准备读取海克斯词库".to_string(),
    );
    let dictionary =
        ocr::AugmentDictionary::load(&dictionary_path).map_err(|error| error.to_string())?;
    log_ocr_stage(
        paths,
        trace_id,
        "ocr-dictionary-load-success",
        format!("OCR 词库加载完成 path={}", dictionary_path.display()),
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
    let ort_dylib_path = ort_dylib_path(&prepared.runtime_root);
    init_ort_from(&ort_dylib_path)
        .map_err(|error| {
            format!(
                "无法加载 ONNX Runtime 动态库 {}: {error}",
                ort_dylib_path.display()
            )
        })?
        .commit();
    log_ocr_stage(
        paths,
        trace_id,
        "ocr-ort-init-success",
        format!(
            "ONNX Runtime 动态库初始化完成 path={}",
            ort_dylib_path.display()
        ),
        "onnxruntime 动态库已显式加载".to_string(),
    );

    log_ocr_stage(
        paths,
        trace_id,
        "ocr-recognizer-init-start",
        format!(
            "开始初始化 OCR 识别器 resource_root={}",
            prepared.runtime_root.display()
        ),
        "准备创建 ORT 会话并加载字符表".to_string(),
    );
    let recognizer_start = Instant::now();
    let mut recognizer = ocr::PpOcrV4RecRecognizer::from_resource_root(&prepared.runtime_root)
        .map_err(|error| error.to_string())?;
    log_ocr_stage(
        paths,
        trace_id,
        "ocr-recognizer-init-success",
        format!(
            "OCR 识别器初始化完成 model={} elapsed_ms={}",
            recognizer.model_path().display(),
            recognizer_start.elapsed().as_millis()
        ),
        "模型、字符表与 ORT 会话已就绪".to_string(),
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
    let report = ocr::recognize_calibrated_name_slots_from_image(
        &mut recognizer,
        &dictionary,
        &calibration,
        &screenshot_path,
        &paths.reports,
        min_confidence,
        min_match_score,
    )
    .map_err(|error| error.to_string())?;
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

#[cfg(test)]
pub(crate) fn run_calibrated_ocr_task(
    _paths: &AppPaths,
    _trace_id: &str,
    _resource_root: PathBuf,
    _calibration: CalibrationConfig,
    _screenshot_path: Option<PathBuf>,
    _preferred_monitor_id: Option<u32>,
    _min_confidence: f32,
    _min_match_score: f32,
) -> Result<CalibratedNameOcrReport, String> {
    Err("HEX-OCR-TEST-STUB: 测试编译不执行 OCR 运行时路径".to_string())
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

#[cfg(not(test))]
fn ort_dylib_path(resource_root: &Path) -> PathBuf {
    let file_name = if cfg!(target_os = "windows") {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    };
    resource_root.join("onnxruntime").join(file_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr::{CalibratedNameOcrSlotReport, CalibratedNameSlot, PixelRectReport};

    #[test]
    fn build_panel_result_expands_when_two_slots_visible() {
        let (panel_state, choices) = build_panel_result(
            &RegionActivity {
                visible: false,
                mean_luma: 10.0,
                stddev_luma: 2.0,
                bright_pixel_ratio: 0.0,
                edge_density: 0.0,
            },
            &[
                SlotRegionActivity {
                    slot: 1,
                    visible: true,
                    mean_luma: 0.0,
                    stddev_luma: 20.0,
                    bright_pixel_ratio: 0.0,
                    edge_density: 0.2,
                },
                SlotRegionActivity {
                    slot: 2,
                    visible: true,
                    mean_luma: 0.0,
                    stddev_luma: 18.0,
                    bright_pixel_ratio: 0.0,
                    edge_density: 0.2,
                },
                SlotRegionActivity {
                    slot: 3,
                    visible: false,
                    mean_luma: 0.0,
                    stddev_luma: 3.0,
                    bright_pixel_ratio: 0.0,
                    edge_density: 0.01,
                },
            ],
            None,
        );

        assert_eq!(panel_state, PanelState::Expanded);
        assert!(choices.is_empty());
    }

    #[test]
    fn build_choices_from_ocr_uses_expected_slot_mapping() {
        let choices = build_choices_from_ocr(&[
            CalibratedNameOcrSlotReport {
                slot: CalibratedNameSlot::Left,
                crop_rect: PixelRectReport {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                crop_path: PathBuf::new(),
                refined_crop_rect: PixelRectReport {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                refined_crop_path: PathBuf::new(),
                enhanced_path: PathBuf::new(),
                preview_path: PathBuf::new(),
                raw_text: "棱彩门票".to_string(),
                confidence: 0.98,
                match_score: 0.99,
                final_name: Some("棱彩门票".to_string()),
                augment_id: Some("prismatic-ticket".to_string()),
                elapsed_ms: 1,
                failure_reason: None,
            },
            CalibratedNameOcrSlotReport {
                slot: CalibratedNameSlot::Center,
                crop_rect: PixelRectReport {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                crop_path: PathBuf::new(),
                refined_crop_rect: PixelRectReport {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                refined_crop_path: PathBuf::new(),
                enhanced_path: PathBuf::new(),
                preview_path: PathBuf::new(),
                raw_text: "构建伙伴".to_string(),
                confidence: 0.98,
                match_score: 0.99,
                final_name: Some("构建伙伴".to_string()),
                augment_id: Some("build-a-bud".to_string()),
                elapsed_ms: 1,
                failure_reason: None,
            },
        ]);

        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].slot, 1);
        assert_eq!(choices[1].slot, 2);
        assert_eq!(choices[0].augment_id, "prismatic-ticket");
        assert_eq!(choices[1].augment_id, "build-a-bud");
    }

    #[test]
    fn parse_preferred_monitor_id_accepts_empty_value() {
        assert_eq!(parse_preferred_monitor_id(None).unwrap(), None);
        assert_eq!(parse_preferred_monitor_id(Some("")).unwrap(), None);
        assert_eq!(parse_preferred_monitor_id(Some("7")).unwrap(), Some(7));
        assert!(parse_preferred_monitor_id(Some("abc")).is_err());
    }
}
