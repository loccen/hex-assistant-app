use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

/// 统一解析运行时资源根目录。
///
/// 目录优先级：
/// 1. 打包态 `resource_dir/resources`
/// 2. 打包态 `resource_dir`
/// 3. 开发态 `current_dir/src-tauri/resources`
pub fn resource_root(app: &AppHandle) -> PathBuf {
    let resource_dir = app.path().resource_dir().ok();
    resolve_resource_root(
        resource_dir.as_deref(),
        std::env::current_dir().ok().as_deref(),
    )
}

pub fn resolve_resource_root(resource_dir: Option<&Path>, current_dir: Option<&Path>) -> PathBuf {
    let mut candidates = Vec::new();

    if let Some(resource_dir) = resource_dir.filter(|path| path.exists()) {
        candidates.push(resource_dir.join("resources"));
        candidates.push(resource_dir.to_path_buf());
    }

    if let Some(current_dir) = current_dir {
        candidates.push(current_dir.join("src-tauri").join("resources"));
    }

    candidates
        .into_iter()
        .find(|path| has_expected_resources(path))
        .or_else(|| {
            resource_dir
                .filter(|path| path.exists())
                .map(|path| path.join("resources"))
                .filter(|path| path.exists())
        })
        .or_else(|| {
            resource_dir
                .filter(|path| path.exists())
                .map(Path::to_path_buf)
        })
        .or_else(|| {
            current_dir
                .map(|path| path.join("src-tauri").join("resources"))
                .filter(|path| path.exists())
        })
        .unwrap_or_else(|| {
            current_dir
                .map(|path| path.join("src-tauri").join("resources"))
                .unwrap_or_else(|| PathBuf::from(".").join("src-tauri").join("resources"))
        })
}

fn has_expected_resources(root: &Path) -> bool {
    root.join("models").is_dir()
        || root.join("dictionaries").is_dir()
        || root.join("onnxruntime").is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn 优先命中打包态嵌套_resources_目录() {
        let temp = temp_dir("nested");
        let resource_dir = temp.join("bundle-root");
        fs::create_dir_all(resource_dir.join("resources").join("models"))
            .expect("应能创建 resources/models");

        let resolved = resolve_resource_root(Some(&resource_dir), Some(&temp));

        assert_eq!(resolved, resource_dir.join("resources"));
    }

    #[test]
    fn 支持打包态直接平铺资源目录() {
        let temp = temp_dir("flat");
        let resource_dir = temp.join("bundle-root");
        fs::create_dir_all(resource_dir.join("models")).expect("应能创建 models");

        let resolved = resolve_resource_root(Some(&resource_dir), Some(&temp));

        assert_eq!(resolved, resource_dir);
    }

    #[test]
    fn 开发态回落到_src_tauri_resources() {
        let temp = temp_dir("dev");
        let current_dir = temp.join("workspace");
        fs::create_dir_all(
            current_dir
                .join("src-tauri")
                .join("resources")
                .join("models"),
        )
        .expect("应能创建开发态 resources/models");

        let resolved = resolve_resource_root(None, Some(&current_dir));

        assert_eq!(resolved, current_dir.join("src-tauri").join("resources"));
    }

    fn temp_dir(label: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间应晚于 UNIX_EPOCH")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("hex-resource-root-{label}-{suffix}"));
        fs::create_dir_all(&path).expect("应能创建临时目录");
        path
    }
}
