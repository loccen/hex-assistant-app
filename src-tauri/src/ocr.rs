#![allow(dead_code)]

use ort::session::Session;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const PPOCR_V4_REC_MODEL: &str = "ppocrv4_rec.onnx";
pub const AUGMENT_DICTIONARY_ZH_CN: &str = "augments.zh-CN.json";
pub const CALIBRATED_NAME_SLOT_COUNT: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrErrorCode {
    ModelMissing,
    DictionaryMissing,
    InvalidDictionary,
    InvalidReplayInput,
    OrtSession,
    InferenceNotImplemented,
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
}

impl PpOcrResourcePaths {
    pub fn from_resource_root(resource_root: impl AsRef<Path>) -> Self {
        Self {
            model_path: resource_root
                .as_ref()
                .join("models")
                .join(PPOCR_V4_REC_MODEL),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OcrResourceStatus {
    pub engine: String,
    pub model_path: PathBuf,
    pub model_exists: bool,
    pub ready: bool,
    pub error_code: Option<String>,
    pub message: String,
}

pub fn check_ppocr_resources(resource_root: impl AsRef<Path>) -> OcrResourceStatus {
    let paths = PpOcrResourcePaths::from_resource_root(resource_root);
    let model_exists = paths.model_path.is_file();
    OcrResourceStatus {
        engine: "ppocr-v4-rec-onnx".to_string(),
        model_path: paths.model_path.clone(),
        model_exists,
        ready: model_exists,
        error_code: (!model_exists).then(|| "HEX-OCR-MODEL-MISSING".to_string()),
        message: if model_exists {
            "PP-OCRv4 rec ONNX 模型文件已就绪".to_string()
        } else {
            format!(
                "缺少 PP-OCRv4 rec ONNX 模型文件，请放置到 {}",
                paths.model_path.display()
            )
        },
    }
}

pub struct PpOcrV4RecRecognizer {
    model_path: PathBuf,
    _session: Session,
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

        let session = Session::builder()
            .and_then(|mut builder| builder.commit_from_file(&paths.model_path))
            .map_err(|error| {
                OcrError::new(
                    OcrErrorCode::OrtSession,
                    format!("无法通过 ORT 加载 PP-OCRv4 rec ONNX 模型: {error}"),
                    Some(paths.model_path.clone()),
                )
            })?;

        Ok(Self {
            model_path: paths.model_path,
            _session: session,
        })
    }

    pub fn model_path(&self) -> &Path {
        &self.model_path
    }

    pub fn recognize_line(
        &self,
        _image_rgb: &[u8],
        _width: u32,
        _height: u32,
    ) -> OcrResult<OcrText> {
        Err(OcrError::new(
            OcrErrorCode::InferenceNotImplemented,
            "PP-OCRv4 rec 的图像预处理和 CTC 解码尚未接入；当前模块只提供 ORT 会话骨架",
            Some(self.model_path.clone()),
        ))
    }
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
}
