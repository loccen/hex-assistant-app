use hex_assistant_app_lib::diagnostics::{build_release_package, release_extract_dir_for_zip};

fn main() {
    let workspace_root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let trace_id = format!(
        "release-cli-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    );

    match build_release_package(&workspace_root, &trace_id) {
        Ok(result) => {
            println!("release 压缩包已生成: {}", result.zip_path.display());
            println!(
                "release 解压目录已覆盖: {}",
                release_extract_dir_for_zip(&result.zip_path).display()
            );
            println!("trace_id: {}", result.trace_id);
            println!("文件数: {}", result.included_files);
        }
        Err(error) => {
            eprintln!("release 压缩包生成失败: {error}");
            std::process::exit(1);
        }
    }
}
