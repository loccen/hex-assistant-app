use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use tauri::{AppHandle, Manager};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedResourceRoot {
    pub source_root: PathBuf,
    pub runtime_root: PathBuf,
    pub mirrored_from_network: bool,
    pub cache_hit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RuntimeResourceManifest {
    source_root: String,
    files: Vec<ResourceFileSignature>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ResourceFileSignature {
    relative_path: String,
    size: u64,
    modified_ms: u128,
}

/// 统一解析运行时资源根目录。
///
/// 目录优先级：
/// 1. 可执行文件同级 `resources`
/// 2. 打包态 `resource_dir` 已经是资源目录
/// 3. 打包态 `resource_dir/resources`
/// 4. 开发态 `current_dir/src-tauri/resources`
pub fn resource_root(app: &AppHandle) -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let resource_dir = app.path().resource_dir().ok();
    resolve_resource_root(
        exe_dir.as_deref(),
        resource_dir.as_deref(),
        std::env::current_dir().ok().as_deref(),
    )
}

/// 为 OCR 运行时准备资源目录。
///
/// 当资源根目录位于 UNC/网络路径时，先把 `resources` 镜像到应用数据目录本地缓存，
/// 再从本地缓存加载模型和 ORT 动态库，避免 Windows 包直接从网络路径初始化 ORT 时挂住。
pub fn prepare_runtime_resource_root(
    resource_root: impl AsRef<Path>,
    cache_dir: impl AsRef<Path>,
) -> Result<PreparedResourceRoot, String> {
    prepare_runtime_resource_root_inner(resource_root.as_ref(), cache_dir.as_ref(), None)
}

pub fn is_network_resource_path(path: impl AsRef<Path>) -> bool {
    let raw = path.as_ref().as_os_str().to_string_lossy();
    raw.starts_with(r"\\?\UNC\") || raw.starts_with(r"\\") || raw.starts_with("//")
}

pub fn resolve_resource_root(
    exe_dir: Option<&Path>,
    resource_dir: Option<&Path>,
    current_dir: Option<&Path>,
) -> PathBuf {
    let mut candidates = Vec::new();

    if let Some(exe_dir) = exe_dir.filter(|path| path.exists()) {
        push_candidate(&mut candidates, exe_dir.join("resources"));
    }

    if let Some(resource_dir) = resource_dir.filter(|path| path.exists()) {
        push_candidate(&mut candidates, resource_dir.to_path_buf());
        push_candidate(&mut candidates, resource_dir.join("resources"));
    }

    if let Some(current_dir) = current_dir {
        push_candidate(
            &mut candidates,
            current_dir.join("src-tauri").join("resources"),
        );
    }

    candidates
        .into_iter()
        .find(|path| has_expected_resources(path))
        .or_else(|| {
            exe_dir
                .filter(|path| path.exists())
                .map(|path| path.join("resources"))
                .filter(|path| path.exists())
        })
        .or_else(|| {
            resource_dir
                .filter(|path| path.exists())
                .map(Path::to_path_buf)
                .filter(|path| path.exists())
        })
        .or_else(|| {
            resource_dir
                .filter(|path| path.exists())
                .map(|path| path.join("resources"))
                .filter(|path| path.exists())
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

fn push_candidate(candidates: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !candidates.iter().any(|existing| existing == &candidate) {
        candidates.push(candidate);
    }
}

fn has_expected_resources(root: &Path) -> bool {
    root.join("models").is_dir()
        || root.join("dictionaries").is_dir()
        || root.join("onnxruntime").is_dir()
}

fn prepare_runtime_resource_root_inner(
    resource_root: &Path,
    cache_dir: &Path,
    force_network_path: Option<bool>,
) -> Result<PreparedResourceRoot, String> {
    let should_mirror =
        force_network_path.unwrap_or_else(|| is_network_resource_path(resource_root));
    if !should_mirror {
        return Ok(PreparedResourceRoot {
            source_root: resource_root.to_path_buf(),
            runtime_root: resource_root.to_path_buf(),
            mirrored_from_network: false,
            cache_hit: false,
        });
    }

    let cache_root = cache_dir.join("runtime-resources");
    let mirror_root = cache_root.join("resources");
    let manifest_path = cache_root.join("manifest.json");
    let manifest = RuntimeResourceManifest {
        source_root: resource_root.display().to_string(),
        files: collect_resource_file_signatures(resource_root)?,
    };
    let cache_hit = read_runtime_resource_manifest(&manifest_path)
        .map(|cached| cached == manifest)
        .unwrap_or(false)
        && has_expected_resources(&mirror_root);

    if !cache_hit {
        if cache_root.exists() {
            fs::remove_dir_all(&cache_root).map_err(|error| {
                format!(
                    "无法清理运行时资源缓存目录 {}: {error}",
                    cache_root.display()
                )
            })?;
        }
        fs::create_dir_all(&cache_root).map_err(|error| {
            format!(
                "无法创建运行时资源缓存目录 {}: {error}",
                cache_root.display()
            )
        })?;
        copy_directory_recursive(resource_root, &mirror_root)?;
        write_runtime_resource_manifest(&manifest_path, &manifest)?;
    }

    Ok(PreparedResourceRoot {
        source_root: resource_root.to_path_buf(),
        runtime_root: mirror_root,
        mirrored_from_network: true,
        cache_hit,
    })
}

fn collect_resource_file_signatures(root: &Path) -> Result<Vec<ResourceFileSignature>, String> {
    let mut files = Vec::new();

    for entry in WalkDir::new(root) {
        let entry = entry
            .map_err(|error| format!("扫描运行时资源目录 {} 失败: {error}", root.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }

        let metadata = entry.metadata().map_err(|error| {
            format!(
                "读取运行时资源文件元数据 {} 失败: {error}",
                entry.path().display()
            )
        })?;
        let modified_ms = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        let relative_path = entry
            .path()
            .strip_prefix(root)
            .map_err(|error| {
                format!(
                    "计算运行时资源相对路径失败 root={} path={}: {error}",
                    root.display(),
                    entry.path().display()
                )
            })?
            .to_string_lossy()
            .replace('\\', "/");
        files.push(ResourceFileSignature {
            relative_path,
            size: metadata.len(),
            modified_ms,
        });
    }

    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(files)
}

fn read_runtime_resource_manifest(path: &Path) -> Option<RuntimeResourceManifest> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_runtime_resource_manifest(
    path: &Path,
    manifest: &RuntimeResourceManifest,
) -> Result<(), String> {
    let content = serde_json::to_string_pretty(manifest)
        .map_err(|error| format!("无法序列化运行时资源缓存 manifest: {error}"))?;
    fs::write(path, format!("{content}\n")).map_err(|error| {
        format!(
            "无法写入运行时资源缓存 manifest {}: {error}",
            path.display()
        )
    })
}

fn copy_directory_recursive(source_root: &Path, target_root: &Path) -> Result<(), String> {
    for entry in WalkDir::new(source_root) {
        let entry = entry.map_err(|error| {
            format!("遍历运行时资源目录 {} 失败: {error}", source_root.display())
        })?;
        let relative = entry.path().strip_prefix(source_root).map_err(|error| {
            format!(
                "计算运行时资源复制相对路径失败 root={} path={}: {error}",
                source_root.display(),
                entry.path().display()
            )
        })?;
        let target_path = target_root.join(relative);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&target_path).map_err(|error| {
                format!("无法创建运行时资源目录 {}: {error}", target_path.display())
            })?;
            continue;
        }

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!("无法创建运行时资源父目录 {}: {error}", parent.display())
            })?;
        }
        fs::copy(entry.path(), &target_path).map_err(|error| {
            format!(
                "无法复制运行时资源文件 {} -> {}: {error}",
                entry.path().display(),
                target_path.display()
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn 优先命中_exe_同级_resources_目录() {
        let temp = temp_dir("exe-adjacent");
        let exe_dir = temp.join("hex-assistant-release");
        let resource_dir = temp.join("wsl-redirected-root");
        fs::create_dir_all(exe_dir.join("resources").join("models"))
            .expect("应能创建 exe 同级 resources/models");
        fs::create_dir_all(resource_dir.join("models")).expect("应能创建误导性的根目录 models");

        let resolved = resolve_resource_root(Some(&exe_dir), Some(&resource_dir), Some(&temp));

        assert_eq!(resolved, exe_dir.join("resources"));
    }

    #[test]
    fn 支持_resource_dir_已经是_resources_目录() {
        let temp = temp_dir("resource-dir-self");
        let resource_dir = temp.join("bundle-root").join("resources");
        fs::create_dir_all(resource_dir.join("models")).expect("应能创建 models");

        let resolved = resolve_resource_root(None, Some(&resource_dir), Some(&temp));

        assert_eq!(resolved, resource_dir);
    }

    #[test]
    fn 支持_resource_dir_为应用根目录() {
        let temp = temp_dir("resource-dir-root");
        let resource_dir = temp.join("bundle-root");
        fs::create_dir_all(resource_dir.join("resources").join("models"))
            .expect("应能创建 resources/models");

        let resolved = resolve_resource_root(None, Some(&resource_dir), Some(&temp));

        assert_eq!(resolved, resource_dir.join("resources"));
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

        let resolved = resolve_resource_root(None, None, Some(&current_dir));

        assert_eq!(resolved, current_dir.join("src-tauri").join("resources"));
    }

    #[test]
    fn 识别_unc_路径为网络资源目录() {
        assert!(is_network_resource_path(PathBuf::from(
            r"\\wsl$\Ubuntu\home\code\hex-assistant-app\src-tauri\resources"
        )));
        assert!(is_network_resource_path(PathBuf::from(
            r"\\?\UNC\wsl$\Ubuntu\home\code\hex-assistant-app\src-tauri\resources"
        )));
        assert!(!is_network_resource_path(PathBuf::from(
            r"C:\Users\loccen\AppData\Local\Programs\hex-assistant\resources"
        )));
    }

    #[test]
    fn 本地路径不触发资源镜像() {
        let temp = temp_dir("runtime-local");
        let source_root = temp.join("resources");
        fs::create_dir_all(source_root.join("models")).expect("应能创建 models");
        fs::write(source_root.join("models").join("model.onnx"), b"model")
            .expect("应能写入模型文件");

        let prepared =
            prepare_runtime_resource_root_inner(&source_root, &temp.join("cache"), Some(false))
                .expect("本地路径不应镜像失败");

        assert_eq!(prepared.runtime_root, source_root);
        assert!(!prepared.mirrored_from_network);
        assert!(!prepared.cache_hit);
    }

    #[test]
    fn 网络路径触发镜像并支持缓存命中() {
        let temp = temp_dir("runtime-network");
        let source_root = temp.join("release-resources");
        fs::create_dir_all(source_root.join("models")).expect("应能创建 models");
        fs::create_dir_all(source_root.join("dictionaries")).expect("应能创建 dictionaries");
        fs::write(
            source_root.join("models").join("ppocrv4_rec.onnx"),
            b"fake-model",
        )
        .expect("应能写入模型文件");
        fs::write(
            source_root.join("dictionaries").join("augments.zh-CN.json"),
            b"{}",
        )
        .expect("应能写入词库文件");

        let cache_dir = temp.join("cache");
        let first = prepare_runtime_resource_root_inner(&source_root, &cache_dir, Some(true))
            .expect("首次镜像应成功");
        assert!(first.mirrored_from_network);
        assert!(!first.cache_hit);
        assert!(first
            .runtime_root
            .join("models")
            .join("ppocrv4_rec.onnx")
            .is_file());

        let second = prepare_runtime_resource_root_inner(&source_root, &cache_dir, Some(true))
            .expect("二次准备应命中缓存");
        assert!(second.mirrored_from_network);
        assert!(second.cache_hit);
        assert_eq!(first.runtime_root, second.runtime_root);
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
