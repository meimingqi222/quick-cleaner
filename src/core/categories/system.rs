//! 系统临时、用户临时、日志、崩溃转储、回收站/废纸篓、DNS 缓存

#[cfg(target_os = "macos")]
use super::target_with_recommendation;
use super::{target, ScanTarget};
use crate::core::categories::CategoryId;
use crate::core::i18n::Text;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

/// 系统临时文件、用户临时文件、日志、崩溃转储、回收站/废纸篓。
///
/// `home` 为 None 时只加不依赖用户目录的系统目标，用户级路径整段跳过。
pub(super) fn push_system_targets(t: &mut Vec<ScanTarget>, home: Option<&Path>) {
    #[cfg(windows)]
    let _ = home;
    #[cfg(windows)]
    {
        let windows =
            PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()));

        // 系统临时：与用户 Home 无关，主目录不可确认时也必须留下。
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
        t.push(target(
            windows.join("Logs"),
            "Windows\\Logs",
            CategoryId::Logs,
        ));

        // 用户临时（精确锚定真实前台用户；主目录不可信时跳过，
        // 绝不能清到别的账户头上）
        if let Some(user_temp) = crate::platform::user_temp_dir() {
            t.push(target(user_temp, "%TEMP%", CategoryId::UserTemp));
        }
        if let Some(local) = crate::platform::user_cache_dir() {
            t.push(target(
                local.join("CrashDumps"),
                Text::new("CrashDumps 崩溃转储", "CrashDumps"),
                CategoryId::Logs,
            ));
            t.push(target(
                local.join("D3DSCache"),
                Text::new("D3D 着色器缓存", "D3D shader cache"),
                CategoryId::Logs,
            ));
        }

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
        if let Some(home) = home {
            let logs = home.join("Library/Logs");

            // 系统与用户临时/缓存
            // 不清空 /private/tmp 和 /private/var/tmp：其中可能有仍在运行的服务
            // 使用的 socket、锁文件或安装事务。若以后恢复，必须按文件年龄和
            // 占用状态逐项筛选，不能把整棵目录作为一个目标。

            // 日志：按顶层子目录展开，不整目录一个目标
            push_log_dir_targets(t, &logs);

            // 废纸篓
            t.push(target(
                home.join(".Trash"),
                Text::new("废纸篓", "Trash"),
                CategoryId::RecycleBin,
            ));

            // 应用窗口状态。恢复的是「上次哪些窗口开着」，删了丢的是用户自己
            // 没保存的工作现场，不满足「最坏情况止于重新生成」，所以不默认勾选
            // （`CategoryId::default_selected` 第 2 条）。
            t.push(target(
                home.join("Library/Saved Application State"),
                Text::new("应用窗口状态", "Saved Application State"),
                CategoryId::UserTemp,
            ));
            // ~/Library/HTTPStorages 不加入默认清理：里面含 .binarycookies
            // 登录会话文件（Telegram、OneDrive、各种应用），删了等于把用户
            // 从一堆应用里登出。如果将来要加，必须放到默认不勾选的分类。
        }

        // DNS 缓存目录。同上，不默认勾选：这些目录由 mDNSResponder 持有，
        // 删文件并不等于 `dscacheutil -flushcache` 的语义（缓存可能还在内存
        // 里），而正在解析时动它属于「在事务中间」——既没换来用户想要的效果，
        // 又踩了规范第 3 条。路径来自 $TMPDIR，不依赖 home。
        push_dns_cache_targets(t);
    }
}

/// `~/Library/Logs` 按顶层子目录展开。
///
/// 一个目标覆盖整目录 = 覆盖 N 个互不相干的所有者，用户只能全选或全不选，
/// 而规范要求的恰恰是「每个目标各自说得清最坏情况」。实机枚举结果：
/// `OneDrive/Personal/general.keystore`（密钥库）、`DiagnosticReports/SFA-*.diag`
/// （当天刚生成的崩溃报告，往往是唯一副本）、`com.apple.CloudTelemetry/`。
///
/// 展开后纯日志的目录照旧默认勾选，两类例外仍然整项展示但不预选：进
/// `helpers::is_not_just_logs` 黑名单的，以及顶层躺着活动数据库的。
#[cfg(target_os = "macos")]
pub(super) fn push_log_dir_targets(t: &mut Vec<ScanTarget>, logs: &Path) {
    let Ok(rd) = std::fs::read_dir(logs) else {
        // 读不到就不加目标。退回「整目录一项」正好是最需要小心的那种兜底。
        return;
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // 目录按所有者划分，可以认为里面就是那个应用的日志；散落的单个文件
        // 只能靠扩展名认。不区分的话，用户丢进来的文件会被当成日志永久删掉。
        let is_dir = entry.file_type().is_ok_and(|ft| ft.is_dir());
        let looks_like_log = is_dir
            || entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("log"));
        // 名字黑名单会腐烂（更新器那张表就是前车之鉴），所以再补一道内容探测：
        // 顶层有 SQLite 事务侧文件 = 这目录正被某个进程当数据库用，里面的
        // `.log` 只是邻居。实机 `Logs/OneDrive/` 顶层就有这种文件。
        let live_database = is_dir && super::helpers::holds_live_database(&entry.path());
        let recommended =
            looks_like_log && !live_database && !super::helpers::is_not_just_logs(&name);
        t.push(target_with_recommendation(
            entry.path(),
            format!("~/Library/Logs/{name}"),
            CategoryId::Logs,
            recommended,
        ));
    }
}

/// DNS 缓存目录。
///
/// macOS 的 DNS 缓存散落在 per-user 的 `$TMPDIR` 下的 `com.apple.dns`
/// 及相关目录中。列出来供用户自己勾选，但不默认勾选：目录由 mDNSResponder
/// 持有，删文件不等于 `dscacheutil -flushcache`，收益不确定而风险确定。
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

#[cfg(test)]
mod tests {
    // 本模块目前只有 macOS 专属测试，被测的 `push_log_dir_targets` 本身就带平台门。
    #[cfg(target_os = "macos")]
    use super::push_log_dir_targets;
    #[cfg(target_os = "macos")]
    use crate::core::categories::CategoryId;

    /// `~/Library/Logs` 按顶层子目录展开，黑名单里那几个不预选。
    ///
    /// 整目录一个目标等于把 N 个互不相干的所有者打包，用户只能全选或全不选；
    /// 实机那里躺着 `OneDrive/…/general.keystore` 和当天的崩溃报告。
    #[test]
    #[cfg(target_os = "macos")]
    fn logs_are_split_by_owner_and_hazards_stay_unpreselected() {
        let root = crate::core::testing::fixture("qc_logs");
        let logs = root.join("Library/Logs");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(logs.join("Notion")).unwrap();
        std::fs::create_dir_all(logs.join("DiagnosticReports")).unwrap();
        std::fs::create_dir_all(logs.join("com.apple.CloudTelemetry")).unwrap();
        std::fs::create_dir_all(logs.join("OneDrive/Personal")).unwrap();
        std::fs::write(logs.join("OneDrive/Personal/general.keystore"), b"k").unwrap();
        std::fs::write(logs.join("warp.log"), b"log").unwrap();
        // 实机 `~/Library/Logs` 顶层真有这些：SQLite 的 telemetry 缓存与它的
        // 事务侧文件，名字在 Logs 里但不是日志。
        std::fs::write(logs.join("telemetryCache.otc"), b"sqlite").unwrap();
        std::fs::write(logs.join("telemetryCache.otc-wal"), b"w").unwrap();
        // 名字不在黑名单里，但内容说明它正被某个进程当数据库用
        std::fs::create_dir_all(logs.join("Telemetry")).unwrap();
        std::fs::write(logs.join("Telemetry/state.otc"), b"sqlite").unwrap();
        std::fs::write(logs.join("Telemetry/state.otc-wal"), b"w").unwrap();

        let mut targets = Vec::new();
        push_log_dir_targets(&mut targets, &logs);
        let entry = |rel: &str| targets.iter().find(|t| t.path == logs.join(rel));

        assert_eq!(entry("Notion").map(|t| t.recommended), Some(true));
        assert_eq!(
            entry("warp.log").map(|t| t.recommended),
            Some(true),
            "顶层散落的单个日志也是目标，不能因为拆分反而漏掉"
        );
        for hazard in ["DiagnosticReports", "OneDrive", "com.apple.CloudTelemetry"] {
            let target = entry(hazard).unwrap_or_else(|| panic!("{hazard} 仍然要展示"));
            assert_eq!(
                target.category,
                CategoryId::Logs,
                "{hazard} 该留在日志类目里"
            );
            assert!(!target.recommended, "{hazard} 不只有日志，不能预选");
        }
        for stray in ["telemetryCache.otc", "telemetryCache.otc-wal"] {
            let target = entry(stray).expect("散落的非日志文件仍要展示");
            assert!(
                !target.recommended,
                "{stray} 不是日志，不能因为住在 Logs 里就被默认删掉"
            );
        }
        // 名字表之外的第二道关口：按内容判定
        let telemetry = entry("Telemetry").expect("内容探测不该把目录从表里抹掉");
        assert!(
            !telemetry.recommended,
            "顶层有 SQLite 事务侧文件的目录正被进程使用，不能预选"
        );
        assert!(
            targets.iter().all(|t| t.path != logs),
            "整目录一个目标会让用户无法分别决定"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
