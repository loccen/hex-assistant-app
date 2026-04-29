#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const CALIBRATION_FILE_NAME: &str = "screen-calibration.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationConfig {
    pub version: u32,
    pub screenshot_size: ScreenshotSize,
    pub name_regions: [NormalizedRect; 3],
    pub bottom_anchors: [NormalizedPoint; 3],
    pub bottom_button_region: NormalizedRect,
    pub coordinate_space: CoordinateSpace,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PixelCalibrationInput {
    pub screenshot_size: ScreenshotSize,
    pub name_regions: [PixelRect; 3],
    pub bottom_anchors: [PixelPoint; 3],
    pub bottom_button_region: PixelRect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationProfileResult {
    pub path: PathBuf,
    pub config: CalibrationConfig,
    pub echo: CalibrationEcho,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationEcho {
    pub screenshot_size: ScreenshotSize,
    pub name_region_pixels: [PixelRect; 3],
    pub bottom_anchor_pixels: [PixelPoint; 3],
    pub bottom_button_region_pixels: PixelRect,
    pub name_regions: [NormalizedRect; 3],
    pub bottom_anchors: [NormalizedPoint; 3],
    pub bottom_button_region: NormalizedRect,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CoordinateSpace {
    Normalized,
}

impl CalibrationConfig {
    pub fn new(
        screenshot_size: ScreenshotSize,
        name_regions: [NormalizedRect; 3],
        bottom_anchors: [NormalizedPoint; 3],
        bottom_button_region: NormalizedRect,
    ) -> Self {
        Self {
            version: 1,
            screenshot_size,
            name_regions,
            bottom_anchors,
            bottom_button_region,
            coordinate_space: CoordinateSpace::Normalized,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version == 0 {
            return Err("校准配置版本不能为 0".to_string());
        }
        if self.screenshot_size.width == 0 || self.screenshot_size.height == 0 {
            return Err("截图尺寸不能为 0".to_string());
        }
        for (index, rect) in self.name_regions.iter().enumerate() {
            validate_rect(*rect, &format!("名称区域 {}", index + 1))?;
        }
        for (index, point) in self.bottom_anchors.iter().enumerate() {
            validate_point(*point, &format!("底部锚点 {}", index + 1))?;
        }
        validate_rect(self.bottom_button_region, "底部按钮区域")?;
        Ok(())
    }
}

pub fn calibration_config_path(app_data_dir: impl AsRef<Path>) -> PathBuf {
    app_data_dir
        .as_ref()
        .join("calibration")
        .join(CALIBRATION_FILE_NAME)
}

pub fn save_calibration_config(
    app_data_dir: impl AsRef<Path>,
    config: &CalibrationConfig,
) -> Result<PathBuf, String> {
    config.validate()?;
    let path = calibration_config_path(app_data_dir);
    let parent = path
        .parent()
        .ok_or_else(|| format!("无法定位校准配置父目录 {}", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("无法创建校准配置目录 {}: {error}", parent.display()))?;
    let content = serde_json::to_string_pretty(config)
        .map_err(|error| format!("无法序列化校准配置: {error}"))?;
    fs::write(&path, format!("{content}\n"))
        .map_err(|error| format!("无法写入校准配置 {}: {error}", path.display()))?;
    Ok(path)
}

pub fn save_pixel_calibration_config(
    app_data_dir: impl AsRef<Path>,
    input: PixelCalibrationInput,
) -> Result<CalibrationProfileResult, String> {
    let config = build_calibration_config_from_pixels(input)?;
    let path = save_calibration_config(app_data_dir, &config)?;
    Ok(CalibrationProfileResult {
        path,
        echo: build_calibration_echo(&config)?,
        config,
    })
}

pub fn load_calibration_config(
    app_data_dir: impl AsRef<Path>,
) -> Result<CalibrationConfig, String> {
    let path = calibration_config_path(app_data_dir);
    let content = fs::read_to_string(&path)
        .map_err(|error| format!("无法读取校准配置 {}: {error}", path.display()))?;
    let config: CalibrationConfig = serde_json::from_str(&content)
        .map_err(|error| format!("无法解析校准配置 {}: {error}", path.display()))?;
    config.validate()?;
    Ok(config)
}

pub fn load_calibration_profile<P: AsRef<Path>>(
    app_data_dir: P,
) -> Result<CalibrationProfileResult, String> {
    let path = calibration_config_path(app_data_dir.as_ref());
    let config = load_calibration_config(app_data_dir)?;
    Ok(CalibrationProfileResult {
        path,
        echo: build_calibration_echo(&config)?,
        config,
    })
}

pub fn build_calibration_config_from_pixels(
    input: PixelCalibrationInput,
) -> Result<CalibrationConfig, String> {
    let name_regions = [
        normalize_pixel_rect(input.name_regions[0], input.screenshot_size, "名称区域 1")?,
        normalize_pixel_rect(input.name_regions[1], input.screenshot_size, "名称区域 2")?,
        normalize_pixel_rect(input.name_regions[2], input.screenshot_size, "名称区域 3")?,
    ];
    let bottom_anchors = [
        normalize_pixel_point(input.bottom_anchors[0], input.screenshot_size, "底部锚点 1")?,
        normalize_pixel_point(input.bottom_anchors[1], input.screenshot_size, "底部锚点 2")?,
        normalize_pixel_point(input.bottom_anchors[2], input.screenshot_size, "底部锚点 3")?,
    ];
    let bottom_button_region = normalize_pixel_rect(
        input.bottom_button_region,
        input.screenshot_size,
        "底部按钮区域",
    )?;

    Ok(CalibrationConfig::new(
        input.screenshot_size,
        name_regions,
        bottom_anchors,
        bottom_button_region,
    ))
}

pub fn build_calibration_echo(config: &CalibrationConfig) -> Result<CalibrationEcho, String> {
    config.validate()?;
    Ok(CalibrationEcho {
        screenshot_size: config.screenshot_size,
        name_region_pixels: [
            denormalize_rect(config.name_regions[0], config.screenshot_size)?,
            denormalize_rect(config.name_regions[1], config.screenshot_size)?,
            denormalize_rect(config.name_regions[2], config.screenshot_size)?,
        ],
        bottom_anchor_pixels: [
            denormalize_point(config.bottom_anchors[0], config.screenshot_size)?,
            denormalize_point(config.bottom_anchors[1], config.screenshot_size)?,
            denormalize_point(config.bottom_anchors[2], config.screenshot_size)?,
        ],
        bottom_button_region_pixels: denormalize_rect(
            config.bottom_button_region,
            config.screenshot_size,
        )?,
        name_regions: config.name_regions,
        bottom_anchors: config.bottom_anchors,
        bottom_button_region: config.bottom_button_region,
    })
}

pub fn normalize_rect(
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    screenshot_size: ScreenshotSize,
) -> Result<NormalizedRect, String> {
    if screenshot_size.width == 0 || screenshot_size.height == 0 {
        return Err("截图尺寸不能为 0".to_string());
    }
    if width == 0 || height == 0 {
        return Err("区域尺寸不能为 0".to_string());
    }
    if x.saturating_add(width) > screenshot_size.width
        || y.saturating_add(height) > screenshot_size.height
    {
        return Err("区域超出截图尺寸".to_string());
    }

    Ok(NormalizedRect {
        x: f64::from(x) / f64::from(screenshot_size.width),
        y: f64::from(y) / f64::from(screenshot_size.height),
        width: f64::from(width) / f64::from(screenshot_size.width),
        height: f64::from(height) / f64::from(screenshot_size.height),
    })
}

pub fn normalize_pixel_rect(
    rect: PixelRect,
    screenshot_size: ScreenshotSize,
    label: &str,
) -> Result<NormalizedRect, String> {
    normalize_rect(rect.x, rect.y, rect.width, rect.height, screenshot_size)
        .map_err(|error| format!("{label}: {error}"))
}

pub fn normalize_point(
    x: u32,
    y: u32,
    screenshot_size: ScreenshotSize,
) -> Result<NormalizedPoint, String> {
    if screenshot_size.width == 0 || screenshot_size.height == 0 {
        return Err("截图尺寸不能为 0".to_string());
    }
    if x > screenshot_size.width || y > screenshot_size.height {
        return Err("点位超出截图尺寸".to_string());
    }

    Ok(NormalizedPoint {
        x: f64::from(x) / f64::from(screenshot_size.width),
        y: f64::from(y) / f64::from(screenshot_size.height),
    })
}

pub fn normalize_pixel_point(
    point: PixelPoint,
    screenshot_size: ScreenshotSize,
    label: &str,
) -> Result<NormalizedPoint, String> {
    normalize_point(point.x, point.y, screenshot_size).map_err(|error| format!("{label}: {error}"))
}

pub fn denormalize_rect(
    rect: NormalizedRect,
    screenshot_size: ScreenshotSize,
) -> Result<PixelRect, String> {
    validate_rect(rect, "归一化区域")?;
    if screenshot_size.width == 0 || screenshot_size.height == 0 {
        return Err("截图尺寸不能为 0".to_string());
    }

    Ok(PixelRect {
        x: (rect.x * f64::from(screenshot_size.width)).round() as u32,
        y: (rect.y * f64::from(screenshot_size.height)).round() as u32,
        width: (rect.width * f64::from(screenshot_size.width)).round() as u32,
        height: (rect.height * f64::from(screenshot_size.height)).round() as u32,
    })
}

pub fn denormalize_point(
    point: NormalizedPoint,
    screenshot_size: ScreenshotSize,
) -> Result<PixelPoint, String> {
    validate_point(point, "归一化点位")?;
    if screenshot_size.width == 0 || screenshot_size.height == 0 {
        return Err("截图尺寸不能为 0".to_string());
    }

    Ok(PixelPoint {
        x: (point.x * f64::from(screenshot_size.width)).round() as u32,
        y: (point.y * f64::from(screenshot_size.height)).round() as u32,
    })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PixelPoint {
    pub x: u32,
    pub y: u32,
}

fn validate_rect(rect: NormalizedRect, label: &str) -> Result<(), String> {
    for (field, value) in [
        ("x", rect.x),
        ("y", rect.y),
        ("width", rect.width),
        ("height", rect.height),
    ] {
        if !value.is_finite() {
            return Err(format!("{label} 的 {field} 不是有效数字"));
        }
    }
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return Err(format!("{label} 的宽高必须大于 0"));
    }
    if rect.x < 0.0 || rect.y < 0.0 || rect.x + rect.width > 1.0 || rect.y + rect.height > 1.0 {
        return Err(format!("{label} 必须位于 0..1 的归一化坐标内"));
    }
    Ok(())
}

fn validate_point(point: NormalizedPoint, label: &str) -> Result<(), String> {
    if !point.x.is_finite() || !point.y.is_finite() {
        return Err(format!("{label} 不是有效数字"));
    }
    if point.x < 0.0 || point.x > 1.0 || point.y < 0.0 || point.y > 1.0 {
        return Err(format!("{label} 必须位于 0..1 的归一化坐标内"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn normalizes_and_denormalizes_rect() {
        let size = ScreenshotSize {
            width: 1920,
            height: 1080,
        };

        let rect = normalize_rect(192, 108, 384, 216, size).expect("应能归一化区域");
        let pixels = denormalize_rect(rect, size).expect("应能还原像素区域");

        assert_eq!(
            rect,
            NormalizedRect {
                x: 0.1,
                y: 0.1,
                width: 0.2,
                height: 0.2
            }
        );
        assert_eq!(
            pixels,
            PixelRect {
                x: 192,
                y: 108,
                width: 384,
                height: 216
            }
        );
    }

    #[test]
    fn validates_required_regions_and_anchors() {
        let config = sample_config();

        config.validate().expect("有效校准配置应通过校验");
    }

    #[test]
    fn rejects_out_of_bounds_region() {
        let mut config = sample_config();
        config.name_regions[0].x = 0.9;
        config.name_regions[0].width = 0.2;

        let error = config.validate().expect_err("越界区域应失败");

        assert!(error.contains("名称区域 1"));
    }

    #[test]
    fn saves_and_loads_calibration_config() {
        let root = temp_root("calibration");
        let config = sample_config();

        let path = save_calibration_config(&root, &config).expect("应能保存校准配置");
        let loaded = load_calibration_config(&root).expect("应能读取校准配置");

        assert!(path.ends_with("calibration/screen-calibration.json"));
        assert_eq!(loaded, config);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn builds_and_echoes_pixel_calibration_profile() {
        let input = PixelCalibrationInput {
            screenshot_size: ScreenshotSize {
                width: 1920,
                height: 1080,
            },
            name_regions: [
                PixelRect {
                    x: 192,
                    y: 108,
                    width: 192,
                    height: 54,
                },
                PixelRect {
                    x: 864,
                    y: 108,
                    width: 192,
                    height: 54,
                },
                PixelRect {
                    x: 1536,
                    y: 108,
                    width: 192,
                    height: 54,
                },
            ],
            bottom_anchors: [
                PixelPoint { x: 384, y: 972 },
                PixelPoint { x: 960, y: 972 },
                PixelPoint { x: 1536, y: 972 },
            ],
            bottom_button_region: PixelRect {
                x: 768,
                y: 886,
                width: 384,
                height: 86,
            },
        };

        let config =
            build_calibration_config_from_pixels(input).expect("像素校准输入应能转换为配置");
        let echo = build_calibration_echo(&config).expect("应能生成回显数据");

        assert_eq!(config.screenshot_size.width, 1920);
        assert_eq!(config.name_regions[0].x, 0.1);
        assert_eq!(echo.name_region_pixels[1].x, 864);
        assert_eq!(echo.bottom_anchor_pixels[2].y, 972);
        assert_eq!(echo.bottom_button_region_pixels.width, 384);
    }

    #[test]
    fn saves_pixel_calibration_profile_with_path_and_echo() {
        let root = temp_root("pixel-calibration");
        let input = PixelCalibrationInput {
            screenshot_size: ScreenshotSize {
                width: 1000,
                height: 500,
            },
            name_regions: [
                PixelRect {
                    x: 10,
                    y: 20,
                    width: 100,
                    height: 30,
                },
                PixelRect {
                    x: 450,
                    y: 20,
                    width: 100,
                    height: 30,
                },
                PixelRect {
                    x: 800,
                    y: 20,
                    width: 100,
                    height: 30,
                },
            ],
            bottom_anchors: [
                PixelPoint { x: 100, y: 450 },
                PixelPoint { x: 500, y: 450 },
                PixelPoint { x: 900, y: 450 },
            ],
            bottom_button_region: PixelRect {
                x: 400,
                y: 420,
                width: 200,
                height: 50,
            },
        };

        let saved = save_pixel_calibration_config(&root, input).expect("应能保存像素校准配置");
        let loaded = load_calibration_profile(&root).expect("应能加载校准配置和回显");

        assert!(saved.path.ends_with("calibration/screen-calibration.json"));
        assert_eq!(saved.config, loaded.config);
        assert_eq!(loaded.echo.bottom_button_region_pixels.height, 50);

        let _ = fs::remove_dir_all(root);
    }

    fn sample_config() -> CalibrationConfig {
        CalibrationConfig::new(
            ScreenshotSize {
                width: 1920,
                height: 1080,
            },
            [
                NormalizedRect {
                    x: 0.1,
                    y: 0.2,
                    width: 0.1,
                    height: 0.05,
                },
                NormalizedRect {
                    x: 0.45,
                    y: 0.2,
                    width: 0.1,
                    height: 0.05,
                },
                NormalizedRect {
                    x: 0.8,
                    y: 0.2,
                    width: 0.1,
                    height: 0.05,
                },
            ],
            [
                NormalizedPoint { x: 0.2, y: 0.9 },
                NormalizedPoint { x: 0.5, y: 0.9 },
                NormalizedPoint { x: 0.8, y: 0.9 },
            ],
            NormalizedRect {
                x: 0.4,
                y: 0.82,
                width: 0.2,
                height: 0.08,
            },
        )
    }

    fn temp_root(label: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间应可用")
            .as_micros();
        std::env::temp_dir().join(format!("hex-assistant-{label}-{suffix}"))
    }
}
