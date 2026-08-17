//! 核心通用数据模型与工具函数

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

/// 格式化字节大小为可读字符串（KB, MB, GB）
///
/// 使用 1024 进制（KiB/MiB/GiB），适合文件/目录大小显示。
pub fn fmt_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// 格式化字节大小为可读字符串（KB, MB, GB），使用 1000 进制。
///
/// macOS / CleanMyMac / Finder 以及磁盘制造商都使用 1000 进制（SI）
/// 显示磁盘容量。磁盘透镜的总容量/已用/空闲用它显示，
/// 才能和系统「关于本机 → 储存空间」以及 CleanMyMac 对齐。
pub fn fmt_size_si(bytes: u64) -> String {
    const KB: f64 = 1000.0;
    const MB: f64 = KB * 1000.0;
    const GB: f64 = MB * 1000.0;
    const TB: f64 = GB * 1000.0;

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

    #[test]
    fn test_fmt_size() {
        assert_eq!(fmt_size(500), "500 B");
        assert_eq!(fmt_size(2048), "2 KB");
        assert_eq!(fmt_size(15 * 1024 * 1024), "15.0 MB");
        assert_eq!(fmt_size(2 * 1024 * 1024 * 1024), "2.00 GB");
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
