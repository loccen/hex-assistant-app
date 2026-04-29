#![allow(dead_code)]

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use xcap::Monitor;

const BLACK_MEAN_LUMA_THRESHOLD: f64 = 3.0;
const BLACK_BRIGHT_PIXEL_RATIO_THRESHOLD: f64 = 0.001;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSampleReport {
    pub captured_at: String,
    pub monitor: MonitorDiagnostic,
    pub image: ImageDiagnostic,
    pub png_path: PathBuf,
    pub json_path: PathBuf,
    pub previous_frame_hash: Option<String>,
    pub stale_frame: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MonitorDiagnostic {
    pub id: u32,
    pub name: String,
    pub friendly_name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub rotation: f32,
    pub scale_factor: f32,
    pub frequency: f32,
    pub primary: bool,
    pub builtin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImageDiagnostic {
    pub width: u32,
    pub height: u32,
    pub capture_duration_ms: u128,
    pub save_duration_ms: u128,
    pub mean_luma: f64,
    pub min_luma: u8,
    pub max_luma: u8,
    pub bright_pixel_ratio: f64,
    pub black_screen: bool,
    pub frame_hash: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FrameAnalysis {
    pub mean_luma: f64,
    pub min_luma: u8,
    pub max_luma: u8,
    pub bright_pixel_ratio: f64,
    pub black_screen: bool,
    pub frame_hash: String,
}

pub fn capture_samples_dir(app_data_dir: impl AsRef<Path>) -> PathBuf {
    app_data_dir.as_ref().join("captures").join("samples")
}

pub fn capture_primary_monitor_sample(
    app_data_dir: impl AsRef<Path>,
) -> Result<CaptureSampleReport, String> {
    capture_monitor_sample(app_data_dir, None)
}

pub fn capture_monitor_sample(
    app_data_dir: impl AsRef<Path>,
    preferred_monitor_id: Option<u32>,
) -> Result<CaptureSampleReport, String> {
    let samples_dir = capture_samples_dir(app_data_dir);
    fs::create_dir_all(&samples_dir)
        .map_err(|error| format!("无法创建截图样本目录 {}: {error}", samples_dir.display()))?;

    let monitors = Monitor::all().map_err(|error| format!("无法枚举显示器: {error}"))?;
    let monitor = select_monitor(monitors, preferred_monitor_id)?;
    let monitor_diagnostic = read_monitor_diagnostic(&monitor)?;

    let capture_start = Instant::now();
    let image = monitor
        .capture_image()
        .map_err(|error| format!("无法截取显示器 {}: {error}", monitor_diagnostic.id))?;
    let capture_duration_ms = capture_start.elapsed().as_millis();

    let analysis = analyze_rgba_frame(image.as_raw(), image.width(), image.height())?;
    let previous_frame_hash = latest_frame_hash(&samples_dir, monitor_diagnostic.id)?;
    let stale_frame = previous_frame_hash
        .as_deref()
        .is_some_and(|hash| hash == analysis.frame_hash);

    let timestamp = timestamp_for_filename();
    let base_name = format!("monitor-{}-{timestamp}", monitor_diagnostic.id);
    let png_path = samples_dir.join(format!("{base_name}.png"));
    let json_path = samples_dir.join(format!("{base_name}.json"));

    let save_start = Instant::now();
    image
        .save(&png_path)
        .map_err(|error| format!("无法保存原始截图 {}: {error}", png_path.display()))?;
    let save_duration_ms = save_start.elapsed().as_millis();

    let report = CaptureSampleReport {
        captured_at: Utc::now().to_rfc3339(),
        monitor: monitor_diagnostic,
        image: ImageDiagnostic {
            width: image.width(),
            height: image.height(),
            capture_duration_ms,
            save_duration_ms,
            mean_luma: analysis.mean_luma,
            min_luma: analysis.min_luma,
            max_luma: analysis.max_luma,
            bright_pixel_ratio: analysis.bright_pixel_ratio,
            black_screen: analysis.black_screen,
            frame_hash: analysis.frame_hash,
        },
        png_path: png_path.clone(),
        json_path: json_path.clone(),
        previous_frame_hash,
        stale_frame,
    };

    let content = serde_json::to_string_pretty(&report)
        .map_err(|error| format!("无法序列化截图诊断 JSON: {error}"))?;
    fs::write(&json_path, format!("{content}\n"))
        .map_err(|error| format!("无法写入截图诊断 JSON {}: {error}", json_path.display()))?;

    Ok(report)
}

pub fn analyze_rgba_frame(
    raw_rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<FrameAnalysis, String> {
    let expected_len = width as usize * height as usize * 4;
    if raw_rgba.len() != expected_len {
        return Err(format!(
            "RGBA 数据长度不匹配: 期望 {expected_len} 字节，实际 {} 字节",
            raw_rgba.len()
        ));
    }
    if width == 0 || height == 0 {
        return Err("截图尺寸不能为 0".to_string());
    }

    let mut hash = Sha256::new();
    hash.update(width.to_le_bytes());
    hash.update(height.to_le_bytes());
    hash.update(raw_rgba);

    let mut min_luma = u8::MAX;
    let mut max_luma = u8::MIN;
    let mut luma_sum = 0f64;
    let mut bright_pixels = 0usize;
    let pixel_count = width as usize * height as usize;

    for pixel in raw_rgba.chunks_exact(4) {
        let luma = luma_from_rgb(pixel[0], pixel[1], pixel[2]);
        min_luma = min_luma.min(luma);
        max_luma = max_luma.max(luma);
        luma_sum += f64::from(luma);
        if luma > 32 {
            bright_pixels += 1;
        }
    }

    let mean_luma = luma_sum / pixel_count as f64;
    let bright_pixel_ratio = bright_pixels as f64 / pixel_count as f64;
    let black_screen = is_black_screen(mean_luma, bright_pixel_ratio);

    Ok(FrameAnalysis {
        mean_luma,
        min_luma,
        max_luma,
        bright_pixel_ratio,
        black_screen,
        frame_hash: bytes_to_hex(&hash.finalize()),
    })
}

pub fn is_stale_frame(previous_hash: Option<&str>, current_hash: &str) -> bool {
    previous_hash.is_some_and(|hash| hash == current_hash)
}

pub fn is_black_screen(mean_luma: f64, bright_pixel_ratio: f64) -> bool {
    mean_luma <= BLACK_MEAN_LUMA_THRESHOLD
        && bright_pixel_ratio <= BLACK_BRIGHT_PIXEL_RATIO_THRESHOLD
}

fn select_monitor(
    monitors: Vec<Monitor>,
    preferred_monitor_id: Option<u32>,
) -> Result<Monitor, String> {
    if monitors.is_empty() {
        return Err("未发现可截图的显示器".to_string());
    }

    if let Some(preferred_id) = preferred_monitor_id {
        for monitor in &monitors {
            if monitor
                .id()
                .map_err(|error| format!("无法读取显示器 ID: {error}"))?
                == preferred_id
            {
                return Ok(monitor.clone());
            }
        }
        return Err(format!("未找到指定显示器 ID {preferred_id}"));
    }

    for monitor in &monitors {
        if monitor
            .is_primary()
            .map_err(|error| format!("无法读取显示器主屏状态: {error}"))?
        {
            return Ok(monitor.clone());
        }
    }

    Ok(monitors[0].clone())
}

fn read_monitor_diagnostic(monitor: &Monitor) -> Result<MonitorDiagnostic, String> {
    Ok(MonitorDiagnostic {
        id: monitor
            .id()
            .map_err(|error| format!("无法读取显示器 ID: {error}"))?,
        name: monitor
            .name()
            .map_err(|error| format!("无法读取显示器名称: {error}"))?,
        friendly_name: monitor
            .friendly_name()
            .map_err(|error| format!("无法读取显示器友好名称: {error}"))?,
        x: monitor
            .x()
            .map_err(|error| format!("无法读取显示器 X 坐标: {error}"))?,
        y: monitor
            .y()
            .map_err(|error| format!("无法读取显示器 Y 坐标: {error}"))?,
        width: monitor
            .width()
            .map_err(|error| format!("无法读取显示器宽度: {error}"))?,
        height: monitor
            .height()
            .map_err(|error| format!("无法读取显示器高度: {error}"))?,
        rotation: monitor
            .rotation()
            .map_err(|error| format!("无法读取显示器旋转角度: {error}"))?,
        scale_factor: monitor
            .scale_factor()
            .map_err(|error| format!("无法读取显示器缩放比例: {error}"))?,
        frequency: monitor
            .frequency()
            .map_err(|error| format!("无法读取显示器刷新率: {error}"))?,
        primary: monitor
            .is_primary()
            .map_err(|error| format!("无法读取显示器主屏状态: {error}"))?,
        builtin: monitor
            .is_builtin()
            .map_err(|error| format!("无法读取显示器内置状态: {error}"))?,
    })
}

fn latest_frame_hash(samples_dir: &Path, monitor_id: u32) -> Result<Option<String>, String> {
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    let prefix = format!("monitor-{monitor_id}-");

    for entry in fs::read_dir(samples_dir)
        .map_err(|error| format!("无法读取截图样本目录 {}: {error}", samples_dir.display()))?
    {
        let entry = entry.map_err(|error| format!("无法读取截图样本目录项: {error}"))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !file_name.starts_with(&prefix) {
            continue;
        }

        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .map_err(|error| format!("无法读取截图样本元数据 {}: {error}", path.display()))?;
        if newest
            .as_ref()
            .is_none_or(|(newest_modified, _)| modified > *newest_modified)
        {
            newest = Some((modified, path));
        }
    }

    let Some((_, path)) = newest else {
        return Ok(None);
    };
    let content = fs::read_to_string(&path)
        .map_err(|error| format!("无法读取旧截图诊断 JSON {}: {error}", path.display()))?;
    let report: CaptureSampleReport = serde_json::from_str(&content)
        .map_err(|error| format!("无法解析旧截图诊断 JSON {}: {error}", path.display()))?;
    Ok(Some(report.image.frame_hash))
}

fn luma_from_rgb(red: u8, green: u8, blue: u8) -> u8 {
    ((u16::from(red) * 77 + u16::from(green) * 150 + u16::from(blue) * 29) >> 8) as u8
}

fn timestamp_for_filename() -> String {
    Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string()
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    #[test]
    fn analyzes_black_frame() {
        let raw = vec![0u8; 4 * 4 * 4];

        let analysis = analyze_rgba_frame(&raw, 4, 4).expect("应能分析纯黑画面");

        assert_eq!(analysis.min_luma, 0);
        assert_eq!(analysis.max_luma, 0);
        assert!(analysis.black_screen);
        assert_eq!(analysis.frame_hash.len(), 64);
    }

    #[test]
    fn analyzes_visible_frame() {
        let image = RgbaImage::from_fn(2, 2, |x, y| {
            if x == y {
                image::Rgba([255, 255, 255, 255])
            } else {
                image::Rgba([0, 0, 0, 255])
            }
        });

        let analysis = analyze_rgba_frame(image.as_raw(), 2, 2).expect("应能分析可见画面");

        assert!(!analysis.black_screen);
        assert_eq!(analysis.max_luma, 255);
        assert_eq!(analysis.bright_pixel_ratio, 0.5);
    }

    #[test]
    fn rejects_wrong_rgba_length() {
        let error = analyze_rgba_frame(&[0, 0, 0], 1, 1).expect_err("长度错误应失败");

        assert!(error.contains("RGBA 数据长度不匹配"));
    }

    #[test]
    fn detects_stale_frame_by_hash() {
        assert!(is_stale_frame(Some("abc"), "abc"));
        assert!(!is_stale_frame(Some("abc"), "def"));
        assert!(!is_stale_frame(None, "abc"));
    }

    #[test]
    fn builds_capture_samples_dir_under_captures() {
        let path = capture_samples_dir("/tmp/hex-app");

        assert!(path.ends_with("captures/samples"));
    }
}
