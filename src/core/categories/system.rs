//! 系统临时、用户临时、日志、崩溃转储、回收站/废纸篓、DNS 缓存

#[cfg(windows)]
use super::target;
use super::ScanTarget;
#[cfg(windows)]
use crate::core::categories::CategoryId;
#[cfg(windows)]
use crate::core::i18n::Text;
#[cfg(windows)]
use crate::core::rules::Operation;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

/// 系统临时文件、用户临时文件、日志、崩溃转储、回收站/废纸篓。
///
/// `home` 为 None 时只加不依赖用户目录的系统目标，用户级路径整段跳过。
pub(super) fn push_system_targets(t: &mut Vec<ScanTarget>, home: Option<&Path>) {
    let _ = home;
    #[cfg(not(windows))]
    let _ = t;
    #[cfg(windows)]
    {
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
                        Operation::Trash,
                        ("engine", "user_trash"),
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
fn push_log_dir_targets(t: &mut Vec<ScanTarget>, logs: &Path) {
    let home = logs
        .parent()
        .and_then(Path::parent)
        .expect("fixture layout");
    crate::core::rules::directories::append_fixture(t, "logs", home, None);
}

#[cfg(test)]
mod tests {
    // `push_log_dir_targets` 走声明式目录布局（`directories::append_fixture`），逻辑
    // 与平台无关，因此测试在 Windows 上也跑（不再只在 macOS 二进制里）。
    use super::push_log_dir_targets;
    use crate::core::categories::CategoryId;

    /// `~/Library/Logs` 按顶层子目录展开，黑名单里那几个不预选。
    ///
    /// 整目录一个目标等于把 N 个互不相干的所有者打包，用户只能全选或全不选；
    /// 实机那里躺着 `OneDrive/…/general.keystore` 和当天的崩溃报告。
    #[test]
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
