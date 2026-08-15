//! 统一的路径安全防护规则
//!
//! 这里是「什么绝对不能删」的**唯一**事实来源。历史上这份清单在
//! `cleaner`、`mft`、`windows::apps`、`windows::residuals` 里各存了一份，
//! 任何一处新增保护项而其它三处忘记同步，都会直接导致数据损坏。
//!
//! 所有规则表都在 `OnceLock` 里构建一次。`is_protected` 处于渲染热路径上
//! （磁盘透镜每行每帧都要调），因此除了归一化路径本身，不再有额外堆分配。

use std::path::Path;
use std::sync::OnceLock;

/// NTFS 核心元数据与系统锁定根文件（按文件名匹配，大小写不敏感）
const NTFS_META_NAMES: &[&str] = &[
    "$logfile",
    "$volume",
    "$attrdef",
    "$bitmap",
    "$boot",
    "$badclus",
    "$secure",
    "$upcase",
    "$extend",
    "pagefile.sys",
    "hiberfil.sys",
    "swapfile.sys",
    "system volume information",
];

/// 相对盘符根、整棵子树都禁止触碰的路径
const DRIVE_PREFIXES: &[&str] = &[
    "\\system volume information",
    "\\$windows.~bt",
    "\\recovery",
];

/// 相对盘符根、目录本身不能删但内容可以清的路径
const DRIVE_EXACT: &[&str] = &[
    "\\program files",
    "\\program files (x86)",
    "\\programdata",
    "\\users",
];

/// 相对 %SystemRoot%、整棵子树都禁止的路径
const WIN_PREFIXES: &[&str] = &[
    "\\system32",
    "\\syswow64",
    "\\winsxs",
    "\\boot",
    "\\fonts",
    "\\servicing",
];

/// 相对用户主目录、目录本身不能删的路径（"" 表示主目录自身）
const HOME_EXACT: &[&str] = &[
    "",
    "\\desktop",
    "\\documents",
    "\\downloads",
    "\\pictures",
    "\\videos",
    "\\music",
];

struct Guards {
    /// 归一化后的 %SystemRoot%，如 `c:\windows`
    windows: String,
    /// 归一化后的当前进程用户主目录，如 `c:\users\administrator`
    home: Option<String>,
    /// 归一化后的真实前台操作用户主目录（跨账户提权时为原登录用户，如 `c:\users\alice`）
    orig_home: Option<String>,
}

fn guards() -> &'static Guards {
    static GUARDS: OnceLock<Guards> = OnceLock::new();
    GUARDS.get_or_init(|| {
        #[cfg(windows)]
        let orig_home = Some(norm(crate::platform::windows::real_user_home()));
        #[cfg(not(windows))]
        let orig_home = None;

        Guards {
            windows: norm_str(
                &std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()),
            ),
            home: dirs::home_dir().map(|h| norm(&h)),
            orig_home,
        }
    })
}

/// 归一化：统一分隔符、去掉尾部分隔符、转小写。
pub fn norm(path: &Path) -> String {
    norm_str(&path.to_string_lossy())
}

fn norm_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        out.push(if c == '/' { '\\' } else { c.to_ascii_lowercase() });
    }
    while out.ends_with('\\') {
        out.pop();
    }
    out
}

/// `lower` 是否等于 `base` 本身，或位于 `base` 之下。
fn at_or_under(lower: &str, base: &str) -> bool {
    lower.len() >= base.len()
        && lower.starts_with(base)
        && (lower.len() == base.len() || lower.as_bytes()[base.len()] == b'\\')
}

/// ASCII 大小写不敏感的前缀匹配，不分配。
fn starts_with_ci(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// 文件名是否是 NTFS 元数据 / 系统锁定文件。
pub fn is_ntfs_meta_name(name: &str) -> bool {
    // $MFT / $MFTMirr 都以 $mft 开头；DumpStack.log.tmp 同理
    starts_with_ci(name, "$mft")
        || starts_with_ci(name, "dumpstack.log")
        || NTFS_META_NAMES.iter().any(|m| name.eq_ignore_ascii_case(m))
}

/// 是否是盘符根、`C:\Windows`、`C:\Program Files` 这类系统骨架目录。
///
/// 与 [`is_protected`] 的区别：这里只判断「顶层骨架」，用于软件残留扫描时
/// 防止把公共根目录整个当成某个软件的残留。
pub fn is_system_root_dir(path: &Path) -> bool {
    let lower = norm(path);
    if lower.len() <= 3 {
        return true;
    }
    if lower == guards().windows {
        return true;
    }
    match lower.get(2..) {
        Some(rest) => DRIVE_EXACT.contains(&rest),
        None => false,
    }
}

/// 软件残留扫描的额外保护：AppData 及其直接子层不能整个当作残留删掉。
pub fn is_protected_residual_path(path: &Path) -> bool {
    if is_system_root_dir(path) {
        return true;
    }
    let lower = norm(path);
    lower.ends_with("\\appdata")
        || lower.ends_with("\\appdata\\local")
        || lower.ends_with("\\appdata\\roaming")
        || lower.ends_with("\\appdata\\locallow")
        || lower.ends_with("\\programdata")
}

/// 绝对不能删除的路径。
///
/// 语义分两档：
/// - **子树禁止**（`System32`、`WinSxS`、`System Volume Information` …）：自身和内部全部不可删。
/// - **自身禁止**（`C:\Windows`、`%TEMP%`、用户主目录 …）：目录本身要保留，但内容可以清。
pub fn is_protected(path: &Path) -> bool {
    let lower = norm(path);

    // 盘符根目录，如 "c:" / "c:\"
    if lower.len() <= 3 {
        return true;
    }

    if let Some(fname) = path.file_name().and_then(|f| f.to_str()) {
        if is_ntfs_meta_name(fname) {
            return true;
        }
    }

    let g = guards();

    // ---- %SystemRoot% 锚定 ----
    // 先判断是否落在 %SystemRoot% 下，再拿剩余部分比对，全程零分配。
    if at_or_under(&lower, &g.windows) {
        let rest = &lower[g.windows.len()..];
        // rest 为空表示 C:\Windows 自身
        if rest.is_empty() || rest == "\\temp" {
            return true;
        }
        if WIN_PREFIXES.iter().any(|p| at_or_under(rest, p)) {
            return true;
        }
    }

    // ---- 盘符锚定 ----
    if let Some(rest) = lower.get(2..) {
        if DRIVE_PREFIXES.iter().any(|p| at_or_under(rest, p)) {
            return true;
        }
        if DRIVE_EXACT.contains(&rest) {
            return true;
        }
    }

    // ---- 用户主目录锚定（同时严格保护当前进程 Home 与真实前台用户 Home） ----
    let check_home = |home_opt: &Option<String>| -> bool {
        if let Some(home) = home_opt {
            if at_or_under(&lower, home) {
                let rest = &lower[home.len()..];
                if HOME_EXACT.contains(&rest) {
                    return true;
                }
            }
        }
        false
    };

    if check_home(&g.home) || check_home(&g.orig_home) {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protects_volume_roots() {
        assert!(is_protected(Path::new("C:\\")));
        assert!(is_protected(Path::new("C:")));
        assert!(is_protected(Path::new("D:\\")));
    }

    #[test]
    fn protects_system_subtrees() {
        assert!(is_protected(Path::new("C:\\Windows\\System32")));
        assert!(is_protected(Path::new("C:\\Windows\\System32\\drivers\\etc")));
        assert!(is_protected(Path::new("c:/windows/winsxs/whatever")));
        assert!(is_protected(Path::new("D:\\System Volume Information\\x")));
    }

    #[test]
    fn protects_key_dirs_themselves_but_not_contents() {
        assert!(is_protected(Path::new("C:\\Windows")));
        assert!(is_protected(Path::new("C:\\Windows\\Temp")));
        assert!(!is_protected(Path::new("C:\\Windows\\Temp\\abc.tmp")));
        assert!(!is_protected(Path::new("C:\\Windows\\Logs\\CBS")));
        assert!(is_protected(Path::new("C:\\Program Files")));
        assert!(!is_protected(Path::new("C:\\Program Files\\Foo")));
    }

    #[test]
    fn allows_ordinary_cache_paths() {
        assert!(!is_protected(Path::new("C:\\Users\\me\\.cargo\\registry")));
        assert!(!is_protected(Path::new("C:\\Users\\me\\go\\pkg\\mod")));
    }

    #[test]
    fn ntfs_meta_names_are_case_insensitive() {
        assert!(is_ntfs_meta_name("$MFT"));
        assert!(is_ntfs_meta_name("$MFTMirr"));
        assert!(is_ntfs_meta_name("pagefile.sys"));
        assert!(is_ntfs_meta_name("DumpStack.log.tmp"));
        assert!(is_ntfs_meta_name("System Volume Information"));
        assert!(!is_ntfs_meta_name("notes.txt"));
    }

    #[test]
    fn at_or_under_respects_component_boundary() {
        assert!(at_or_under("c:\\windows\\system32", "c:\\windows\\system32"));
        assert!(at_or_under("c:\\windows\\system32\\x", "c:\\windows\\system32"));
        // 不能把 system32foo 误判成 system32 的子路径
        assert!(!at_or_under("c:\\windows\\system32foo", "c:\\windows\\system32"));
    }

    #[test]
    fn system_root_dirs() {
        assert!(is_system_root_dir(Path::new("C:\\")));
        assert!(is_system_root_dir(Path::new("C:\\Program Files (x86)")));
        assert!(is_system_root_dir(Path::new("C:\\Users")));
        assert!(!is_system_root_dir(Path::new("C:\\Users\\me")));
    }
}
