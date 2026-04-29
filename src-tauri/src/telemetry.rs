use crate::app_paths::AppPaths;
use crate::models::{TelemetryEvent, TelemetryEventInput};
use chrono::Utc;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn new_trace_id(prefix: &str) -> String {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default();
    format!("{prefix}-{micros}")
}

pub fn write_event(paths: &AppPaths, input: TelemetryEventInput) -> Result<TelemetryEvent, String> {
    let event = TelemetryEvent {
        timestamp: Utc::now().to_rfc3339(),
        trace_id: new_trace_id(&input.stage),
        stage: input.stage,
        input_summary: input.input_summary,
        output_summary: input.output_summary,
        duration_ms: input.duration_ms,
        level: input.level,
        error_code: input.error_code,
        message: input.message,
    };

    append_event(paths, &event)?;
    Ok(event)
}

pub fn append_event(paths: &AppPaths, event: &TelemetryEvent) -> Result<(), String> {
    let line =
        serde_json::to_string(event).map_err(|error| format!("无法序列化结构化日志: {error}"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.app_log_path())
        .map_err(|error| format!("无法打开应用日志文件: {error}"))?;
    writeln!(file, "{line}").map_err(|error| format!("无法写入应用日志文件: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_paths::AppPaths;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn writes_structured_log_line() {
        let paths = temp_paths("telemetry");
        paths.ensure_all().expect("应能创建测试目录");

        let event = write_event(
            &paths,
            TelemetryEventInput {
                stage: "test-stage".to_string(),
                input_summary: "输入摘要".to_string(),
                output_summary: "输出摘要".to_string(),
                duration_ms: 12,
                level: "info".to_string(),
                error_code: None,
                message: "测试日志".to_string(),
            },
        )
        .expect("应能写入结构化日志");

        let content = std::fs::read_to_string(paths.app_log_path()).expect("应能读取日志");
        assert!(content.contains(&event.trace_id));
        assert!(content.contains("test-stage"));

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
