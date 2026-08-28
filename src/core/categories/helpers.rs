//! 扫描目标辅助函数：文件年龄判断、损坏 LaunchAgent 检测、敏感 Apple 缓存识别

use std::path::Path;

/// 路径（目录或文件）的最后修改时间是否已超过 `age`。
///
/// 读不到元数据时返回 `false`——判定依据不足就不默认勾选。
pub(super) fn is_older_than(path: &Path, age: std::time::Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|elapsed| elapsed >= age)
}

/// LaunchAgent 的 plist 是不是已经指向一个不存在的程序。
///
/// 「什么算损坏」是领域判断，留在这里；读 plist 的机制在
/// `platform::macos::plist`——以前这里直接 `Command::new("plutil")`，
/// 是领域层自己调外部进程。
///
/// 整份 plist 只解析一次。解析成功后缺少 Program/ProgramArguments 才能
/// 证明配置本身无可执行入口；文件读不动、语法损坏或 plutil 失败都属于
/// “探测失败”，必须 fail closed，不能据此授权删除。
#[cfg(target_os = "macos")]
pub(super) fn is_broken_launch_agent(plist: &Path) -> bool {
    let Some(value) = crate::platform::macos::plist::read_value(plist) else {
        return false;
    };
    let program = value
        .get("Program")
        .and_then(serde_json::Value::as_str)
        .filter(|program| !program.trim().is_empty())
        .or_else(|| {
            value
                .get("ProgramArguments")
                .and_then(serde_json::Value::as_array)
                .and_then(|arguments| arguments.first())
                .and_then(serde_json::Value::as_str)
                .filter(|program| !program.trim().is_empty())
        });
    let Some(program) = program else {
        return true;
    };

    // 相对命令可能由 launchd 按 PATH 解析，无法仅凭文件系统路径证明损坏。
    let program = Path::new(&program);
    program.is_absolute()
        && std::fs::symlink_metadata(program)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
}

/// 名字叫 Logs 的目录里，已知不只有日志的顶层条目。
///
/// 判据来自实机枚举 `~/Library/Logs`：
/// - `OneDrive/Personal/general.keystore`、`OneDrive/ListSync/Common/general.keystore`
///   —— 密钥库，不是日志
/// - `DiagnosticReports/`、`CrashReporter/` —— 崩溃与诊断报告，用户报 bug 时
///   常常只有这一份副本
/// - `com.apple.*` —— 系统遥测目录，认不出所有者（`CloudTelemetry` 那套在
///   `~/Library/Caches` 里本来就在敏感表上）
///
/// 名字黑名单会腐烂（更新器那张表就是前车之鉴），所以这条只当兜底；判定
/// 「是不是活数据库」走下面的内容探测，不依赖名字。
///
/// 这些不能默认勾选，但仍然整项展示：看见再决定是用户的权利，不是我们的。
#[cfg(target_os = "macos")]
pub(super) fn is_not_just_logs(name: &str) -> bool {
    const NOT_LOGS_EXACT: &[&str] = &["DiagnosticReports", "CrashReporter", "OneDrive"];
    NOT_LOGS_EXACT.contains(&name) || name.starts_with("com.apple.")
}

/// 目录是否正被某个进程当成数据库使用。
///
/// SQLite 的 `-wal`/`-shm` 只在有连接时存在，正常关闭会被删掉；见到就说明
/// 此刻有进程握着它。实机 `~/Library/Logs/OneDrive/` 顶层就有
/// `syncReporterTelemetryCache.otc` 和它的 `-wal`/`-shm`——一个叫 Logs 的
/// 目录里住着活动数据库，只看目录名必然误判。
///
/// 实现挪到了 `core::safety::holds_live_database`——删除路径
/// （`cleaner::clean_path`）现在把同一份判据提升成删除级的硬拒绝，不能只
/// 留在这里当展示层的默认勾选建议，两处必须是同一套逻辑。这里保留一层瘦
/// 包装，调用点（`system.rs`）不用改。
#[cfg(target_os = "macos")]
pub(super) fn holds_live_database(dir: &Path) -> bool {
    crate::core::safety::holds_live_database(dir)
}

/// `~/Library/Caches` 下不应被默认清理的 Apple 系统服务缓存。
///
/// 这些目录涉及认证令牌、iCloud 数据、安全服务、账户信息等，
/// 盲目清理会导致用户被登出、iCloud 同步中断、安全提示弹窗等问题。
/// 它们虽然叫 "Caches"，但重建成本远高于普通应用缓存。
#[cfg(target_os = "macos")]
pub(super) fn is_sensitive_apple_cache(name: &str) -> bool {
    // 精确匹配的敏感目录名
    const SENSITIVE_EXACT: &[&str] = &[
        "CloudKit",
        "com.apple.AuthenticationServicesCore.AuthenticationServicesAgent",
        "com.apple.amsaccountsd",
        "com.apple.amsengagementd",
        "com.apple.appleaccountd",
        "com.apple.securityd",
        "com.apple.identityservicesd",
        "com.apple.protectedcloudstorage.protectedcloudkeysyncing",
        "com.apple.ap.adprivacyd",
        "com.apple.findmy.fmipcore",
        "com.apple.passd",
        "com.apple.ScreenTimeAgent",
        "com.apple.ScreenTimeSettingsAgent",
        "com.apple.icloudwebd",
        "com.apple.iTunesCloud",
        "com.apple.itunescloudd",
        "com.apple.CloudTelemetry",
        "com.apple.iCloudNotificationAgent",
        "com.apple.HomeKit",
        "com.apple.gamed",
    ];

    if SENSITIVE_EXACT.contains(&name) {
        return true;
    }

    // 前缀匹配：以下前缀的目录都涉及敏感系统服务
    const SENSITIVE_PREFIXES: &[&str] = &[
        "com.apple.AuthenticationServices",
        "com.apple.ams",
        "com.apple.appleaccount",
        "com.apple.identity",
        "com.apple.protectedcloud",
        "com.apple.security",
        "com.apple.icloud",
        "com.apple.iCloud",
        "com.apple.Cloud",
        "com.apple.cloud",
        "com.apple.findmy",
        "com.apple.HomeKit",
        "com.apple.homekit",
        "com.apple.ScreenTime",
        "com.apple.screentime",
        "com.apple.passd",
        "com.apple.biome",
    ];

    SENSITIVE_PREFIXES.iter().any(|p| name.starts_with(p))
}
