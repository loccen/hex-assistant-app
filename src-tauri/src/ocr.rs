#![allow(dead_code)]

#[cfg(test)]
#[path = "calibration.rs"]
mod calibration;

#[cfg(not(test))]
use crate::calibration::{denormalize_rect, CalibrationConfig, PixelRect, ScreenshotSize};
#[cfg(test)]
use calibration::{denormalize_rect, CalibrationConfig, PixelRect, ScreenshotSize};
use chrono::Utc;
use image::{imageops, DynamicImage, ImageBuffer, Rgb, RgbImage};
use ndarray::{Array4, Axis};
use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const PPOCR_V4_REC_MODEL: &str = "ppocrv4_rec.onnx";
pub const PPOCR_V4_REC_CHARACTERS: &str = "ppocrv4_rec_chars.txt";
pub const AUGMENT_DICTIONARY_ZH_CN: &str = "augments.zh-CN.json";
pub const CALIBRATED_NAME_SLOT_COUNT: usize = 3;
const PPOCR_REC_IMAGE_HEIGHT: u32 = 48;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrErrorCode {
    ModelMissing,
    DictionaryMissing,
    CharacterTableMissing,
    InvalidDictionary,
    InvalidImage,
    InvalidCalibration,
    InvalidReplayInput,
    OrtSession,
    Inference,
    Decode,
    Io,
    Serialization,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrError {
    pub code: OcrErrorCode,
    pub message: String,
    pub path: Option<PathBuf>,
}

impl OcrError {
    fn new(code: OcrErrorCode, message: impl Into<String>, path: Option<PathBuf>) -> Self {
        Self {
            code,
            message: message.into(),
            path,
        }
    }
}

impl std::fmt::Display for OcrError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.path {
            Some(path) => write!(formatter, "{}: {}", self.message, path.display()),
            None => write!(formatter, "{}", self.message),
        }
    }
}

impl std::error::Error for OcrError {}

pub type OcrResult<T> = Result<T, OcrError>;

#[derive(Debug, Clone)]
pub struct PpOcrResourcePaths {
    pub model_path: PathBuf,
    pub augment_dictionary_path: PathBuf,
    pub character_dictionary_path: PathBuf,
}

impl PpOcrResourcePaths {
    pub fn from_resource_root(resource_root: impl AsRef<Path>) -> Self {
        let resource_root = resource_root.as_ref();
        Self {
            model_path: resource_root.join("models").join(PPOCR_V4_REC_MODEL),
            augment_dictionary_path: resource_root
                .join("dictionaries")
                .join(AUGMENT_DICTIONARY_ZH_CN),
            character_dictionary_path: resource_root
                .join("dictionaries")
                .join(PPOCR_V4_REC_CHARACTERS),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OcrResourceStatus {
    pub engine: String,
    pub model_path: PathBuf,
    pub model_exists: bool,
    pub augment_dictionary_path: PathBuf,
    pub augment_dictionary_exists: bool,
    pub character_dictionary_path: PathBuf,
    pub character_dictionary_exists: bool,
    pub ready: bool,
    pub error_code: Option<String>,
    pub message: String,
}

pub fn check_ppocr_resources(resource_root: impl AsRef<Path>) -> OcrResourceStatus {
    let paths = PpOcrResourcePaths::from_resource_root(resource_root);
    let model_exists = paths.model_path.is_file();
    let augment_dictionary_exists = paths.augment_dictionary_path.is_file();
    let character_dictionary_exists = paths.character_dictionary_path.is_file();
    let ready = model_exists && augment_dictionary_exists;
    let error_code = if !model_exists {
        Some("HEX-OCR-MODEL-MISSING".to_string())
    } else if !augment_dictionary_exists {
        Some("HEX-OCR-DICTIONARY-MISSING".to_string())
    } else {
        None
    };
    let message = if ready {
        if character_dictionary_exists {
            "PP-OCRv4 rec ONNX 模型、海克斯词库和外置字符表已就绪".to_string()
        } else {
            "PP-OCRv4 rec ONNX 模型和海克斯词库已就绪；字符表将优先从模型 metadata 读取".to_string()
        }
    } else if !model_exists {
        format!(
            "缺少 PP-OCRv4 rec ONNX 模型文件，请放置到 {}",
            paths.model_path.display()
        )
    } else {
        format!(
            "缺少海克斯词库文件，请放置到 {}",
            paths.augment_dictionary_path.display()
        )
    };
    OcrResourceStatus {
        engine: "ppocr-v4-rec-onnx".to_string(),
        model_path: paths.model_path.clone(),
        model_exists,
        augment_dictionary_path: paths.augment_dictionary_path.clone(),
        augment_dictionary_exists,
        character_dictionary_path: paths.character_dictionary_path.clone(),
        character_dictionary_exists,
        ready,
        error_code,
        message,
    }
}

pub struct PpOcrV4RecRecognizer {
    model_path: PathBuf,
    character_dictionary_path: PathBuf,
    input_name: String,
    output_name: String,
    char_list: Vec<String>,
    session: Session,
}

impl PpOcrV4RecRecognizer {
    pub fn from_resource_root(resource_root: impl AsRef<Path>) -> OcrResult<Self> {
        let paths = PpOcrResourcePaths::from_resource_root(resource_root);
        if !paths.model_path.is_file() {
            return Err(OcrError::new(
                OcrErrorCode::ModelMissing,
                "缺少 PP-OCRv4 rec ONNX 模型文件",
                Some(paths.model_path),
            ));
        }

        let builder = Session::builder().map_err(|error| {
            OcrError::new(
                OcrErrorCode::OrtSession,
                format!("无法创建 PP-OCRv4 rec ORT 会话构造器: {error}"),
                Some(paths.model_path.clone()),
            )
        })?;
        let mut builder = builder
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|error| {
                OcrError::new(
                    OcrErrorCode::OrtSession,
                    format!("无法设置 PP-OCRv4 rec ORT 图优化级别: {error}"),
                    Some(paths.model_path.clone()),
                )
            })?;
        let session = builder
            .commit_from_file(&paths.model_path)
            .map_err(|error| {
                OcrError::new(
                    OcrErrorCode::OrtSession,
                    format!("无法通过 ORT 加载 PP-OCRv4 rec ONNX 模型: {error}"),
                    Some(paths.model_path.clone()),
                )
            })?;
        let input_name = session
            .inputs()
            .first()
            .map(|input| input.name().to_string())
            .ok_or_else(|| {
                OcrError::new(
                    OcrErrorCode::OrtSession,
                    "PP-OCRv4 rec ONNX 模型没有输入节点",
                    Some(paths.model_path.clone()),
                )
            })?;
        let output_name = session
            .outputs()
            .first()
            .map(|output| output.name().to_string())
            .ok_or_else(|| {
                OcrError::new(
                    OcrErrorCode::OrtSession,
                    "PP-OCRv4 rec ONNX 模型没有输出节点",
                    Some(paths.model_path.clone()),
                )
            })?;
        let char_list = load_character_table(&session, &paths.character_dictionary_path)?;

        Ok(Self {
            model_path: paths.model_path,
            character_dictionary_path: paths.character_dictionary_path,
            input_name,
            output_name,
            char_list,
            session,
        })
    }

    pub fn model_path(&self) -> &Path {
        &self.model_path
    }

    pub fn recognize_line(
        &mut self,
        image_rgb: &[u8],
        width: u32,
        height: u32,
    ) -> OcrResult<OcrText> {
        if width == 0 || height == 0 {
            return Err(OcrError::new(
                OcrErrorCode::InvalidImage,
                "OCR 输入图片宽高不能为 0",
                None,
            ));
        }
        let expected_len = width as usize * height as usize * 3;
        if image_rgb.len() != expected_len {
            return Err(OcrError::new(
                OcrErrorCode::InvalidImage,
                format!(
                    "OCR 输入 RGB 数据长度不匹配: 期望 {expected_len} 字节，实际 {} 字节",
                    image_rgb.len()
                ),
                None,
            ));
        }
        let image = RgbImage::from_raw(width, height, image_rgb.to_vec()).ok_or_else(|| {
            OcrError::new(
                OcrErrorCode::InvalidImage,
                "无法从 RGB 数据构造 OCR 输入图片",
                None,
            )
        })?;
        self.recognize_dynamic(&DynamicImage::ImageRgb8(image))
    }

    pub fn recognize_dynamic(&mut self, image: &DynamicImage) -> OcrResult<OcrText> {
        let input_array = preprocess_ppocr_rec(image)?;
        let shape = input_array.shape().to_vec();
        let data = input_array.into_raw_vec_and_offset().0;
        let tensor = Tensor::<f32>::from_array((shape, data)).map_err(|error| {
            OcrError::new(
                OcrErrorCode::Inference,
                format!("无法创建 PP-OCRv4 rec 输入张量: {error}"),
                Some(self.model_path.clone()),
            )
        })?;
        let outputs = self
            .session
            .run(ort::inputs![self.input_name.as_str() => tensor])
            .map_err(|error| {
                OcrError::new(
                    OcrErrorCode::Inference,
                    format!("PP-OCRv4 rec ONNX 推理失败: {error}"),
                    Some(self.model_path.clone()),
                )
            })?;
        let output = outputs.get(self.output_name.as_str()).ok_or_else(|| {
            OcrError::new(
                OcrErrorCode::Inference,
                format!("PP-OCRv4 rec 输出中缺少节点 {}", self.output_name),
                Some(self.model_path.clone()),
            )
        })?;
        let output_array = output.try_extract_array::<f32>().map_err(|error| {
            OcrError::new(
                OcrErrorCode::Inference,
                format!("无法提取 PP-OCRv4 rec 输出张量: {error}"),
                Some(self.model_path.clone()),
            )
        })?;
        let (raw_text, confidence) = ctc_decode(output_array, &self.char_list)?;
        Ok(OcrText {
            raw_text,
            confidence,
        })
    }
}

fn load_character_table(
    session: &Session,
    character_dictionary_path: &Path,
) -> OcrResult<Vec<String>> {
    let metadata_characters = session
        .metadata()
        .ok()
        .and_then(|metadata| metadata.custom("character"));

    if let Some(raw) = metadata_characters {
        let table = parse_character_table(&raw);
        if table.len() > 2 {
            return Ok(table);
        }
    }

    let raw = fs::read_to_string(character_dictionary_path).map_err(|error| {
        OcrError::new(
            OcrErrorCode::CharacterTableMissing,
            format!(
                "模型 metadata 未提供有效 character 字符表，且无法读取外置 PP-OCR 字符表: {error}"
            ),
            Some(character_dictionary_path.to_path_buf()),
        )
    })?;
    let table = parse_character_table(&raw);
    if table.len() <= 2 {
        return Err(OcrError::new(
            OcrErrorCode::InvalidDictionary,
            "PP-OCR 字符表为空，无法执行 CTC 解码",
            Some(character_dictionary_path.to_path_buf()),
        ));
    }
    Ok(table)
}

fn parse_character_table(raw: &str) -> Vec<String> {
    let mut list = vec!["blank".to_string()];
    list.extend(
        raw.lines()
            .map(str::trim_end)
            .filter(|line| !line.is_empty())
            .map(ToString::to_string),
    );
    list.push(" ".to_string());
    list
}

fn preprocess_ppocr_rec(image: &DynamicImage) -> OcrResult<Array4<f32>> {
    if image.width() == 0 || image.height() == 0 {
        return Err(OcrError::new(
            OcrErrorCode::InvalidImage,
            "OCR 预处理图片宽高不能为 0",
            None,
        ));
    }

    let target_height = PPOCR_REC_IMAGE_HEIGHT;
    let target_width = ((image.width() as f64 * target_height as f64 / image.height() as f64)
        .round() as u32)
        .max(target_height);
    let resized = image.resize_exact(target_width, target_height, imageops::FilterType::Triangle);
    let rgb = resized.to_rgb8();
    let width = target_width as usize;
    let height = target_height as usize;
    let mut tensor = Array4::<f32>::zeros((1, 3, height, width));

    for y in 0..height {
        for x in 0..width {
            let pixel = rgb.get_pixel(x as u32, y as u32);
            tensor[[0, 0, y, x]] = normalize_ppocr_channel(pixel[0]);
            tensor[[0, 1, y, x]] = normalize_ppocr_channel(pixel[1]);
            tensor[[0, 2, y, x]] = normalize_ppocr_channel(pixel[2]);
        }
    }

    Ok(tensor)
}

fn normalize_ppocr_channel(value: u8) -> f32 {
    (f32::from(value) / 255.0 - 0.5) / 0.5
}

fn ctc_decode(output: ndarray::ArrayViewD<f32>, char_list: &[String]) -> OcrResult<(String, f32)> {
    if output.ndim() != 3 {
        return Err(OcrError::new(
            OcrErrorCode::Decode,
            format!(
                "PP-OCRv4 rec 输出维度应为 [1,T,C]，实际为 {} 维",
                output.ndim()
            ),
            None,
        ));
    }
    if output.shape()[0] != 1 {
        return Err(OcrError::new(
            OcrErrorCode::Decode,
            format!(
                "PP-OCRv4 rec 当前只支持 batch=1，实际为 {}",
                output.shape()[0]
            ),
            None,
        ));
    }

    let steps = output.shape()[1];
    let mut text = String::new();
    let mut confidences = Vec::new();
    let mut previous_index = usize::MAX;

    for step_index in 0..steps {
        let step = output.index_axis(Axis(1), step_index);
        let row = step.index_axis(Axis(0), 0);
        let (index, score) = row
            .iter()
            .copied()
            .enumerate()
            .max_by(|left, right| left.1.total_cmp(&right.1))
            .unwrap_or((0, 0.0));

        if index != 0 && index != previous_index {
            if let Some(character) = char_list.get(index) {
                if character != "blank" {
                    text.push_str(character);
                    confidences.push(score);
                }
            }
        }
        previous_index = index;
    }

    let confidence = if confidences.is_empty() {
        0.0
    } else {
        confidences.iter().sum::<f32>() / confidences.len() as f32
    };
    Ok((text, confidence))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OcrText {
    pub raw_text: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AugmentDictionary {
    pub locale: String,
    pub version: u32,
    pub augments: Vec<AugmentEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AugmentEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

impl AugmentDictionary {
    pub fn load(path: impl AsRef<Path>) -> OcrResult<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path).map_err(|error| {
            OcrError::new(
                OcrErrorCode::DictionaryMissing,
                format!("无法读取海克斯词库: {error}"),
                Some(path.to_path_buf()),
            )
        })?;
        serde_json::from_str(&content).map_err(|error| {
            OcrError::new(
                OcrErrorCode::InvalidDictionary,
                format!("海克斯词库 JSON 格式无效: {error}"),
                Some(path.to_path_buf()),
            )
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MatchResult {
    pub raw_text: String,
    pub confidence: f32,
    pub match_score: f32,
    pub final_name: Option<String>,
    pub augment_id: Option<String>,
    pub failure_reason: Option<String>,
}

pub fn match_augment_name(
    dictionary: &AugmentDictionary,
    raw_text: impl AsRef<str>,
    confidence: f32,
    min_confidence: f32,
    min_match_score: f32,
) -> MatchResult {
    let raw_text = raw_text.as_ref().to_string();
    let normalized_raw = normalize_name(&raw_text);
    if normalized_raw.is_empty() {
        return MatchResult {
            raw_text,
            confidence,
            match_score: 0.0,
            final_name: None,
            augment_id: None,
            failure_reason: Some("OCR 原始文本为空".to_string()),
        };
    }

    let best = dictionary
        .augments
        .iter()
        .flat_map(|entry| {
            std::iter::once(entry.name.as_str())
                .chain(entry.aliases.iter().map(String::as_str))
                .map(move |candidate| (entry, candidate))
        })
        .map(|(entry, candidate)| {
            let score = similarity_score(&normalized_raw, &normalize_name(candidate));
            (entry, score)
        })
        .max_by(|left, right| left.1.total_cmp(&right.1));

    let Some((entry, match_score)) = best else {
        return MatchResult {
            raw_text,
            confidence,
            match_score: 0.0,
            final_name: None,
            augment_id: None,
            failure_reason: Some("海克斯词库为空".to_string()),
        };
    };

    let failure_reason = if confidence < min_confidence {
        Some(format!(
            "OCR 置信度 {confidence:.3} 低于阈值 {min_confidence:.3}"
        ))
    } else if match_score < min_match_score {
        Some(format!(
            "词库匹配分 {match_score:.3} 低于阈值 {min_match_score:.3}"
        ))
    } else {
        None
    };

    MatchResult {
        raw_text,
        confidence,
        match_score,
        final_name: failure_reason.is_none().then(|| entry.name.clone()),
        augment_id: failure_reason.is_none().then(|| entry.id.clone()),
        failure_reason,
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CalibratedNameSlot {
    Left,
    Center,
    Right,
}

impl CalibratedNameSlot {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SlotReplayInput {
    pub slot: CalibratedNameSlot,
    pub raw_text: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SlotReplayReport {
    pub slot: CalibratedNameSlot,
    pub raw_text: String,
    pub confidence: f32,
    pub match_score: f32,
    pub final_name: Option<String>,
    pub augment_id: Option<String>,
    pub elapsed_ms: u128,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OfflineReplayReport {
    pub engine: String,
    pub slot_count: usize,
    pub min_confidence: f32,
    pub min_match_score: f32,
    pub slots: Vec<SlotReplayReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalibratedNameOcrReport {
    pub engine: String,
    pub created_at: String,
    pub source_image_path: PathBuf,
    pub output_dir: PathBuf,
    pub report_path: PathBuf,
    pub slot_count: usize,
    pub min_confidence: f32,
    pub min_match_score: f32,
    pub elapsed_ms: u128,
    pub slots: Vec<CalibratedNameOcrSlotReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalibratedNameOcrSlotReport {
    pub slot: CalibratedNameSlot,
    pub crop_rect: PixelRectReport,
    pub crop_path: PathBuf,
    pub enhanced_path: PathBuf,
    pub raw_text: String,
    pub confidence: f32,
    pub match_score: f32,
    pub final_name: Option<String>,
    pub augment_id: Option<String>,
    pub elapsed_ms: u128,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PixelRectReport {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl From<PixelRect> for PixelRectReport {
    fn from(rect: PixelRect) -> Self {
        Self {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        }
    }
}

pub fn recognize_calibrated_name_slots_from_image(
    recognizer: &mut PpOcrV4RecRecognizer,
    dictionary: &AugmentDictionary,
    calibration: &CalibrationConfig,
    image_path: impl AsRef<Path>,
    output_root: impl AsRef<Path>,
    min_confidence: f32,
    min_match_score: f32,
) -> OcrResult<CalibratedNameOcrReport> {
    calibration.validate().map_err(|error| {
        OcrError::new(
            OcrErrorCode::InvalidCalibration,
            format!("校准配置无效: {error}"),
            None,
        )
    })?;

    let image_path = image_path.as_ref();
    let image = image::open(image_path).map_err(|error| {
        OcrError::new(
            OcrErrorCode::InvalidImage,
            format!("无法读取 OCR 源图片: {error}"),
            Some(image_path.to_path_buf()),
        )
    })?;
    if image.width() == 0 || image.height() == 0 {
        return Err(OcrError::new(
            OcrErrorCode::InvalidImage,
            "OCR 源图片宽高不能为 0",
            Some(image_path.to_path_buf()),
        ));
    }

    let total_start = Instant::now();
    let output_dir = output_root.as_ref().join(format!(
        "calibrated-name-slots-{}",
        timestamp_for_filename()
    ));
    fs::create_dir_all(&output_dir).map_err(|error| {
        OcrError::new(
            OcrErrorCode::Io,
            format!("无法创建 OCR 报告目录: {error}"),
            Some(output_dir.clone()),
        )
    })?;

    let screenshot_size = ScreenshotSize {
        width: image.width(),
        height: image.height(),
    };
    let slots = [
        CalibratedNameSlot::Left,
        CalibratedNameSlot::Center,
        CalibratedNameSlot::Right,
    ];
    let mut reports = Vec::with_capacity(CALIBRATED_NAME_SLOT_COUNT);

    for (index, slot) in slots.iter().copied().enumerate() {
        let slot_start = Instant::now();
        let rect = denormalize_rect(calibration.name_regions[index], screenshot_size).map_err(
            |error| {
                OcrError::new(
                    OcrErrorCode::InvalidCalibration,
                    format!("无法还原第 {} 个名称区域: {error}", index + 1),
                    None,
                )
            },
        )?;
        let rect = clamp_pixel_rect(rect, image.width(), image.height()).ok_or_else(|| {
            OcrError::new(
                OcrErrorCode::InvalidCalibration,
                format!("第 {} 个名称区域裁剪后为空", index + 1),
                None,
            )
        })?;
        let crop = image.crop_imm(rect.x, rect.y, rect.width, rect.height);
        let enhanced = enhance_name_crop(&crop);
        let crop_path = output_dir.join(format!("slot-{}-crop.png", slot.as_str()));
        let enhanced_path = output_dir.join(format!("slot-{}-enhanced.png", slot.as_str()));
        crop.save(&crop_path).map_err(|error| {
            OcrError::new(
                OcrErrorCode::Io,
                format!("无法写入 slot 裁剪图: {error}"),
                Some(crop_path.clone()),
            )
        })?;
        enhanced.save(&enhanced_path).map_err(|error| {
            OcrError::new(
                OcrErrorCode::Io,
                format!("无法写入 slot 增强图: {error}"),
                Some(enhanced_path.clone()),
            )
        })?;

        let matched = match recognizer.recognize_dynamic(&enhanced) {
            Ok(recognized) => match_augment_name(
                dictionary,
                &recognized.raw_text,
                recognized.confidence,
                min_confidence,
                min_match_score,
            ),
            Err(error) => MatchResult {
                raw_text: String::new(),
                confidence: 0.0,
                match_score: 0.0,
                final_name: None,
                augment_id: None,
                failure_reason: Some(error.to_string()),
            },
        };
        reports.push(CalibratedNameOcrSlotReport {
            slot,
            crop_rect: rect.into(),
            crop_path,
            enhanced_path,
            raw_text: matched.raw_text,
            confidence: matched.confidence,
            match_score: matched.match_score,
            final_name: matched.final_name,
            augment_id: matched.augment_id,
            elapsed_ms: slot_start.elapsed().as_millis(),
            failure_reason: matched.failure_reason,
        });
    }

    let report_path = output_dir.join("report.json");
    let report = CalibratedNameOcrReport {
        engine: "ppocr-v4-rec-onnx".to_string(),
        created_at: Utc::now().to_rfc3339(),
        source_image_path: image_path.to_path_buf(),
        output_dir: output_dir.clone(),
        report_path: report_path.clone(),
        slot_count: reports.len(),
        min_confidence,
        min_match_score,
        elapsed_ms: total_start.elapsed().as_millis(),
        slots: reports,
    };
    write_calibrated_name_ocr_report(&report_path, &report)?;
    Ok(report)
}

pub fn write_calibrated_name_ocr_report(
    output_path: impl AsRef<Path>,
    report: &CalibratedNameOcrReport,
) -> OcrResult<PathBuf> {
    let output_path = output_path.as_ref();
    let content = serde_json::to_string_pretty(report).map_err(|error| {
        OcrError::new(
            OcrErrorCode::Serialization,
            format!("无法序列化 OCR 识别报告: {error}"),
            Some(output_path.to_path_buf()),
        )
    })?;
    fs::write(output_path, format!("{content}\n")).map_err(|error| {
        OcrError::new(
            OcrErrorCode::Io,
            format!("无法写入 OCR 识别报告: {error}"),
            Some(output_path.to_path_buf()),
        )
    })?;
    Ok(output_path.to_path_buf())
}

pub fn replay_calibrated_name_slots(
    dictionary: &AugmentDictionary,
    inputs: &[SlotReplayInput],
    min_confidence: f32,
    min_match_score: f32,
) -> OcrResult<OfflineReplayReport> {
    if inputs.len() != CALIBRATED_NAME_SLOT_COUNT {
        return Err(OcrError::new(
            OcrErrorCode::InvalidReplayInput,
            format!(
                "离线回放只接受 {CALIBRATED_NAME_SLOT_COUNT} 个校准名称区域，实际收到 {} 个",
                inputs.len()
            ),
            None,
        ));
    }

    let mut seen = Vec::with_capacity(CALIBRATED_NAME_SLOT_COUNT);
    let mut slots = Vec::with_capacity(CALIBRATED_NAME_SLOT_COUNT);
    for input in inputs {
        if seen.contains(&input.slot) {
            return Err(OcrError::new(
                OcrErrorCode::InvalidReplayInput,
                format!("离线回放存在重复名称区域 {}", input.slot.as_str()),
                None,
            ));
        }
        seen.push(input.slot);

        let start = Instant::now();
        let matched = match_augment_name(
            dictionary,
            &input.raw_text,
            input.confidence,
            min_confidence,
            min_match_score,
        );
        slots.push(SlotReplayReport {
            slot: input.slot,
            raw_text: matched.raw_text,
            confidence: matched.confidence,
            match_score: matched.match_score,
            final_name: matched.final_name,
            augment_id: matched.augment_id,
            elapsed_ms: start.elapsed().as_millis(),
            failure_reason: matched.failure_reason,
        });
    }

    Ok(OfflineReplayReport {
        engine: "offline-calibrated-name-slots".to_string(),
        slot_count: slots.len(),
        min_confidence,
        min_match_score,
        slots,
    })
}

pub fn write_offline_replay_report(
    output_dir: impl AsRef<Path>,
    report: &OfflineReplayReport,
) -> OcrResult<PathBuf> {
    let output_dir = output_dir.as_ref();
    fs::create_dir_all(output_dir).map_err(|error| {
        OcrError::new(
            OcrErrorCode::Io,
            format!("无法创建 OCR 离线回放目录: {error}"),
            Some(output_dir.to_path_buf()),
        )
    })?;

    let output_path = output_dir.join("calibrated-name-slots-report.json");
    let content = serde_json::to_string_pretty(report).map_err(|error| {
        OcrError::new(
            OcrErrorCode::Serialization,
            format!("无法序列化 OCR 离线回放报告: {error}"),
            Some(output_path.clone()),
        )
    })?;
    fs::write(&output_path, format!("{content}\n")).map_err(|error| {
        OcrError::new(
            OcrErrorCode::Io,
            format!("无法写入 OCR 离线回放报告: {error}"),
            Some(output_path.clone()),
        )
    })?;
    Ok(output_path)
}

fn clamp_pixel_rect(rect: PixelRect, image_width: u32, image_height: u32) -> Option<PixelRect> {
    if image_width == 0 || image_height == 0 {
        return None;
    }
    let x = rect.x.min(image_width);
    let y = rect.y.min(image_height);
    let right = rect.x.saturating_add(rect.width).min(image_width);
    let bottom = rect.y.saturating_add(rect.height).min(image_height);
    let width = right.saturating_sub(x);
    let height = bottom.saturating_sub(y);
    (width > 0 && height > 0).then_some(PixelRect {
        x,
        y,
        width,
        height,
    })
}

fn enhance_name_crop(crop: &DynamicImage) -> DynamicImage {
    let rgb = crop.to_rgb8();
    let mut min_luma = u8::MAX;
    let mut max_luma = u8::MIN;
    for pixel in rgb.pixels() {
        let luma = luma_from_rgb(pixel[0], pixel[1], pixel[2]);
        min_luma = min_luma.min(luma);
        max_luma = max_luma.max(luma);
    }

    let range = u16::from(max_luma.saturating_sub(min_luma)).max(1);
    let stretched: RgbImage = ImageBuffer::from_fn(rgb.width(), rgb.height(), |x, y| {
        let pixel = rgb.get_pixel(x, y);
        let adjust = |channel: u8| -> u8 {
            let shifted = channel.saturating_sub(min_luma);
            ((u16::from(shifted) * 255) / range).min(255) as u8
        };
        Rgb([adjust(pixel[0]), adjust(pixel[1]), adjust(pixel[2])])
    });
    let scaled_width = stretched.width().saturating_mul(2).max(1);
    let scaled_height = stretched.height().saturating_mul(2).max(1);
    DynamicImage::ImageRgb8(imageops::resize(
        &stretched,
        scaled_width,
        scaled_height,
        imageops::FilterType::CatmullRom,
    ))
}

fn luma_from_rgb(red: u8, green: u8, blue: u8) -> u8 {
    (0.299 * f64::from(red) + 0.587 * f64::from(green) + 0.114 * f64::from(blue)).round() as u8
}

fn timestamp_for_filename() -> String {
    Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string()
}

fn normalize_name(value: &str) -> String {
    value
        .chars()
        .filter_map(|character| {
            if character.is_alphanumeric() || is_cjk(character) {
                Some(character.to_lowercase().next().unwrap_or(character))
            } else {
                None
            }
        })
        .collect()
}

fn is_cjk(character: char) -> bool {
    matches!(
        character as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF
    )
}

fn similarity_score(left: &str, right: &str) -> f32 {
    if left.is_empty() && right.is_empty() {
        return 1.0;
    }
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let max_len = left.chars().count().max(right.chars().count()) as f32;
    1.0 - (levenshtein(left, right) as f32 / max_len)
}

fn levenshtein(left: &str, right: &str) -> usize {
    let left_chars: Vec<char> = left.chars().collect();
    let right_chars: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right_chars.len()).collect();
    let mut current = vec![0; right_chars.len() + 1];

    for (left_index, left_char) in left_chars.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_char) in right_chars.iter().enumerate() {
            let insertion = current[right_index] + 1;
            let deletion = previous[right_index + 1] + 1;
            let substitution = previous[right_index] + usize::from(left_char != right_char);
            current[right_index + 1] = insertion.min(deletion).min(substitution);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[right_chars.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dictionary() -> AugmentDictionary {
        AugmentDictionary {
            locale: "zh-CN".to_string(),
            version: 1,
            augments: vec![
                AugmentEntry {
                    id: "prismatic-ticket".to_string(),
                    name: "棱彩门票".to_string(),
                    aliases: vec!["彩色门票".to_string()],
                },
                AugmentEntry {
                    id: "build-a-bud".to_string(),
                    name: "好事成双".to_string(),
                    aliases: vec![],
                },
                AugmentEntry {
                    id: "trade-sector".to_string(),
                    name: "利滚利".to_string(),
                    aliases: vec![],
                },
            ],
        }
    }

    #[test]
    fn missing_model_returns_locatable_error() {
        let root = std::env::temp_dir().join(format!(
            "hex-ocr-missing-model-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let error = match PpOcrV4RecRecognizer::from_resource_root(&root) {
            Ok(_) => panic!("模型缺失时不应创建识别器"),
            Err(error) => error,
        };
        assert_eq!(error.code, OcrErrorCode::ModelMissing);
        assert_eq!(
            error.path.unwrap(),
            root.join("models").join(PPOCR_V4_REC_MODEL)
        );
    }

    #[test]
    fn resource_check_reports_missing_model_path() {
        let root = PathBuf::from("/tmp/hex-ocr-resource-check");
        let status = check_ppocr_resources(&root);
        assert!(!status.ready);
        assert_eq!(
            status.model_path,
            root.join("models").join(PPOCR_V4_REC_MODEL)
        );
        assert_eq!(status.error_code.as_deref(), Some("HEX-OCR-MODEL-MISSING"));
    }

    #[test]
    fn resource_check_reports_missing_augment_dictionary_after_model_exists() {
        let root = temp_root("ocr-resource-status");
        let model_dir = root.join("models");
        fs::create_dir_all(&model_dir).unwrap();
        fs::write(model_dir.join(PPOCR_V4_REC_MODEL), b"fake model").unwrap();

        let status = check_ppocr_resources(&root);

        assert!(!status.ready);
        assert!(status.model_exists);
        assert!(!status.augment_dictionary_exists);
        assert_eq!(
            status.error_code.as_deref(),
            Some("HEX-OCR-DICTIONARY-MISSING")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parses_character_table_with_blank_and_space() {
        let table = parse_character_table("你\n好\nA\n");

        assert_eq!(table.first().map(String::as_str), Some("blank"));
        assert_eq!(table[1], "你");
        assert_eq!(table[2], "好");
        assert_eq!(table.last().map(String::as_str), Some(" "));
    }

    #[test]
    fn preprocesses_rec_image_to_ppocr_tensor_shape() {
        let image =
            DynamicImage::ImageRgb8(ImageBuffer::from_fn(20, 10, |_, _| Rgb([255, 128, 0])));

        let tensor = preprocess_ppocr_rec(&image).expect("应能预处理 OCR 图片");

        assert_eq!(tensor.shape(), &[1, 3, 48, 96]);
        assert!((tensor[[0, 0, 0, 0]] - 1.0).abs() < 0.001);
        assert!(tensor[[0, 2, 0, 0]] < -0.99);
    }

    #[test]
    fn ctc_decode_skips_blank_and_repeated_indices() {
        let output = ndarray::Array3::from_shape_vec(
            (1, 5, 4),
            vec![
                0.1, 0.8, 0.1, 0.0, // a
                0.1, 0.7, 0.2, 0.0, // repeated a
                0.9, 0.0, 0.1, 0.0, // blank
                0.1, 0.2, 0.6, 0.1, // b
                0.1, 0.1, 0.1, 0.7, // space
            ],
        )
        .unwrap()
        .into_dyn();
        let characters = vec![
            "blank".to_string(),
            "a".to_string(),
            "b".to_string(),
            " ".to_string(),
        ];

        let (text, confidence) = ctc_decode(output.view(), &characters).unwrap();

        assert_eq!(text, "ab ");
        assert!((confidence - 0.7).abs() < 0.001);
    }

    #[test]
    fn clamps_pixel_rect_to_image_bounds() {
        let rect = PixelRect {
            x: 8,
            y: 8,
            width: 10,
            height: 10,
        };

        let clamped = clamp_pixel_rect(rect, 12, 13).expect("裁剪区域应仍有面积");

        assert_eq!(
            clamped,
            PixelRect {
                x: 8,
                y: 8,
                width: 4,
                height: 5
            }
        );
    }

    #[test]
    fn dictionary_match_uses_generic_similarity() {
        let matched = match_augment_name(&test_dictionary(), " 棱彩 门票 ", 0.95, 0.8, 0.8);
        assert_eq!(matched.final_name.as_deref(), Some("棱彩门票"));
        assert_eq!(matched.augment_id.as_deref(), Some("prismatic-ticket"));
        assert!(matched.failure_reason.is_none());
    }

    #[test]
    fn dictionary_match_rejects_low_confidence() {
        let matched = match_augment_name(&test_dictionary(), "棱彩门票", 0.2, 0.8, 0.8);
        assert_eq!(matched.final_name, None);
        assert!(matched.failure_reason.unwrap().contains("OCR 置信度"));
    }

    #[test]
    fn offline_replay_requires_three_slots() {
        let inputs = vec![SlotReplayInput {
            slot: CalibratedNameSlot::Left,
            raw_text: "棱彩门票".to_string(),
            confidence: 0.95,
        }];
        let error =
            replay_calibrated_name_slots(&test_dictionary(), &inputs, 0.8, 0.8).unwrap_err();
        assert_eq!(error.code, OcrErrorCode::InvalidReplayInput);
    }

    #[test]
    fn offline_replay_records_slot_details_and_failure_reason() {
        let inputs = vec![
            SlotReplayInput {
                slot: CalibratedNameSlot::Left,
                raw_text: "棱彩门票".to_string(),
                confidence: 0.95,
            },
            SlotReplayInput {
                slot: CalibratedNameSlot::Center,
                raw_text: "未知强化".to_string(),
                confidence: 0.95,
            },
            SlotReplayInput {
                slot: CalibratedNameSlot::Right,
                raw_text: "好事成双".to_string(),
                confidence: 0.3,
            },
        ];
        let report = replay_calibrated_name_slots(&test_dictionary(), &inputs, 0.8, 0.8).unwrap();
        assert_eq!(report.slot_count, 3);
        assert_eq!(report.slots[0].final_name.as_deref(), Some("棱彩门票"));
        assert_eq!(report.slots[0].raw_text, "棱彩门票");
        assert!(report.slots[0].match_score >= 0.99);
        assert!(report.slots[1].failure_reason.is_some());
        assert!(report.slots[2]
            .failure_reason
            .as_ref()
            .unwrap()
            .contains("OCR 置信度"));
    }

    #[test]
    fn write_replay_report_persists_json() {
        let root = std::env::temp_dir().join(format!(
            "hex-ocr-replay-report-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let report = OfflineReplayReport {
            engine: "offline-calibrated-name-slots".to_string(),
            slot_count: 3,
            min_confidence: 0.8,
            min_match_score: 0.8,
            slots: vec![],
        };
        let path = write_offline_replay_report(&root, &report).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("offline-calibrated-name-slots"));
        fs::remove_dir_all(root).unwrap();
    }

    fn temp_root(label: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间应可用")
            .as_micros();
        std::env::temp_dir().join(format!("hex-assistant-{label}-{suffix}"))
    }
}
