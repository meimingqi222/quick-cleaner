//! Windows 自替换：spawn PowerShell helper，等父进程退出后换 exe 再拉起。
//!
//! 运行中的 exe **可以 rename、不能覆盖**。因此采用 rename-first：
//! 任一时刻磁盘上至少有一份完整二进制。

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// 当前进程是否来自发行安装（不在 cargo target 目录里）。
pub fn is_packaged_install() -> bool {
    match std::env::current_exe() {
        Ok(exe) => !crate::core::updater::looks_like_dev_build(&exe),
        Err(_) => false,
    }
}

/// 更新缓存目录：`<user_data>/QuickCleaner/update-cache`。
pub fn update_cache_dir() -> Option<PathBuf> {
    crate::platform::user_data_dir().map(|d| d.join("QuickCleaner").join("update-cache"))
}

/// 用系统默认浏览器打开 http(s) 链接。
///
/// 不能走 `open_in_default_app`：那条路径要求 `path.exists()`，URL 不是文件。
pub fn open_url(url: &str) {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return;
    }
    let wide_url: Vec<u16> = std::ffi::OsStr::new(url)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let wide_open: Vec<u16> = std::ffi::OsStr::new("open")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        winapi::um::shellapi::ShellExecuteW(
            std::ptr::null_mut(),
            wide_open.as_ptr(),
            wide_url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            winapi::um::winuser::SW_SHOWNORMAL,
        );
    }
}

/// 启动时清理上次更新留下的 `.old` 备份。
pub fn cleanup_previous_update_leftovers() {
    if let Ok(exe) = std::env::current_exe() {
        gpui_app_update_installer::remove_exe_backup(&exe);
    }
}

/// `payload` 为解压出的新 `quick-cleaner.exe`。
///
/// 预检留在本产品；通过后由共享安装器写 helper 并 spawn，本调用方应随后退出进程。
/// helper rename-first：任一时刻磁盘上至少有一份完整二进制，copy 失败时把 `.old`
/// 挪回原名。spawn 带 `CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB`，
/// job 不允许 breakaway 时去掉该标志重试（PITFALLS P9）。
pub fn apply_update_and_restart(payload: &Path) -> Result<(), String> {
    if !payload.is_file() {
        return Err(format!("update payload missing: {}", payload.display()));
    }
    let current = std::env::current_exe().map_err(|e| format!("current exe: {e}"))?;
    if current == payload {
        return Err("payload is the running executable".into());
    }

    // 预检：目标目录必须可写，失败在 UI 还活着时就报出来，不要等 helper 退出后。
    let Some(parent) = current.parent() else {
        return Err("current exe has no parent directory".into());
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
    gpui_app_update_installer::spawn_windows_exe_replacement(&current, payload, &cache)
        .map_err(|e| e.to_string())
}
