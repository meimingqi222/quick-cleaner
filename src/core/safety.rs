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

/// 相对 `%PUBLIC%`（通常是 `C:\Users\Public`）、目录本身不能删的路径。
///
/// 公共桌面不是某款软件的残留。`Remote Desktop Manager` 这类名字一旦
/// 走反向包含匹配，整棵 `Public\Desktop` 都会被列出来等着删。
const PUBLIC_EXACT: &[&str] = &[
    "",
    "\\desktop",
    "\\documents",
    "\\downloads",
    "\\pictures",
    "\\videos",
    "\\music",
    "\\libraries",
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
    /// 归一化后的 `%PUBLIC%`，如 `c:\users\public`。Windows 专有。
    public: Option<String>,
    /// macOS「自身禁止」档：`~`、`~/Library`、`~/Library/Application Support`
    /// 三个目录**本身**的归一化路径。对齐 Windows 对 AppData 骨架的处理——
    /// 骨架自保、内容照常可清（旧版 IDE 数据、卸载残留都住在这里面）。
    #[cfg(target_os = "macos")]
    macos_self_banned: Vec<String>,
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

        #[cfg(windows)]
        let public = std::env::var("PUBLIC")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| norm_str(&s));
        #[cfg(not(windows))]
        let public = None;

        #[cfg(target_os = "macos")]
        let macos_self_banned = dirs::home_dir()
            .map(|h| {
                let n = norm(&h);
                [
                    n.clone(),
                    format!("{n}\\library"),
                    format!("{n}\\library\\application support"),
                ]
                .into_iter()
                .collect()
            })
            .unwrap_or_default();

        Guards {
            windows: norm_str(
                &std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()),
            ),
            home: dirs::home_dir().map(|h| norm(&h)),
            orig_home,
            known_folders,
            public,
            #[cfg(target_os = "macos")]
            macos_self_banned,
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
///
/// `pub(crate)`：`whitelist` 模块的祖先保护判断（白名单条目是不是某个候选
/// 目录的后代）复用同一套边界语义，不能各写一份——归一化规则稍有出入就会
/// 出现「白名单判定」和「保护判定」对同一路径给出不同答案的怪事。
pub(crate) fn at_or_under(lower: &str, base: &str) -> bool {
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
///   macOS 侧对齐 Windows 对 AppData 骨架的处理：`~`、`~/Library`、
///   `~/Library/Application Support` 的目录本身不可删——一键删掉整个
///   应用数据根目录是最坏事故；内容不受影响，旧版 IDE 数据、卸载残留
///   等类目照常工作。
pub fn is_protected(path: &Path) -> bool {
    let lower = norm(path);

    // 盘符根目录，如 "c:" / "c:\"
    if lower.len() <= 3 {
        return true;
    }

    // 用户白名单与系统保护表同等优先级，放在最前面：它是用户亲口说的
    // 「永远别碰」，任何删除通道都必须先过这一关。条目自身和整个子树
    // 都受保护（见 core::whitelist 模块头注释）。
    if crate::core::whitelist::is_whitelisted(path) {
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

    // macOS 自身禁止档（精确匹配，见 Guards::macos_self_banned 的注释）。
    #[cfg(target_os = "macos")]
    if guards().macos_self_banned.contains(&lower) {
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

    if let Some(public) = &g.public {
        if at_or_under(&lower, public) {
            let rest = &lower[public.len()..];
            if PUBLIC_EXACT.contains(&rest) {
                return true;
            }
        }
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

/// SQLite 事务侧伴随文件后缀：只在有连接握着数据库时才存在，正常关闭会
/// 被清掉。见 `sqlite_family_key` / `holds_live_database` / `is_live_database`。
const SQLITE_COMPANION_SUFFIXES: &[&str] = &["-wal", "-shm", "-journal"];

/// 常见 SQLite 主库扩展名。`.otc` 来自实机枚举：`~/Library/Logs/OneDrive/`
/// 顶层的 `syncReporterTelemetryCache.otc` 就是这个扩展名的活数据库。
const SQLITE_MAIN_EXTENSIONS: &[&str] = &[".db", ".sqlite", ".sqlite3", ".otc"];

/// 把文件名折成 SQLite 家族的归一化 key：主库返回自己（小写），伴随文件
/// 返回它所属主库的文件名（去掉 `-wal`/`-shm`/`-journal` 后缀）。不认识的
/// 文件名返回 `None`。
///
/// 大小写不敏感——参考项目 Mole 记录过 `.DB-WAL` 这类大写变体绕过纯大小写
/// 敏感匹配的问题；Windows 以及跨文件系统搬运场景下，文件名大小写混用
/// 并不罕见。
///
/// 同时供两处使用：`is_live_database` 判断文件是否属于活跃家族，以及
/// `cleaner::delete_tree` 按家族分组、决定组内删除顺序。两处共用一份 key
/// 计算，不能各写一套——算法哪怕有一丁点出入，「判定活库」和「决定删除
/// 顺序」这两件事就可能对不上号。
pub fn sqlite_family_key(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    for suffix in SQLITE_COMPANION_SUFFIXES {
        if let Some(base) = lower.strip_suffix(suffix) {
            return Some(base.to_string());
        }
    }
    if SQLITE_MAIN_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
        return Some(lower);
    }
    None
}

/// 文件名本身是不是 SQLite 的事务侧伴随文件（大小写不敏感）。
///
/// 供 `cleaner::delete_tree` 决定同一家族内的删除顺序：伴随文件先删、
/// 主库最后删——反过来会制造「主库已消失、`-wal` 还在」这种脏状态，正是
/// 下面 `is_live_database` 文档里那次事故的诱因之一。
pub fn is_sqlite_companion_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SQLITE_COMPANION_SUFFIXES
        .iter()
        .any(|suffix| lower.ends_with(suffix))
}

/// 目录顶层是否躺着活数据库标记。
///
/// 迁移自 `categories/helpers.rs`：那里只用它决定扫描产出的目标要不要默认
/// 勾选，是纯展示层的建议——判错的代价只是「这一项没被自动勾上，用户还是
/// 能看见、能手动选」。现在它同时被 [`is_live_database`] 复用做删除级的
/// 硬拒绝，调用点见 `cleaner::clean_path`。
///
/// 只看顶层一层：SQLite 的伴随文件永远和主库同目录，再深没有意义。读不到
/// 目录内容时 fail closed——判不出等于看不清，不能当成「没有活库」放行。
pub fn holds_live_database(dir: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return true;
    };
    rd.flatten().any(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        let lower = name.to_ascii_lowercase();
        SQLITE_COMPANION_SUFFIXES
            .iter()
            .any(|suffix| lower.ends_with(suffix))
            || SQLITE_MAIN_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
    })
}

/// 单个文件是不是「正被某个数据库连接使用」的家族成员：自己是 SQLite 主库
/// 或伴随文件，**并且**同目录的兄弟文件里能凑出「主库 + 至少一个伴随文件」
/// 的组合。
///
/// 只按扩展名判断（任何 `.db` 都算）会把缩略图缓存（Windows 的
/// `thumbcache_*.db`）、iOS 设备备份的 `Manifest.db` 这些从不带
/// `-wal`/`-shm` 的正常清理目标一并挡死——这两个类目会因此永远清不掉。
/// 只有伴随文件真的和主库同时出现，才是「此刻有连接握着它」的实证，
/// 对应下面 `is_live_database` 文档里 Fusion `Cache.db` 那次事故的成因。
///
/// 读不到父目录时 fail closed：判不出邻居是谁，不能假装安全放行。
fn is_active_sqlite_member(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|f| f.to_str()) else {
        return true;
    };
    let Some(key) = sqlite_family_key(name) else {
        return false;
    };
    let Some(parent) = path.parent() else {
        return true;
    };
    let Ok(rd) = std::fs::read_dir(parent) else {
        return true;
    };
    let members = rd
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .and_then(sqlite_family_key)
                .as_deref()
                == Some(key.as_str())
        })
        .count();
    members >= 2
}

/// 删除级闸门：这个路径是不是活数据库家族的一员，命中即拒绝删除。
///
/// 事故背景（参考项目 Mole 的 issue #1390）：Autodesk Fusion 的
/// `AcCoreConsole` 进程在它的 `Cache.db` 被 unlink 之后进入无界写循环，
/// 把整个卷写满。Mole 的修法是把这类判断放进删除入口本身
/// （`validate_path_for_deletion`）——任何删除调用方都绕不过，而不是散落
/// 在某条扫描规则的展示层判定里（本仓库此前正是后一种：`categories::
/// helpers::holds_live_database` 只影响默认勾选，用户手动勾上就直奔
/// `delete_tree`，没有任何拦截）。
///
/// 目录和文件走两条不同的证据链，宽严故意不对称：
/// - **目录**：顶层出现任意活库标记就整个拒绝（[`holds_live_database`]），
///   刻意宽松。`~/Library/Logs/OneDrive` 这种目标一旦被误判拒绝，代价只是
///   用户少清一个日志目录；换来的是绝不会把一个正在用的数据库连同它的
///   日志邻居一起端掉。
/// - **文件**：必须是 SQLite 家族成员，**且**能在同目录里凑出「主库 +
///   伴随文件」的组合（[`is_active_sqlite_member`]），刻意收紧。只按扩展
///   名判断会连缩略图缓存、iOS 备份的 `Manifest.db` 一起挡死。
///
/// **fail closed**：`symlink_metadata` 读不出来，一律当作「可能是活库」
/// 拒绝，不是放行——判不出不等于安全。
///
/// 只在 `cleaner::clean_path` 入口查一次，不在 `delete_tree` 的每一层递归
/// 里重复套用目录级的宽松判据——那会连坐 iOS 设备备份里每个子目录顶层都
/// 放着的 `Manifest.db`（同样没有 `-wal`/`-shm`），把整个 iOS 备份类目变成
/// 什么都删不掉。嵌套更深处的活库家族仍有兜底：`cleaner::delete_tree` 按
/// SQLite 家族分组删除时，会拒绝整组「主库 + 伴随文件同时存在」的文件（就
/// 是这里 [`is_active_sqlite_member`] 的同一份判据），且组内强制先删伴随
/// 文件、伴随文件删不掉就不动主库——双重防线，互不依赖单一检查点。
pub fn is_live_database(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Err(_) => true,
        Ok(md) => {
            let ft = md.file_type();
            if ft.is_symlink() {
                // 符号链接只是重定向，`delete_tree` 只删链接本身、不追进去，
                // 这里没有真正的数据库内容可判。
                false
            } else if ft.is_dir() {
                holds_live_database(path)
            } else {
                is_active_sqlite_member(path)
            }
        }
    }
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

/// 路径是否位于 `~/Library/Application Support` **之下**（不含根自身——
/// 根自身由 [`is_protected`] 的自身禁止档挡住）。
///
/// 磁盘透镜/右键删除的确认弹窗用它升级警示：Application Support 里装
/// 的是聊天记录、密码库、本地数据库这类不可重建的应用数据，和删一个
/// 缓存目录不是一个量级的事。
#[cfg(target_os = "macos")]
pub fn under_home_app_support(path: &Path) -> bool {
    let Some(home) = guards().home.as_deref() else {
        return false;
    };
    let root = format!("{home}\\library\\application support");
    let lower = norm(path);
    lower != root && at_or_under(&lower, &root)
}

#[cfg(not(target_os = "macos"))]
pub fn under_home_app_support(_path: &Path) -> bool {
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
    use std::path::PathBuf;

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

    /// macOS 自身禁止档：`~`、`~/Library`、`~/Library/Application Support`
    /// 的目录本身受保护，但内容不受影响——后者是旧版 IDE 数据与卸载
    /// 残留两个类目的工作前提。
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_home_skeletons_are_self_banned_but_contents_are_free() {
        let home = dirs::home_dir().expect("测试环境必须有 home");
        assert!(is_protected(&home));
        assert!(is_protected(&home.join("Library")));
        assert!(is_protected(&home.join("Library/Application Support")));

        // 内容照常可清
        assert!(!is_protected(&home.join("Library/Caches")));
        assert!(!is_protected(
            &home.join("Library/Application Support/JetBrains/IntelliJIdea2025.2")
        ));
        // 系统级 /Library 仍然整棵受保护（子树禁止档）
        assert!(is_protected(Path::new("/Library/Fonts")));
        // 大小写不敏感
        assert!(is_protected(&home.join("LIBRARY")));
    }

    /// 确认弹窗的升级判定：严格位于 Application Support 之下才算。
    #[cfg(target_os = "macos")]
    #[test]
    fn under_home_app_support_is_strict() {
        let home = dirs::home_dir().expect("测试环境必须有 home");
        let app_support = home.join("Library/Application Support");
        assert!(!under_home_app_support(&app_support), "根自身不算（它已被保护挡住）");
        assert!(under_home_app_support(
            &app_support.join("JetBrains/IntelliJIdea2025.2")
        ));
        assert!(!under_home_app_support(&home.join("Library/Caches")));
        assert!(!under_home_app_support(Path::new("/Applications/Safari.app")));
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

    /// `sqlite_family_key` 把主库和它的伴随文件折成同一个 key，大小写不
    /// 敏感——闭合 Mole 记录过的 `.DB-WAL` 大写变体绕过问题。
    #[test]
    fn sqlite_family_key_groups_companions_with_main() {
        assert_eq!(sqlite_family_key("Cache.db"), Some("cache.db".into()));
        assert_eq!(sqlite_family_key("Cache.db-wal"), Some("cache.db".into()));
        assert_eq!(sqlite_family_key("Cache.DB-SHM"), Some("cache.db".into()));
        assert_eq!(
            sqlite_family_key("cache.db-journal"),
            Some("cache.db".into())
        );
        assert_eq!(
            sqlite_family_key("telemetry.otc-wal"),
            Some("telemetry.otc".into())
        );
        assert_eq!(sqlite_family_key("notes.txt"), None);
    }

    fn temp_test_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("qc_safety_livedb_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 活库目录（顶层带 `-wal`）被拒删：即便用户手动勾选，删除入口也要挡住。
    #[test]
    fn live_database_directory_is_rejected() {
        let dir = temp_test_dir("dir_wal");
        std::fs::write(dir.join("syncReporterTelemetryCache.otc"), b"x").unwrap();
        std::fs::write(dir.join("syncReporterTelemetryCache.otc-wal"), b"x").unwrap();
        assert!(is_live_database(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `.db-wal` 的大写变体同样要被拒删——纯粹的大小写敏感匹配会漏掉它。
    #[test]
    fn live_database_file_gate_is_case_insensitive() {
        let dir = temp_test_dir("case_insensitive");
        let db = dir.join("Cache.DB");
        let wal = dir.join("Cache.DB-WAL");
        std::fs::write(&db, b"x").unwrap();
        std::fs::write(&wal, b"x").unwrap();
        assert!(is_live_database(&db), "大写主库应被拒删");
        assert!(is_live_database(&wal), "大写伴随文件应被拒删");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 没有伴随文件的孤立 `.db` 文件不能被拦——否则缩略图缓存、iOS 备份的
    /// `Manifest.db` 这类正常清理目标会被永久挡死。
    #[test]
    fn lone_db_file_without_companion_is_not_blocked() {
        let dir = temp_test_dir("lone_db");
        let lone = dir.join("thumbcache_1920.db");
        std::fs::write(&lone, b"x").unwrap();
        assert!(
            !is_live_database(&lone),
            "没有 -wal/-shm 伴随文件的 .db 不该被当成活库"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 读不到目录 / 拿不到 metadata 时一律 fail closed：判不出等于不安全，
    /// 不能悄悄放行删除。
    #[test]
    fn live_database_fails_closed_when_unreadable() {
        let missing = std::env::temp_dir().join("qc_safety_livedb_does_not_exist_ever");
        let _ = std::fs::remove_dir_all(&missing);
        assert!(
            is_live_database(&missing),
            "symlink_metadata 读不出来应当 fail closed"
        );
        assert!(
            holds_live_database(&missing),
            "read_dir 读不出来应当 fail closed"
        );
    }

    #[cfg(windows)]
    #[test]
    fn public_desktop_is_not_an_app_residual() {
        let public = std::env::var("PUBLIC").unwrap_or_else(|_| r"C:\Users\Public".into());
        let desktop = Path::new(&public).join("Desktop");
        assert!(
            is_protected(&desktop),
            "{} 是公共桌面，不能当残留删",
            desktop.display()
        );
        assert!(is_protected(Path::new(&public)));
        // 公共桌面上某软件自己的快捷方式目录仍可清
        assert!(!is_protected(&desktop.join("Remote Desktop Manager")));
    }
}
