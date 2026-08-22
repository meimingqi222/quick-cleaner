//! 核心通用数据模型与工具函数

use std::path::{Path, PathBuf};

/// APFS 本地快照的虚拟路径前缀。
///
/// 快照没有可枚举的文件系统路径，扫描管线又要求每个目标都有一个 `PathBuf`，
/// 所以用 `tmutil://snapshot/<name>` 这条虚拟路径代表它：
/// `categories::macos` 造，`scanner` 靠它跳过 `symlink_metadata` 与称重
/// （COW 快照的实际占用取不到，一律记 0），`cleaner` 靠它路由到
/// `tmutil deletelocalsnapshots`。
///
/// 三方各自写一遍前缀字面量必然漂移，构造与判定都只走这里的三个函数。
const SNAPSHOT_PREFIX: &str = "tmutil://snapshot/";

/// 由快照名造虚拟路径。
pub fn snapshot_path(name: &str) -> PathBuf {
    PathBuf::from(format!("{SNAPSHOT_PREFIX}{name}"))
}

/// 是否是虚拟路径——即不该拿去做任何文件系统调用的目标。
pub fn is_virtual_path(path: &Path) -> bool {
    path.to_string_lossy().starts_with(SNAPSHOT_PREFIX)
}

/// 取回虚拟路径里的快照名，不是快照路径则为 `None`。
pub fn snapshot_name(path: &Path) -> Option<String> {
    path.to_string_lossy()
        .strip_prefix(SNAPSHOT_PREFIX)
        .map(str::to_string)
}

/// 复选框三态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Off,
    On,
    Partial,
}

impl Check {
    /// 由「选中数 / 总数」推出父级复选框的三态。
    ///
    /// 空集合视为未选中，避免出现「零个子项却显示全选」。
    pub fn from_counts(selected: usize, total: usize) -> Self {
        if total == 0 || selected == 0 {
            Check::Off
        } else if selected >= total {
            Check::On
        } else {
            Check::Partial
        }
    }
}

/// 格式化字节大小为可读字符串（KB, MB, GB, TB）
///
/// 进制按平台对齐系统显示标准：
///   - Windows 资源管理器用 1024 进制（KiB/MiB/GiB），自 Windows 95 起未变
///   - macOS Finder / 储存空间用 1000 进制（SI），自 10.6 Snow Leopard 起改用
///
/// 调用方无需关心平台差异，统一调 `fmt_size` 即可。
pub fn fmt_size(bytes: u64) -> String {
    // cfg!() 在编译时求值，整个 if 会在 const eval 阶段折叠为常量。
    const BASE: f64 = if cfg!(windows) { 1024.0 } else { 1000.0 };
    const KB: f64 = BASE;
    const MB: f64 = KB * BASE;
    const GB: f64 = MB * BASE;
    const TB: f64 = GB * BASE;

    let b = bytes as f64;
    if b >= TB {
        format!("{:.2} TB", b / TB)
    } else if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// 给大数字添加千分位逗号，提升可读性
pub fn commas(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// 字符串长度截断加省略号
pub fn truncate(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 卷标签靠它截断。按字节切会在中文字符中间 panic。
    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate("短名", 22), "短名");
        // 24 字节 / 8 字符，不足 22 字符，应原样返回
        assert_eq!(truncate("我的外置移动硬盘", 22), "我的外置移动硬盘");
        assert_eq!(truncate("一二三四", 2), "一二…");
        assert_eq!(truncate("🍎🍎🍎", 1), "🍎…");
    }

    /// 构造与解析必须是一对：cleaner 拿 `snapshot_name` 的结果直接喂给
    /// `tmutil deletelocalsnapshots`，多切或少切一段前缀就是删不掉。
    #[test]
    fn snapshot_path_round_trips() {
        let p = snapshot_path("com.apple.TimeMachine.2024-01-15-123456");
        assert!(is_virtual_path(&p));
        assert_eq!(
            snapshot_name(&p).as_deref(),
            Some("com.apple.TimeMachine.2024-01-15-123456")
        );
    }

    #[test]
    fn real_paths_are_not_virtual() {
        let p = Path::new("/Users/me/Library/Caches");
        assert!(!is_virtual_path(p));
        assert_eq!(snapshot_name(p), None);
    }

    #[test]
    fn test_fmt_size() {
        assert_eq!(fmt_size(500), "500 B");
        #[cfg(windows)]
        {
            assert_eq!(fmt_size(2048), "2 KB");
            assert_eq!(fmt_size(15 * 1024 * 1024), "15.0 MB");
            assert_eq!(fmt_size(2 * 1024 * 1024 * 1024), "2.00 GB");
        }
        #[cfg(not(windows))]
        {
            assert_eq!(fmt_size(2000), "2 KB");
            assert_eq!(fmt_size(15 * 1000 * 1000), "15.0 MB");
            assert_eq!(fmt_size(2 * 1000 * 1000 * 1000), "2.00 GB");
        }
    }

    #[test]
    fn test_check_from_counts() {
        assert_eq!(Check::from_counts(0, 0), Check::Off);
        assert_eq!(Check::from_counts(0, 5), Check::Off);
        assert_eq!(Check::from_counts(2, 5), Check::Partial);
        assert_eq!(Check::from_counts(5, 5), Check::On);
    }

    #[test]
    fn test_commas() {
        assert_eq!(commas(100), "100");
        assert_eq!(commas(1000), "1,000");
        assert_eq!(commas(1000000), "1,000,000");
    }
}
