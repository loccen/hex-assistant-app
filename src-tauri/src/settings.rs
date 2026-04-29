use crate::app_paths::AppPaths;
use crate::models::AppSettings;
use std::fs;

pub fn load_or_create_settings(paths: &AppPaths) -> Result<AppSettings, String> {
    let settings_path = paths.settings_path();

    if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)
            .map_err(|error| format!("无法读取配置文件 {}: {error}", settings_path.display()))?;
        serde_json::from_str(&content)
            .map_err(|error| format!("无法解析配置文件 {}: {error}", settings_path.display()))
    } else {
        let settings = AppSettings::default();
        let content = serde_json::to_string_pretty(&settings)
            .map_err(|error| format!("无法序列化默认配置: {error}"))?;
        fs::write(&settings_path, format!("{content}\n")).map_err(|error| {
            format!("无法写入默认配置文件 {}: {error}", settings_path.display())
        })?;
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn creates_default_settings_file() {
        let paths = temp_paths("settings");
        paths.ensure_all().expect("应能创建测试目录");

        let settings = load_or_create_settings(&paths).expect("应能写入默认配置");

        assert_eq!(settings.language, "zh-CN");
        assert_eq!(settings.capture.default_display_mode, "borderless");
        assert!(paths.settings_path().exists());

        let _ = std::fs::remove_dir_all(paths.root);
    }

    fn temp_paths(label: &str) -> AppPaths {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间应可用")
            .as_micros();
        let root = std::env::temp_dir().join(format!("hex-assistant-{label}-{suffix}"));
        build_paths(root)
    }

    fn build_paths(root: PathBuf) -> AppPaths {
        AppPaths {
            config: root.join("config"),
            calibration: root.join("calibration"),
            logs: root.join("logs"),
            samples: root.join("samples"),
            ocr_replay: root.join("ocr-replay"),
            captures: root.join("captures"),
            reports: root.join("reports"),
            cache: root.join("cache"),
            root,
        }
    }
}
