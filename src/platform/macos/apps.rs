//! macOS 已安装应用程序 (.app Bundle) 枚举与卸载支持骨架

use crate::core::apps::{AppRegRoot, InstalledApp};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// 枚举 macOS 系统中已安装的 .app 应用程序包
pub fn list_installed_apps(_live: &AtomicBool) -> Vec<InstalledApp> {
    let mut apps = Vec::new();
    let app_dirs = [
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];

    for root in &app_dirs {
        if let Ok(rd) = std::fs::read_dir(root) {
            for entry in rd.flatten() {
                let path = entry.path();
                if path.is_dir() && path.extension().map_or(false, |ext| ext == "app") {
                    let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                    let size = dir_size(&path);
                    apps.push(InstalledApp {
                        id: name.clone(),
                        name: name.clone(),
                        version: String::new(),
                        publisher: String::from("macOS Application"),
                        last_used_date: None,
                        last_used_raw: 0,
                        install_date: None,
                        install_date_raw: 0,
                        install_location: Some(path.clone()),
                        display_icon: None,
                        uninstall_string: None,
                        quiet_uninstall_string: None,
                        estimated_size: size,
                        registry_root: AppRegRoot::SystemApp,
                        registry_subpath: String::new(),
                        is_system_component: false,
                        uninstaller_missing: false,
                    });
                }
            }
        }
    }
    apps
}

/// 在访达中定位指定路径
pub fn reveal_in_explorer(path: &Path) {
    if !path.exists() {
        return;
    }
    let _ = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn();
}

fn dir_size(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

pub fn run_uninstaller_and_wait(app: &InstalledApp) -> Result<(), String> {
    if let Some(loc) = &app.install_location {
        if loc.exists() {
            // macOS 默认卸载为将 .app 移入废纸篓
            let trash = dirs::home_dir().map(|h| h.join(".Trash")).unwrap_or_else(|| PathBuf::from("/tmp"));
            let dest = trash.join(loc.file_name().unwrap_or_default());
            std::fs::rename(loc, dest).map_err(|e| format!("移入废纸篓失败: {e}"))?;
            return Ok(());
        }
    }
    Err("未找到应用程序路径".into())
}
