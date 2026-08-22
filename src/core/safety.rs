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

/// 相对**用户 profile 根**、目录本身不能删但内容可以清的路径。
///
/// 用「profile 根 + 相对路径」而不是简单的尾部匹配：后者会把随便哪个目录
/// 底下叫 `appdata` 的文件夹也一并挡住。而之所以不锚定当前用户的主目录，
/// 是因为多 profile 机器上、以及跨账户提权时，扫描会走到别的用户的
/// `AppData` 底下，那些同样不能整删。
///
/// `ProgramData` 不在这里——它挂在盘符根下，已经由 [`DRIVE_EXACT`] 覆盖。
const PROFILE_EXACT: &[&str] = &[
    "\\appdata",
    "\\appdata\\local",
    "\\appdata\\locallow",
    "\\appdata\\roaming",
];

struct Guards {
    /// 归一化后的 %SystemRoot%，如 `c:\windows`
    windows: String,
    /// 归一化后的当前进程用户主目录，如 `c:\users\administrator`
    home: Option<String>,
    /// 归一化后的真实前台操作用户主目录（跨账户提权时为原登录用户，如 `c:\users\alice`）
    orig_home: Option<String>,
    /// 归一化后的「已知文件夹」实际落点（桌面 / 文档 / 下载 / 图片……）。
    ///
    /// 光靠 `HOME_EXACT` 里那份 `\desktop` 清单是不够的：OneDrive 备份会把
    /// 桌面重定向到 `%USERPROFILE%\OneDrive\桌面`，中文系统上这些目录的磁盘
    /// 名本身就是本地化的，企业环境还可能整体挪到网络盘。
    /// 见 `platform::windows::real_user_known_folders`。
    known_folders: Vec<String>,
}

fn guards() -> &'static Guards {
    static GUARDS: OnceLock<Guards> = OnceLock::new();
    GUARDS.get_or_init(|| {
        #[cfg(windows)]
        let orig_home = Some(norm(crate::platform::windows::real_user_home()));
        #[cfg(not(windows))]
        let orig_home = None;

        #[cfg(windows)]
        let known_folders = crate::platform::windows::real_user_known_folders()
            .iter()
            .map(|p| norm(p))
            .collect();
        #[cfg(not(windows))]
        let known_folders = Vec::new();

        Guards {
            windows: norm_str(
                &std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()),
            ),
            home: dirs::home_dir().map(|h| norm(&h)),
            orig_home,
            known_folders,
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
        if c == '/' {
            out.push('\\');
        } else if c.is_ascii() {
            // 绝大多数字符走这条：单字节、不展开
            out.push(c.to_ascii_lowercase());
        } else {
            // 非 ASCII 也得折叠大小写。用户名带重音符时（`C:\Users\Ömer`），
            // 只做 ASCII 折叠会让 `c:\users\ÖMER\desktop` 和守卫里存的
            // `c:\users\ömer` 对不上，那个用户的桌面就保护不到了。
            //
            // `to_lowercase` 可能一对多（`İ` → `i̇`），但路径两边都过同一个
            // 函数，比较依然自洽。
            out.extend(c.to_lowercase());
        }
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

/// `lower` 是否恰好是某个用户 profile 的根，形如 `c:\users\alice`。
///
/// 「恰好」很重要：`c:\users\alice\appdata` 不算，否则 `PROFILE_EXACT` 的
/// 相对路径就会在错误的层级上生效。
fn is_profile_root(lower: &str) -> bool {
    let Some(rest) = lower.get(2..) else {
        return false;
    };
    let Some(name) = rest.strip_prefix("\\users\\") else {
        return false;
    };
    !name.is_empty() && !name.contains('\\')
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
    #[cfg(target_os = "macos")]
    if MACOS_PROTECTED_PREFIXES
        .iter()
        .any(|prefix| at_or_under(&lower, prefix))
    {
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

/// 软件残留扫描的额外保护：公共骨架目录不能整个当作某个软件的残留删掉。
///
/// `AppData` 那几层以前只挡在这里，而 `clean_arbitrary`（磁盘透镜的任意
/// 路径删除）走的是 [`is_protected`]，两条删除路径口径不一致。现在那几层
/// 已经并进 [`is_protected`]，这里只额外补上「顶层骨架」这一档。
pub fn is_protected_residual_path(path: &Path) -> bool {
    is_system_root_dir(path) || is_protected(path)
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

    // macOS 的系统骨架此前完全依赖 SIP/文件权限兜底；拿到完全磁盘访问后，
    // 磁盘透镜的任意路径删除仍可能触碰其中部分内容。应用卸载有独立入口，
    // 系统树和 /Applications 不应由通用清理器删除。
    #[cfg(target_os = "macos")]
    if MACOS_PROTECTED_PREFIXES
        .iter()
        .any(|prefix| at_or_under(&lower, prefix))
    {
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

    // ---- 已知文件夹（可能被 OneDrive / 组策略重定向到任意位置）----
    if g.known_folders.contains(&lower) {
        return true;
    }

    // ---- 任意用户 profile 下的 AppData 各层 ----
    if PROFILE_EXACT
        .iter()
        .any(|sfx| lower.strip_suffix(sfx).is_some_and(is_profile_root))
    {
        return true;
    }

    false
}

/// 应用残留清理**唯一**允许提权触碰的系统目录，写法是「父目录」。
///
/// `/Library` 整棵子树在 [`MACOS_PROTECTED_PREFIXES`] 里是禁删的，磁盘透镜的
/// 任意路径删除绝不能碰。但 launchd plist 和系统级支持目录本来就只住在那儿，
/// 一刀切的后果是这些残留在 UI 上列得出来、点了清理却静默失败。
///
/// 口子开在「父目录精确等于表中某项」这一层，不是前缀匹配。三重后果：
/// - `/Library/Application Support` 自身删不掉（它的父目录是 `/library`）
/// - `/Library/Application Support/org.pqrs` 可以（残留扫描产出的就是这一层）
/// - `/Library/Application Support/org.pqrs/Karabiner-Elements` 不行——再深
///   一层意味着调用方算错了粒度，宁可失败也不能以 root 身份 `rm -rf` 下去
#[cfg(target_os = "macos")]
const MACOS_ELEVATED_RESIDUAL_PARENTS: &[&str] = &[
    "\\library\\application support",
    "\\library\\application scripts",
    "\\library\\caches",
    "\\library\\logs",
    "\\library\\preferences",
    "\\library\\launchagents",
    "\\library\\launchdaemons",
    "\\library\\privilegedhelpertools",
    "\\private\\var\\db\\receipts",
];

/// `/Library` 白名单目录下、不以 `com.apple.` 命名的系统组件。
#[cfg(target_os = "macos")]
const MACOS_SYSTEM_COMPONENT_NAMES: &[&str] = &[
    "systemconfiguration",
    "crashreporter",
    "keychains",
    "security",
    "preferencepanes",
    "systemprofiler",
    "logs",
    "caches",
    "managedpreferences",
    "systemmigration",
    "apple",
];

/// 该路径是否属于「残留清理可以提权删除」的白名单。
///
/// 只有 macOS 残留清理这一条调用链可以用它绕开 [`is_protected`]；磁盘透镜、
/// 分类清理都不许调，否则 `/Library` 的保护就形同虚设。
#[cfg(target_os = "macos")]
pub fn is_elevated_residual_target(path: &Path) -> bool {
    // 符号链接会让「父目录在白名单里」这个判断失去意义：`/Library/Caches/x`
    // 可以指向任意位置，以 root 身份 rm -rf 过去就是任意文件删除。
    match std::fs::symlink_metadata(path) {
        Ok(md) if !md.file_type().is_symlink() => {}
        _ => return false,
    }
    // 白名单目录里混着系统自己的东西：`/Library/Preferences/com.apple.*` 是
    // 登录窗口、SystemConfiguration 这类配置，`/Library/Application Support`
    // 下还有 com.apple.TCC。以 root 身份删掉其中任何一个都是在拆系统。
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name.starts_with("com.apple.") || name == "com.apple" {
        return false;
    }
    // 白名单目录里还住着一批不带反向域名的系统组件。厂商前缀匹配够不到它们
    // （前缀一定含点，而这些名字不含点），但提权删除的口子不该依赖上游扫描器
    // 的实现细节来保证安全。
    if MACOS_SYSTEM_COMPONENT_NAMES
        .iter()
        .any(|reserved| name == *reserved)
    {
        return false;
    }

    let Some(parent) = path.parent() else {
        return false;
    };
    let parent = norm(parent);
    MACOS_ELEVATED_RESIDUAL_PARENTS
        .iter()
        .any(|allowed| parent == *allowed)
}

#[cfg(not(target_os = "macos"))]
pub fn is_elevated_residual_target(_path: &Path) -> bool {
    false
}

#[cfg(target_os = "macos")]
const MACOS_PROTECTED_PREFIXES: &[&str] = &[
    "\\system",
    "\\library",
    "\\applications",
    "\\bin",
    "\\sbin",
    "\\usr",
    "\\private\\etc",
    "\\private\\var\\db",
    "\\private\\var\\root",
    "\\private\\var\\vm",
    "\\private\\var\\protected",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protects_volume_roots() {
        assert!(is_protected(Path::new("C:\\")));
        assert!(is_protected(Path::new("C:")));
        assert!(is_protected(Path::new("D:\\")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn elevated_residual_allowlist_only_opens_one_level() {
        // 扫描器产出的就是这一层，必须放行——否则 UI 列得出来、点了却静默失败
        assert!(is_elevated_residual_target(Path::new(
            "/Library/Application Support/org.pqrs"
        )));
        assert!(is_elevated_residual_target(Path::new(
            "/Library/LaunchDaemons/org.pqrs.karabiner.karabiner_grabber.plist"
        )));
        assert!(is_elevated_residual_target(Path::new(
            "/private/var/db/receipts/org.pqrs.Karabiner-Elements.bom"
        )));

        // 骨架目录自身：父目录是 /library，不在表里
        assert!(!is_elevated_residual_target(Path::new(
            "/Library/Application Support"
        )));
        assert!(!is_elevated_residual_target(Path::new("/Library")));
        // 再深一层说明调用方算错了粒度
        assert!(!is_elevated_residual_target(Path::new(
            "/Library/Application Support/org.pqrs/Karabiner-Elements"
        )));
        // 白名单之外
        assert!(!is_elevated_residual_target(Path::new(
            "/System/Library/CoreServices"
        )));
        assert!(!is_elevated_residual_target(Path::new("/usr/bin/env")));
        assert!(!is_elevated_residual_target(Path::new(
            "/Library/Extensions/foo.kext"
        )));
    }

    /// 白名单目录里混着 Apple 自己的配置，以 root 身份删掉就是拆系统。
    #[cfg(target_os = "macos")]
    #[test]
    fn elevated_residual_allowlist_never_touches_apple_items() {
        for p in [
            "/Library/Preferences/com.apple.loginwindow.plist",
            "/Library/Application Support/com.apple.TCC",
            "/Library/LaunchDaemons/com.apple.smbd.plist",
        ] {
            assert!(
                !is_elevated_residual_target(Path::new(p)),
                "{p} 不应被允许提权删除"
            );
        }
    }

    /// 白名单目录里还住着不带反向域名的系统组件。
    #[cfg(target_os = "macos")]
    #[test]
    fn elevated_residual_allowlist_never_touches_bare_system_components() {
        for p in [
            "/Library/Preferences/SystemConfiguration",
            "/Library/Application Support/CrashReporter",
            "/Library/Caches/Keychains",
        ] {
            assert!(
                !is_elevated_residual_target(Path::new(p)),
                "{p} 不应被允许提权删除"
            );
        }
    }

    /// 提权删除的口子只对残留清理开放，磁盘透镜那条路必须照旧全挡。
    #[cfg(target_os = "macos")]
    #[test]
    fn library_subtree_stays_protected_for_generic_cleaning() {
        for p in [
            "/Library/Application Support/org.pqrs",
            "/Library/LaunchDaemons/org.pqrs.x.plist",
            "/private/var/db/receipts/org.pqrs.x.bom",
        ] {
            assert!(is_protected(Path::new(p)), "{p} 对通用清理仍应是保护路径");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn protects_macos_system_trees() {
        for path in [
            "/System/Library/CoreServices",
            "/Library/Application Support",
            "/Applications/Safari.app",
            "/usr/local/bin/tool",
            "/private/etc/hosts",
            "/private/var/db/receipts/example.plist",
        ] {
            assert!(is_protected(Path::new(path)), "未保护 {path}");
        }
        assert!(
            !is_protected(Path::new("/private/var/folders/user/C/cache")),
            "每用户 Darwin 缓存仍需允许精确清理"
        );
    }

    #[test]
    fn protects_system_subtrees() {
        assert!(is_protected(Path::new("C:\\Windows\\System32")));
        assert!(is_protected(Path::new(
            "C:\\Windows\\System32\\drivers\\etc"
        )));
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

    /// 非 ASCII 的用户名也要能折叠大小写，否则守卫比对不上。
    #[test]
    fn normalisation_folds_non_ascii_case() {
        assert_eq!(
            norm(Path::new("C:/Users/ÖMER")),
            norm(Path::new(r"c:\users\ömer"))
        );
        assert_eq!(norm(Path::new(r"D:\ÄÖÜ\")), r"d:\äöü");
        // 中文没有大小写概念，原样保留
        assert_eq!(norm(Path::new(r"C:\用户\文档")), r"c:\用户\文档");
    }

    #[test]
    fn at_or_under_respects_component_boundary() {
        assert!(at_or_under(
            "c:\\windows\\system32",
            "c:\\windows\\system32"
        ));
        assert!(at_or_under(
            "c:\\windows\\system32\\x",
            "c:\\windows\\system32"
        ));
        // 不能把 system32foo 误判成 system32 的子路径
        assert!(!at_or_under(
            "c:\\windows\\system32foo",
            "c:\\windows\\system32"
        ));
    }

    /// AppData 那几层以前只有残留扫描挡着，磁盘透镜的任意路径删除绕得过去。
    /// 现在两条路径共用同一份规则。
    #[test]
    fn profile_skeleton_dirs_are_protected_on_every_path() {
        for p in [
            r"C:\Users\me\AppData",
            r"C:\Users\me\AppData\Local",
            r"C:\Users\me\AppData\LocalLow",
            r"C:\Users\me\AppData\Roaming",
            r"C:\ProgramData",
            // 别的用户的 profile 同样要挡住（多账户机器 / 跨账户提权）
            r"D:\Users\someone-else\AppData\Local",
        ] {
            assert!(is_protected(Path::new(p)), "{p} 应当受保护");
            assert!(
                is_protected_residual_path(Path::new(p)),
                "{p} 残留路径也该受保护"
            );
        }
    }

    /// 但它们的**内容**照样可以清——保护的是目录本身，不是整棵子树。
    #[test]
    fn contents_under_appdata_stay_cleanable() {
        for p in [
            r"C:\Users\me\AppData\Local\Temp",
            r"C:\Users\me\AppData\Local\SomeApp\Cache",
            r"C:\ProgramData\SomeVendor\logs",
        ] {
            assert!(!is_protected(Path::new(p)), "{p} 不该被挡住");
        }
    }

    /// 名字里带 appdata 但不是那几层的目录不能被误伤。
    #[test]
    fn suffix_match_does_not_overreach() {
        assert!(!is_protected(Path::new(r"C:\Users\me\myappdata")));
        assert!(!is_protected(Path::new(
            r"C:\Users\me\AppData\Local\appdata"
        )));
    }

    #[test]
    fn system_root_dirs() {
        assert!(is_system_root_dir(Path::new("C:\\")));
        assert!(is_system_root_dir(Path::new("C:\\Program Files (x86)")));
        assert!(is_system_root_dir(Path::new("C:\\Users")));
        assert!(!is_system_root_dir(Path::new("C:\\Users\\me")));
    }
}
