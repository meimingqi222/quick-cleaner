//! Windows 自替换：spawn PowerShell helper，等父进程退出后换 exe 再拉起。
//!
//! 运行中的 exe **可以 rename、不能覆盖**。因此采用 rename-first：
//! 任一时刻磁盘上至少有一份完整二进制。

use std::io::Write;
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
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let old = {
        let mut name = exe.as_os_str().to_owned();
        name.push(".old");
        PathBuf::from(name)
    };
    if old.exists() {
        let _ = std::fs::remove_file(&old);
    }
}

/// `payload` 为解压出的新 `quick-cleaner.exe`。
///
/// 写 helper 脚本 → spawn → 本调用方应随后退出进程。
///
/// rename-first：任一时刻磁盘上至少有一份完整二进制。copy 失败时 helper
/// 必须把 `.old` 挪回原名，否则用户只剩损坏路径、旧进程又已退出。
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
    let script_path = cache.join("apply-update.ps1");
    let log_path = cache.join("apply-update.log");

    // 路径可能含空格；全部单引号包裹，内部单引号翻倍。
    let q = |p: &Path| -> String {
        let s = p.to_string_lossy().replace('\'', "''");
        format!("'{s}'")
    };
    let parent_pid = std::process::id();
    let script = format!(
        r#"$ErrorActionPreference = 'Stop'
$exe = {exe}
$new = {new}
$old = "$exe.old"
$log = {log}
function Log($m) {{ Add-Content -Path $log -Value ("$(Get-Date -Format o) " + $m) }}
try {{
  Log "waiting for pid {pid}"
  Wait-Process -Id {pid} -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 400
  if (Test-Path -LiteralPath $old) {{ Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue }}
  Move-Item -LiteralPath $exe -Destination $old -Force
  try {{
    Copy-Item -LiteralPath $new -Destination $exe -Force
    Log "copied new binary"
    Start-Process -FilePath $exe
    Log "relaunched"
  }} catch {{
    Log ("COPY FAILED, restoring: " + $_.Exception.Message)
    if (Test-Path -LiteralPath $exe) {{ Remove-Item -LiteralPath $exe -Force -ErrorAction SilentlyContinue }}
    if (Test-Path -LiteralPath $old) {{
      Move-Item -LiteralPath $old -Destination $exe -Force
      Start-Process -FilePath $exe
      Log "restored previous binary and relaunched"
    }} else {{
      Log "CRITICAL: no .old to restore"
    }}
    exit 1
  }}
}} catch {{
  Log ("FAILED: " + $_.Exception.Message)
  exit 1
}}
"#,
        exe = q(&current),
        new = q(payload),
        log = q(&log_path),
        pid = parent_pid,
    );
    let mut f = std::fs::File::create(&script_path).map_err(|e| format!("write helper: {e}"))?;
    f.write_all(script.as_bytes())
        .map_err(|e| format!("write helper: {e}"))?;
    drop(f);

    let mut cmd = std::process::Command::new("powershell");
    cmd.arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(&script_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
        if cmd.spawn().is_ok() {
            return Ok(());
        }
        // 父进程 job 不允许 breakaway 时 CreateProcess 直接失败，去掉该标志再试。
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
        cmd.spawn()
            .map_err(|e| format!("spawn update helper: {e}"))?;
    }
    Ok(())
}
