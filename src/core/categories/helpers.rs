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

/// `~/Library/Caches` 下不应被默认清理的 Apple 系统服务缓存。
///
/// 这些目录涉及认证令牌、iCloud 数据、安全服务、账户信息等，
/// 盲目清理会导致用户被登出、iCloud 同步中断、安全提示弹窗等问题。
/// 它们虽然叫 "Caches"，但重建成本远高于普通应用缓存。
#[cfg(any(target_os = "macos", test))]
pub(super) fn is_sensitive_apple_cache(name: &str) -> bool {
    let snapshot = crate::core::rules::current();
    snapshot.matches_name("macos", "sensitive_cache", name)
}

/// 把文件的 mtime 往前挪 `days` 天。
///
/// std 只能这样改已打开文件的修改时间，改不了目录，所以年龄门只在文件
/// 叶子上验；目录叶子走同一把 `helpers::is_older_than`，逻辑没有分叉。
///
/// 测试夹具，所以整体带 `#[cfg(test)]`，绝不进产物代码。放在 `helpers.rs` 而不是
/// 各调用方自己的测试模块里：吃年龄门的目标不止 `cache.rs` 一处，谁要用谁就
/// 再抄一份，两份实现迟早走样。
#[cfg(test)]
#[cfg(target_os = "macos")]
pub(super) fn backdate(path: &std::path::Path, days: u64) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(days * 86_400))
        .unwrap();
}

#[cfg(test)]
mod tests {
    #[test]
    fn sensitive_name_policies_follow_runtime_configuration_and_preserve_baseline() {
        use crate::core::rules::{current, with_snapshot, RuleSnapshot};
        let snapshot = current();
        let baseline: serde_json::Value = serde_json::from_str(include_str!(
            "../../../rules/fixtures/sensitive-names-baseline.json"
        ))
        .unwrap();
        for key in [
            "sensitive_cache_exact",
            "sensitive_cache_prefixes",
            "sensitive_group_contains",
        ] {
            assert_eq!(
                serde_json::to_value(snapshot.list("macos", key)).unwrap(),
                baseline[key]
            );
        }
        assert!(super::is_sensitive_apple_cache("CloudKit"));
        assert!(super::is_sensitive_apple_cache(
            "com.apple.security.fixture"
        ));
        assert!(!super::is_sensitive_apple_cache("fixture.NewSensitive"));
        let mut bundle = snapshot.bundle.clone();
        bundle
            .rules
            .iter_mut()
            .find(|rule| rule.id == "macos")
            .unwrap()
            .lists
            .get_mut("sensitive_cache_prefixes")
            .unwrap()
            .push("fixture.".into());
        bundle.validate().unwrap();
        with_snapshot(std::sync::Arc::new(RuleSnapshot { bundle }), || {
            assert!(super::is_sensitive_apple_cache("fixture.NewSensitive"));
            assert!(current().matches_name("macos", "sensitive_group", "group.bitwarden.shared"));
            assert!(!current().matches_name("macos", "sensitive_group", "group.harmless"));
        });
        assert!(!super::is_sensitive_apple_cache("fixture.NewSensitive"));
    }
    // 被测的 `is_broken_launch_agent` 只在 macOS 上存在，import 跟着门控。
    #[cfg(target_os = "macos")]
    use super::is_broken_launch_agent;

    #[cfg(target_os = "macos")]
    #[test]
    fn broken_launch_agent_requires_conclusive_evidence() {
        let root = crate::core::testing::fixture("qc_broken_launch_agent_tests");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let write = |name: &str, body: &str| {
            let path = root.join(name);
            std::fs::write(&path, body).unwrap();
            path
        };
        let plist = |entry: &str| {
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>{entry}</dict></plist>"#
            )
        };

        let valid = write(
            "valid.plist",
            &plist("<key>Program</key><string>/bin/launchctl</string>"),
        );
        let missing = write(
            "missing.plist",
            &plist("<key>Program</key><string>/definitely/missing/quick-cleaner</string>"),
        );
        let relative = write(
            "relative.plist",
            &plist("<key>ProgramArguments</key><array><string>tool-on-path</string></array>"),
        );
        let empty = write("empty.plist", &plist(""));
        // 语法根本不合法意味着“探测失败”，不是“确认损坏”；不能因此把
        // 一个可能只是无权读取/临时写到一半的系统 LaunchAgent 放进删除候选。
        let malformed = write("malformed.plist", "<plist><dict><key>Program");

        assert!(!is_broken_launch_agent(&valid));
        assert!(is_broken_launch_agent(&missing));
        assert!(!is_broken_launch_agent(&relative));
        assert!(is_broken_launch_agent(&empty));
        assert!(
            !is_broken_launch_agent(&malformed),
            "语法非法只能判为探测失败，不能授权删除"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
