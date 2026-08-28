//! Homebrew 的 owner command 清理：`brew cleanup`。
//!
//! 动机与「只删 `~/Library/Caches/Homebrew` 目录」的区别：**命令自己知道
//! 怎么安全收缩**——它不只清下载缓存，还会清旧版本 keg、断链的
//! Caskroom 残余，并且知道哪些东西不能动；裸删缓存目录只能碰到第一项。
//! 这也是 Mole 的做法（clean 流程末尾跑 `brew cleanup`，7 天内清过则
//! 跳过）。
//!
//! # 集成方式
//!
//! 与 Docker 镜像同构的虚拟路径（`brew://cleanup`，见 `core::model`）：
//! 扫描期 `brew cleanup -n`（dry-run）拿体积估算进列表；删除期
//! `cleaner` 路由到 [`run_cleanup`] 跑真命令。体积永远只认 brew 自己
//! dry-run 输出的 `approximately X` 摘要行——逐行提取路径再 `stat`
//! 求和是备选，但输出文案随 brew 版本漂移，摘要行的稳定性反而更好；
//! 两者都拿不到就标 0（体积为估算值，不是删除依据，不影响安全性）。
//!
//! # 节流
//!
//! `brew cleanup` 不是瞬时的命令，每次扫描都跑一次 dry-run 没有道理。
//! `Settings.brew_cleanup_at` 记录上一次真实清理时间，间隔不到
//! [`THROTTLE_DAYS`] 天就不出现该条目。记时间在真实清理成功后，
//! dry-run 失败不记。

use std::path::{Path, PathBuf};
use std::time::Duration;

/// 两次 `brew cleanup` 之间的最小间隔（天）。
pub const THROTTLE_DAYS: i64 = 7;

/// dry-run 最多等多久。`brew cleanup -n` 正常在秒级，brew 索引大或
/// 网络卷挂载的 Cellar 可能拖到十秒；超时就当这轮没有可清内容。
const PREVIEW_TIMEOUT: Duration = Duration::from_secs(15);

/// 真清理最多等多久。实际删文件比 dry-run 慢一个量级，给一分钟。
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(60);

/// 这台机器上装了 brew 吗。
///
/// 只认两个标准安装前缀（Apple Silicon 与 Intel），`which brew` 会顺着
/// PATH 找到用户自己 wrapper 的同名脚本，那不一定是 Homebrew。
fn brew_exe() -> Option<&'static str> {
    ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
        .into_iter()
        .find(|path| std::fs::symlink_metadata(path).is_ok())
}

/// 距上次真实清理是否不足 [`THROTTLE_DAYS`] 天（或从未清过——返回 false，
/// 即**应该**出现条目）。
///
/// `None`（从没清过）和「记录损坏读不出」都按「该清」处理：节流是体验
/// 优化不是安全闸，宁可多提示一次。
pub fn should_offer(settings_cleanup_at: Option<i64>) -> bool {
    let Some(at) = settings_cleanup_at else {
        return true;
    };
    let now = chrono::Local::now().timestamp();
    now.saturating_sub(at) >= THROTTLE_DAYS * 24 * 3600
}

/// dry-run 预览：`brew cleanup -n` 的体积估算与将删除的条目数。
///
/// 返回 `None` 表示这轮不给条目：brew 不存在、命令失败/超时、或者
/// 输出里没有任何将删除的内容。三种情况在界面上都该是「没有这一条」，
/// 不是「有一条 0 B 的假条目」。
pub fn cleanup_preview() -> Option<(u64, u64)> {
    let exe = brew_exe()?;
    let run = crate::core::proc::run_with_timeout(exe, &["cleanup", "-n"], PREVIEW_TIMEOUT)?;
    if !run.ok {
        return None;
    }
    parse_cleanup_output(&String::from_utf8_lossy(&run.stdout))
}

/// 解析 `brew cleanup -n` 的输出，提取体积估算与将删除条目数。
///
/// 单独抽出来是为了离线测试——brew 的输出文案随版本漂移，这里必须
/// 同时覆盖新旧两套格式，否则哪天 brew 改了措辞我们会在用户机器上
/// 静默少报。
pub(crate) fn parse_cleanup_output(stdout: &str) -> Option<(u64, u64)> {
    // 摘要行给体积估算。输出文案随版本漂移过（实测 brew 4.x 是
    // `==> This operation would free approximately 140.9MB of disk
    // space.`，更早的版本是 `This operation has freed approximately
    // ...`），所以只锚定稳定的两端：`approximately ` 前缀与
    // ` of disk space.` 后缀，动词和 `==>` 装饰不参与匹配。
    let bytes = stdout.lines().find_map(|line| {
        let start = line.find("approximately ")? + "approximately ".len();
        let rest = line.get(start..)?.strip_suffix(" of disk space.")?;
        parse_human_size(rest)
    });
    // 条目数：`Would remove` / `Prune:` / `Removing:` 开头的行各是一条
    // 将删除的内容（同样覆盖新旧两套文案）。
    let files = stdout
        .lines()
        .filter(|line| {
            line.starts_with("Would remove")
                || line.starts_with("Prune:")
                || line.starts_with("Removing:")
        })
        .count() as u64;

    // 体积拿得到但条目数为 0（输出格式变了、只有汇总行）时，按体积
    // 判断有没有可清内容；体积也拿不到时靠条目数。两个都是 0 = 没东西可清。
    match (bytes, files) {
        (Some(b), _) if b > 0 => Some((b, files.max(1))),
        (None, f) if f > 0 => Some((0, f)),
        _ => None,
    }
}

/// 真实清理：`brew cleanup`。成功（退出码 0）时在 Settings 里记下时间
/// （供 [`should_offer`] 节流）；失败如实返回 false，由 `cleaner` 报
/// Failed，不记时间——下次扫描还会再提示。
pub fn run_cleanup() -> bool {
    let Some(exe) = brew_exe() else {
        return false;
    };
    let ok = crate::core::proc::run_with_timeout(exe, &["cleanup"], CLEANUP_TIMEOUT)
        .is_some_and(|run| run.ok);
    if ok {
        let mut settings = crate::core::settings::Settings::load();
        settings.brew_cleanup_at = Some(chrono::Local::now().timestamp());
        settings.save();
    }
    ok
}

/// 构造清理目标用的虚拟路径。
pub fn virtual_path() -> PathBuf {
    PathBuf::from("brew://cleanup")
}

/// `path` 是否是 brew 清理的虚拟路径。
pub fn is_brew_virtual(path: &Path) -> bool {
    path.to_string_lossy() == "brew://cleanup"
}

/// 解析 brew 摘要行里的人类可读体积（`2.1MB` / `512KB` / `1.2GB`）。
///
/// brew 的格式是数字紧挨双字母单位、无空格、无小数点分隔的本地化
/// 差异（brew 输出固定英文）。拿不准就返回 None，调用方降级处理。
fn parse_human_size(s: &str) -> Option<u64> {
    let s = s.trim();
    let split_at = s.find(|c: char| c.is_ascii_alphabetic())?;
    let (num, unit) = s.split_at(split_at);
    let value: f64 = num.parse().ok()?;
    // brew 用 1000 进制（与 macOS Finder 一致），见 model::fmt_size 的
    // 平台口径说明。
    let multiplier: f64 = match unit {
        "B" => 1.0,
        "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        _ => return None,
    };
    Some((value * multiplier) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_human_size_covers_brew_units() {
        assert_eq!(parse_human_size("2.1MB"), Some(2_100_000));
        assert_eq!(parse_human_size("512KB"), Some(512_000));
        assert_eq!(parse_human_size("1.2GB"), Some(1_200_000_000));
        assert_eq!(parse_human_size("999B"), Some(999));
        assert_eq!(parse_human_size("bogus"), None);
        assert_eq!(parse_human_size("2.1"), None);
    }

    #[test]
    fn throttle_decision_by_age() {
        let now = chrono::Local::now().timestamp();
        assert!(should_offer(None), "从没清过：该提示");
        assert!(
            !should_offer(Some(now - 3600)),
            "一小时前清过：七天内不该再提示"
        );
        assert!(
            should_offer(Some(now - (THROTTLE_DAYS + 1) * 24 * 3600)),
            "超过节流窗口：该提示"
        );
    }

    #[test]
    fn brew_virtual_path_round_trip() {
        let p = virtual_path();
        assert!(is_brew_virtual(&p));
        assert!(crate::core::model::is_virtual_path(&p));
        assert!(!is_brew_virtual(Path::new("/tmp/not-brew")));
    }

    /// 锁死实测的 brew 4.x dry-run 输出格式（`Would remove:` + `==>
    /// This operation would free approximately 140.9MB of disk space.`）。
    #[test]
    fn parse_brew_4x_output() {
        let out = "\
Pruning symlinks...
Would remove: /opt/homebrew/Cellar/foo/1.0 (15 files, 3.1MB)
Would remove: /opt/homebrew/Cellar/bar/2.0 (8 files, 1.2MB)
==> This operation would free approximately 140.9MB of disk space.
";
        // 体积取摘要行的 140.9MB（≈140,900,000）；条目数按 `Would remove`
        // 行数。两者都有效时体积优先、条目数保全。
        let (bytes, files) = parse_cleanup_output(out).expect("应能解析 brew 4.x 输出");
        assert_eq!(bytes, 140_900_000);
        assert_eq!(files, 2);
    }

    /// 旧版 brew 的措辞（`This operation has freed approximately`，无
    /// `==>`、`Prune:` 而非 `Would remove`）同样得解析出来。
    #[test]
    fn parse_legacy_brew_output() {
        let out = "\
Prune: /usr/local/Cellar/foo/1.0
Prune: /usr/local/Cellar/bar/2.0
This operation has freed approximately 512KB of disk space.
";
        let (bytes, files) = parse_cleanup_output(out).expect("应能解析旧版 brew 输出");
        assert_eq!(bytes, 512_000);
        assert_eq!(files, 2);
    }

    /// 只有摘要行、没有任何 `Would remove`/`Prune:` 时，按体积判断有内容。
    #[test]
    fn parse_volume_only_falls_back_to_summary() {
        let out = "==> This operation would free approximately 2.1MB of disk space.\n";
        let (bytes, files) = parse_cleanup_output(out).expect("有体积就该有条目");
        assert_eq!(bytes, 2_100_000);
        assert_eq!(files, 1);
    }

    /// 真的没有可清内容时（正常退出但空输出）返回 None——界面上不该冒出
    /// 一条 0 B 的假条目。
    #[test]
    fn parse_empty_output_is_none() {
        assert_eq!(parse_cleanup_output("Pruning symlinks...\n"), None);
    }
}
