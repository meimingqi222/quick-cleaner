//! macOS 自替换：预检后交给共享安装器的 shell helper，等退出后替换 `.app` 再 `open`。

use gpui_app_update_installer::enclosing_app_bundle;
use std::path::{Path, PathBuf};

pub fn is_packaged_install() -> bool {
    match std::env::current_exe() {
        Ok(exe) => {
            // 只有装在 .app bundle 里才算发行安装；cargo run 直接是裸二进制。
            enclosing_app_bundle(&exe).is_some()
                && !crate::core::updater::looks_like_dev_build(&exe)
        }
        Err(_) => false,
    }
}

pub fn update_cache_dir() -> Option<PathBuf> {
    crate::platform::user_data_dir().map(|d| d.join("QuickCleaner").join("update-cache"))
}

/// 用系统默认浏览器打开 http(s) 链接。
pub fn open_url(url: &str) {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return;
    }
    let _ = std::process::Command::new("open").arg(url).spawn();
}

pub fn cleanup_previous_update_leftovers() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    if let Some(bundle) = enclosing_app_bundle(&exe) {
        gpui_app_update_installer::remove_bundle_backup(&bundle);
    }
}

/// `payload` 为解压出的 `QuickCleaner.app`。
///
/// 预检留在本产品；共享安装器的 helper rename-first + 失败回滚：copy 失败时把
/// `.old` 挪回原名再 open，保证用户至少能启动旧版。helper 以 `setsid` +
/// `trap '' HUP` 脱离父进程会话（PITFALLS P9）。
pub fn apply_update_and_restart(payload: &Path) -> Result<(), String> {
    if !payload.is_dir() {
        return Err(format!("update payload missing: {}", payload.display()));
    }
    let exe = std::env::current_exe().map_err(|e| format!("current exe: {e}"))?;
    let current_bundle =
        enclosing_app_bundle(&exe).ok_or_else(|| "not running from a .app bundle".to_string())?;

    let Some(parent) = current_bundle.parent() else {
        return Err("app bundle has no parent directory".into());
    };
    let probe = parent.join(format!(".qc-update-write-probe-{}", std::process::id()));
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
        }
        Err(e) => {
            return Err(format!(
                "install directory is not writable ({}): {e}",
                parent.display()
            ));
        }
    }

    let cache = update_cache_dir().ok_or_else(|| "user data dir unavailable".to_string())?;
    std::fs::create_dir_all(&cache).map_err(|e| format!("create cache: {e}"))?;
    gpui_app_update_installer::spawn_macos_bundle_replacement(&current_bundle, payload, &cache)
        .map_err(|e| e.to_string())
}
