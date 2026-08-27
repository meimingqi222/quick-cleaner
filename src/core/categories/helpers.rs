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
/// 另外去掉了原来先跑一次 `plutil -lint` 的预检：`-extract` 对语法非法的
/// plist 本来就会失败，两条路都归到「读不出 Program」这个分支，结果完全
/// 一样，但每个 plist 少 fork 一次进程。
#[cfg(target_os = "macos")]
pub(super) fn is_broken_launch_agent(plist: &Path) -> bool {
    use crate::platform::macos::plist::read_scalar;

    let Some(program) =
        read_scalar(plist, "Program").or_else(|| read_scalar(plist, "ProgramArguments.0"))
    else {
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
/// 只看顶层一层：这些伴随文件和库文件本身永远同目录，再深没有意义。
#[cfg(target_os = "macos")]
pub(super) fn holds_live_database(dir: &Path) -> bool {
    const MARKERS: &[&str] = &["-wal", "-shm", ".otc", ".sqlite", ".sqlite3", ".db"];
    let Ok(rd) = std::fs::read_dir(dir) else {
        // 读不动就等于看不清，按「不满足第 1 条」处理。
        return true;
    };
    rd.flatten().any(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        MARKERS.iter().any(|marker| name.ends_with(marker))
    })
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
