//! macOS 自替换：spawn shell helper，等退出后替换 `.app` 再 `open`。

use std::io::Write;
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

fn enclosing_app_bundle(exe: &Path) -> Option<PathBuf> {
    for ancestor in exe.ancestors() {
        if ancestor.extension().and_then(|e| e.to_str()) == Some("app") {
            return Some(ancestor.to_path_buf());
        }
    }
    None
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
    let Some(bundle) = enclosing_app_bundle(&exe) else {
        return;
    };
    let old = bundle.with_extension("app.old");
    if old.exists() {
        let _ = std::fs::remove_dir_all(&old);
    }
}

/// `payload` 为解压出的 `QuickCleaner.app`。
///
/// rename-first + 失败回滚：copy 失败时把 `.old` 挪回原名再 open，保证用户
/// 至少能启动旧版。
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
    let script_path = cache.join("apply-update.sh");
    let log_path = cache.join("apply-update.log");

    let shell_quote = |p: &Path| -> String {
        let s = p.to_string_lossy().replace('\'', "'\\''");
        format!("'{s}'")
    };
    let parent_pid = std::process::id();
    let script = format!(
        r#"#!/bin/bash
set -euo pipefail
trap '' HUP
CUR={cur}
NEW={new}
OLD="${{CUR}}.old"
LOG={log}
log() {{ echo "$(date -Iseconds) $*" >> "$LOG"; }}
log "waiting for pid {pid}"
while kill -0 {pid} 2>/dev/null; do sleep 0.2; done
sleep 0.3
rm -rf "$OLD"
mv "$CUR" "$OLD"
if cp -R "$NEW" "$CUR"; then
  log "replaced app bundle"
  open "$CUR"
  log "relaunched"
else
  log "COPY FAILED, restoring previous bundle"
  rm -rf "$CUR"
  mv "$OLD" "$CUR"
  open "$CUR"
  log "restored and relaunched previous bundle"
  exit 1
fi
"#,
        cur = shell_quote(&current_bundle),
        new = shell_quote(payload),
        log = shell_quote(&log_path),
        pid = parent_pid,
    );
    let mut f = std::fs::File::create(&script_path).map_err(|e| format!("write helper: {e}"))?;
    f.write_all(script.as_bytes())
        .map_err(|e| format!("write helper: {e}"))?;
    drop(f);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755));
    }

    let mut cmd = std::process::Command::new("/bin/bash");
    cmd.arg(&script_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: 子进程里只 setsid，让 helper 离开父进程会话，避免 quit 时
        // SIGHUP 把替换脚本带走（mv 之后、cp 之前死掉会只剩 .app.old）。
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    cmd.spawn()
        .map_err(|e| format!("spawn update helper: {e}"))?;
    Ok(())
}
