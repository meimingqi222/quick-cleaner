//! 系统临时、用户临时、日志、崩溃转储、回收站/废纸篓、DNS 缓存

use super::{target, ScanTarget};
use crate::core::categories::CategoryId;
use crate::core::i18n::Text;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

/// 系统临时文件、用户临时文件、日志、崩溃转储、回收站/废纸篓
pub(super) fn push_system_targets(t: &mut Vec<ScanTarget>, home: &Path) {
    #[cfg(windows)]
    let _ = home;
    #[cfg(windows)]
    {
        let local = crate::platform::windows::real_user_local_appdata();
        let windows =
            PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()));

        // 系统临时
        t.push(target(
            windows.join("Temp"),
            "Windows\\Temp",
            CategoryId::SystemTemp,
        ));
        t.push(target(
            windows.join("SoftwareDistribution\\Download"),
            Text::new("Windows 更新缓存", "Windows Update cache"),
            CategoryId::SystemTemp,
        ));
        t.push(target(
            windows.join("SystemTemp"),
            "SystemTemp",
            CategoryId::SystemTemp,
        ));
        t.push(target(
            PathBuf::from("C:\\tmp"),
            "C:\\tmp",
            CategoryId::SystemTemp,
        ));

        // 用户临时（精确锚定真实前台用户）
        t.push(target(
            crate::platform::windows::real_user_temp(),
            "%TEMP%",
            CategoryId::UserTemp,
        ));
        t.push(target(
            local.join("CrashDumps"),
            Text::new("CrashDumps 崩溃转储", "CrashDumps"),
            CategoryId::Logs,
        ));

        // 日志
        t.push(target(
            windows.join("Logs"),
            "Windows\\Logs",
            CategoryId::Logs,
        ));
        t.push(target(
            local.join("D3DSCache"),
            Text::new("D3D 着色器缓存", "D3D shader cache"),
            CategoryId::Logs,
        ));

        // 回收站（只统计真实前台用户自己的 SID 子目录）
        if let Some(sid) = crate::platform::windows::real_user_sid() {
            for letter in 'A'..='Z' {
                let rb = PathBuf::from(format!("{letter}:\\$Recycle.Bin")).join(&sid);
                if rb.exists() {
                    t.push(target(
                        rb,
                        Text::new(
                            format!("{letter}: 回收站"),
                            format!("{letter}: Recycle Bin"),
                        ),
                        CategoryId::RecycleBin,
                    ));
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let logs = home.join("Library/Logs");

        // 系统与用户临时/缓存
        // 不清空 /private/tmp 和 /private/var/tmp：其中可能有仍在运行的服务
        // 使用的 socket、锁文件或安装事务。若以后恢复，必须按文件年龄和
        // 占用状态逐项筛选，不能把整棵目录作为一个目标。

        // 日志
        t.push(target(logs, "~/Library/Logs", CategoryId::Logs));

        // 废纸篓
        t.push(target(
            home.join(".Trash"),
            Text::new("废纸篓", "Trash"),
            CategoryId::RecycleBin,
        ));

        // 应用窗口状态：只存窗口位置/大小，删了无影响
        t.push(target(
            home.join("Library/Saved Application State"),
            Text::new("应用窗口状态", "Saved Application State"),
            CategoryId::UserTemp,
        ));
        // ~/Library/HTTPStorages 不加入默认清理：里面含 .binarycookies
        // 登录会话文件（Telegram、OneDrive、各种应用），删了等于把用户
        // 从一堆应用里登出。如果将来要加，必须放到默认不勾选的分类。

        // DNS 缓存目录（可安全清理）
        push_dns_cache_targets(t);
    }
}

/// DNS 缓存目录。
///
/// macOS 的 DNS 缓存散落在 per-user 的 `$TMPDIR` 下的 `com.apple.dns`
/// 及相关目录中，可安全清理，系统会自动重建。
#[cfg(target_os = "macos")]
pub(super) fn push_dns_cache_targets(t: &mut Vec<ScanTarget>) {
    let tmpdir = std::env::temp_dir();
    let Some(user_cache_root) = tmpdir.parent() else {
        return;
    };
    for root in [user_cache_root.join("C"), user_cache_root.join("T")] {
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if (name.starts_with("com.apple.dns") || name.starts_with("com.apple.networkd"))
                && entry.file_type().is_ok_and(|ft| ft.is_dir())
            {
                t.push(target(
                    entry.path(),
                    Text::new(format!("DNS 缓存 · {name}"), format!("DNS Cache · {name}")),
                    CategoryId::SystemTemp,
                ));
            }
        }
    }
}
