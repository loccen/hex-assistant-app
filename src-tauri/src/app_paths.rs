use crate::models::DirectoryStatus;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub calibration: PathBuf,
    pub logs: PathBuf,
    pub samples: PathBuf,
    pub ocr_replay: PathBuf,
    pub captures: PathBuf,
    pub reports: PathBuf,
    pub cache: PathBuf,
}

impl AppPaths {
    pub fn from_app(app: &AppHandle) -> Result<Self, String> {
        let root = app
            .path()
            .app_data_dir()
            .map_err(|error| format!("无法定位应用数据目录: {error}"))?;

        Ok(Self {
            config: root.join("config"),
            calibration: root.join("calibration"),
            logs: root.join("logs"),
            samples: root.join("samples"),
            ocr_replay: root.join("ocr-replay"),
            captures: root.join("captures"),
            reports: root.join("reports"),
            cache: root.join("cache"),
            root,
        })
    }

    pub fn ensure_all(&self) -> Result<(), String> {
        for path in self.all_dirs() {
            fs::create_dir_all(path)
                .map_err(|error| format!("无法创建应用数据子目录 {}: {error}", path.display()))?;
        }
        Ok(())
    }

    pub fn settings_path(&self) -> PathBuf {
        self.config.join("settings.json")
    }

    pub fn app_log_path(&self) -> PathBuf {
        self.logs.join("app.jsonl")
    }

    pub fn status_list(&self) -> Vec<DirectoryStatus> {
        self.named_dirs()
            .into_iter()
            .map(|(key, path)| DirectoryStatus {
                key,
                exists: path.exists(),
                path,
            })
            .collect()
    }

    pub fn all_dirs(&self) -> Vec<&PathBuf> {
        vec![
            &self.root,
            &self.config,
            &self.calibration,
            &self.logs,
            &self.samples,
            &self.ocr_replay,
            &self.captures,
            &self.reports,
            &self.cache,
        ]
    }

    fn named_dirs(&self) -> Vec<(String, PathBuf)> {
        vec![
            ("root".to_string(), self.root.clone()),
            ("config".to_string(), self.config.clone()),
            ("calibration".to_string(), self.calibration.clone()),
            ("logs".to_string(), self.logs.clone()),
            ("samples".to_string(), self.samples.clone()),
            ("ocr-replay".to_string(), self.ocr_replay.clone()),
            ("captures".to_string(), self.captures.clone()),
            ("reports".to_string(), self.reports.clone()),
            ("cache".to_string(), self.cache.clone()),
        ]
    }
}
