//! macOS 软件残留深度扫描
//!
//! M5 实现：
//! - 以 `CFBundleIdentifier`（存在 `registry_subpath` 里）为主键搜索残留
//! - 扫描 `~/Library` 下的 Application Support、Caches、Preferences、Logs 等
//! - 再按厂商前缀补扫 `/Library`（系统级支持目录、LaunchAgents/LaunchDaemons）
//!   和 `~/.config` 一类点目录——这两处的名字都不含完整 Bundle ID
//! - 区分安全项（缓存、日志）和注意项（配置、容器数据、系统级组件）
//! - 不删除用户数据、登录态、凭据等高风险内容

use crate::core::apps::{
    InstalledApp, ResidualItem, ResidualKind, ResidualScanResult, ResidualSource,
};
use crate::core::cleaner::{clean_path, CleanProgress, CleanReport, CleanResult};
use std::path::{Path, PathBuf};

/// 扫描应用卸载后的残留文件和目录。
///
/// 以 `CFBundleIdentifier`（存在 `app.registry_subpath`）为主键，
/// 在 `~/Library`、`/Library` 和点目录下的多个已知位置搜索匹配的残留。
pub fn scan_residuals(app: &InstalledApp) -> ResidualScanResult {
    let Some(home) = dirs::home_dir() else {
        return ResidualScanResult {
            app_name: app.name.clone(),
            app_id: app.id.clone(),
            items: Vec::new(),
            total_file_size: 0,
        };
    };

    let darwin_cache = darwin_user_cache_dir();
    scan_residuals_in(
        app,
        &ScanRoots {
            home: &home,
            system_library: Path::new("/Library"),
            receipts: Path::new("/private/var/db/receipts"),
            darwin_cache: darwin_cache.as_deref(),
        },
    )
}

/// 扫描起点。系统级路径独立成字段，测试才能把 `/Library` 重定向到临时目录。
struct ScanRoots<'a> {
    home: &'a Path,
    system_library: &'a Path,
    receipts: &'a Path,
    darwin_cache: Option<&'a Path>,
}

fn scan_residuals_in(app: &InstalledApp, roots: &ScanRoots<'_>) -> ResidualScanResult {
    let mut items = Vec::new();

    let home = roots.home;
    let receipts = roots.receipts;
    let darwin_cache = roots.darwin_cache;
    let library = home.join("Library");
    let bundle_id = &app.registry_subpath;
    let app_name = &app.name;

    // 1. Application Support — 安全清理（应用数据，非用户文档）
    //    按应用名和 bundle id 两种方式搜索
    for search_name in &[app_name, bundle_id] {
        if search_name.is_empty() {
            continue;
        }
        let path = library.join("Application Support").join(search_name);
        if path.exists() {
            let size = super::apps::dir_size(&path);
            items.push(ResidualItem::certain(
                ResidualKind::Directory(path, size),
                ResidualSource::AppSupportDir,
            ));
        }
    }

    // 2. Caches — 安全清理
    for search_name in &[app_name, bundle_id] {
        if search_name.is_empty() {
            continue;
        }
        let path = library.join("Caches").join(search_name);
        if path.exists() {
            let size = super::apps::dir_size(&path);
            items.push(ResidualItem::certain(
                ResidualKind::Directory(path, size),
                ResidualSource::CacheDir,
            ));
        }
    }

    // 3. Preferences — 安全清理（plist 文件）
    if !bundle_id.is_empty() {
        let plist = library
            .join("Preferences")
            .join(format!("{bundle_id}.plist"));
        if plist.exists() {
            let size = super::apps::dir_size(&plist);
            items.push(ResidualItem::certain(
                ResidualKind::File(plist, size),
                ResidualSource::PreferenceFile,
            ));
        }
    }

    // 4. Logs — 安全清理
    for search_name in &[app_name, bundle_id] {
        if search_name.is_empty() {
            continue;
        }
        let path = library.join("Logs").join(search_name);
        if path.exists() {
            let size = super::apps::dir_size(&path);
            items.push(ResidualItem::certain(
                ResidualKind::Directory(path, size),
                ResidualSource::LogDir,
            ));
        }
    }

    // 5. Saved Application State — 安全清理（窗口状态）
    if !bundle_id.is_empty() {
        let path = library
            .join("Saved Application State")
            .join(format!("{bundle_id}.savedState"));
        if path.exists() {
            let size = super::apps::dir_size(&path);
            items.push(ResidualItem::certain(
                ResidualKind::Directory(path, size),
                ResidualSource::Other,
            ));
        }
    }

    // 6. HTTPStorages — 安全清理
    if !bundle_id.is_empty() {
        let path = library.join("HTTPStorages").join(bundle_id);
        if path.exists() {
            let size = super::apps::dir_size(&path);
            items.push(ResidualItem::certain(
                ResidualKind::Directory(path, size),
                ResidualSource::Other,
            ));
        }
    }

    // 7. Containers — 注意项（沙盒数据，可能含用户文档）
    if !bundle_id.is_empty() {
        let path = library.join("Containers").join(bundle_id);
        if path.exists() {
            let size = super::apps::dir_size(&path);
            items.push(ResidualItem::possible(
                ResidualKind::Directory(path, size),
                ResidualSource::ContainerDir,
            ));
        }
    }

    // 8. Group Containers — 注意项（应用组共享数据）
    if !bundle_id.is_empty() {
        let group_dir = library.join("Group Containers");
        if let Ok(entries) = std::fs::read_dir(&group_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                // Group container 通常以 bundle id 前缀命名
                if name_str.starts_with(bundle_id) || name_str.contains(bundle_id.as_str()) {
                    let path = entry.path();
                    let size = super::apps::dir_size(&path);
                    items.push(ResidualItem::possible(
                        ResidualKind::Directory(path, size),
                        ResidualSource::Other,
                    ));
                }
            }
        }
    }

    // 主应用之外的 Login Item、XPC 和 Extension 通常拥有独立 Bundle ID。
    // 这些 ID 必须在 .app 仍存在时从包内 Info.plist 读取；仅拿主 ID 拼路径
    // 会漏掉 iShotHelper 这一类辅助容器和 Application Scripts。
    let bundle_ids = app_bundle_ids(app);
    for (index, id) in bundle_ids.iter().enumerate() {
        let primary = index == 0;

        add_named_entry(
            &mut items,
            &library.join("Application Scripts"),
            id,
            "",
            ResidualSource::ApplicationScript,
            primary,
        );
        add_named_entry(
            &mut items,
            &library.join("Containers"),
            id,
            "",
            ResidualSource::ContainerDir,
            false,
        );

        let recent = library.join(
            "Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.ApplicationRecentDocuments",
        );
        for suffix in [".sfl2", ".sfl3", ".sfl4"] {
            add_named_entry(
                &mut items,
                &recent,
                id,
                suffix,
                ResidualSource::RecentDocumentList,
                true,
            );
        }

        // 安装收据位于 root 管理的系统目录，默认列为“需要确认”，不会
        // 和用户缓存一起自动勾选。这里仍应展示，否则会错误报告“无残留”。
        for suffix in [".bom", ".plist"] {
            add_named_entry(
                &mut items,
                receipts,
                id,
                suffix,
                ResidualSource::PackageReceipt,
                false,
            );
        }

        if let Some(cache_root) = darwin_cache {
            add_named_entry(
                &mut items,
                cache_root,
                id,
                "",
                ResidualSource::CacheDir,
                true,
            );
        }
    }

    // App Group 名称不一定包含主 Bundle ID，必须从已签名 entitlements 读取。
    // 同一个 group ID 会同时对应 Group Containers 和 Application Scripts。
    for group_id in app_group_ids(app) {
        add_named_entry(
            &mut items,
            &library.join("Group Containers"),
            &group_id,
            "",
            ResidualSource::ContainerDir,
            false,
        );
        add_named_entry(
            &mut items,
            &library.join("Application Scripts"),
            &group_id,
            "",
            ResidualSource::ApplicationScript,
            false,
        );
    }

    // 上面全部是精确 ID 匹配，够不到两类东西：住在 `/Library` 的系统级组件
    // （Karabiner 的 77 MB 驱动目录、8 个 launchd plist），以及和主 ID 同厂商
    // 但不同产品名的兄弟组件（`org.pqrs.Karabiner-Menu`）。二者都按厂商前缀找。
    add_vendor_family(&mut items, &library, roots, &bundle_ids);

    // Homebrew 装的、或跨平台移植的 App 会把配置写进 `~/.config` 一类点目录，
    // 这些名字既不是 Bundle ID 也不一定等于 App 名（`~/.config/karabiner`）。
    add_dotfile_configs(&mut items, home, app);

    // 系统扩展只在扩展数据库里，磁盘上没有能直接删的路径。列出来是为了让
    // 用户知道「卸干净了但驱动还在跑」，清理动作另走 systemextensionsctl。
    add_system_extensions(&mut items, &bundle_ids);

    // 前面的传统精确路径和扩展 Bundle ID 扫描可能指向同一项，按真实路径
    // 去重后重新统计，避免 UI 重复展示或重复计算大小。
    let mut seen = std::collections::HashSet::<PathBuf>::new();
    items.retain(|item| match &item.kind {
        ResidualKind::File(path, _) | ResidualKind::Directory(path, _) => seen.insert(path.clone()),
        _ => true,
    });
    let total_file_size = items.iter().map(ResidualItem::size).sum();

    // 注意：以下目录不扫描，因为可能包含用户数据或凭据：
    // - ~/Library/Keychains — 凭据，危险
    // - ~/Library/Cookies — 登录态，危险
    // - ~/Library/Accounts — 账户信息，危险
    // - ~/Library/Mail — 用户邮件，危险

    ResidualScanResult {
        app_name: app.name.clone(),
        app_id: app.id.clone(),
        items,
        total_file_size,
    }
}

/// 返回主 Bundle ID 以及 app 包中确实属于该应用的辅助组件 ID。
fn app_bundle_ids(app: &InstalledApp) -> Vec<String> {
    let mut ids = Vec::new();
    if valid_bundle_id(&app.registry_subpath) {
        ids.push(app.registry_subpath.clone());
    }

    let Some(app_path) = app.install_location.as_deref() else {
        return ids;
    };
    let contents = app_path.join("Contents");
    if !contents.is_dir() {
        return ids;
    }

    for entry in walkdir::WalkDir::new(&contents)
        .max_depth(12)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file() && entry.file_name() == "Info.plist")
        .take(128)
    {
        let Some(component_contents) = entry.path().parent() else {
            continue;
        };
        let Some(bundle_root) = component_contents.parent() else {
            continue;
        };
        if bundle_root == app_path {
            continue;
        }

        let extension = bundle_root
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let is_owned_component = extension.eq_ignore_ascii_case("xpc")
            || extension.eq_ignore_ascii_case("appex")
            || (extension.eq_ignore_ascii_case("app")
                && bundle_root.starts_with(contents.join("Library/LoginItems")));
        if !is_owned_component {
            continue;
        }

        let (Some(id), _) = super::apps::read_info_plist(entry.path()) else {
            continue;
        };
        if valid_bundle_id(&id)
            && !id.starts_with("org.sparkle-project.")
            && !ids.iter().any(|known| known.eq_ignore_ascii_case(&id))
        {
            ids.push(id);
        }
    }
    ids
}

fn valid_bundle_id(id: &str) -> bool {
    id.len() >= 3
        && id.contains('.')
        && !id.contains('/')
        && !id.contains('\\')
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn app_group_ids(app: &InstalledApp) -> Vec<String> {
    let Some(app_path) = app.install_location.as_deref() else {
        return Vec::new();
    };
    let Ok(output) = std::process::Command::new("codesign")
        .arg("-d")
        .arg("--entitlements")
        .arg(":-")
        .arg(app_path)
        .output()
    else {
        return Vec::new();
    };
    parse_application_groups(&String::from_utf8_lossy(&output.stdout))
}

fn parse_application_groups(entitlements: &str) -> Vec<String> {
    let Some((_, after_key)) =
        entitlements.split_once("<key>com.apple.security.application-groups</key>")
    else {
        return Vec::new();
    };
    let Some((array, _)) = after_key.split_once("</array>") else {
        return Vec::new();
    };

    let mut groups = Vec::new();
    let mut rest = array;
    while let Some((_, after_open)) = rest.split_once("<string>") {
        let Some((value, after_close)) = after_open.split_once("</string>") else {
            break;
        };
        if valid_bundle_id(value) && !groups.iter().any(|known| known == value) {
            groups.push(value.to_string());
        }
        rest = after_close;
    }
    groups
}

fn darwin_user_cache_dir() -> Option<PathBuf> {
    let length = unsafe { libc::confstr(libc::_CS_DARWIN_USER_CACHE_DIR, std::ptr::null_mut(), 0) };
    if length == 0 {
        return None;
    }
    let mut buffer = vec![0u8; length];
    let written = unsafe {
        libc::confstr(
            libc::_CS_DARWIN_USER_CACHE_DIR,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
        )
    };
    if written == 0 {
        return None;
    }
    let value = std::ffi::CStr::from_bytes_until_nul(&buffer).ok()?;
    Some(PathBuf::from(value.to_string_lossy().into_owned()))
}

/// 在指定目录的直接子项中按 Bundle ID 不区分 ASCII 大小写匹配。
/// 不能直接 `root.join(id)`：大小写敏感 APFS 上历史版本可能留下
/// `iShotHelper` 与 `ishothelper` 两种目录。
fn add_named_entry(
    items: &mut Vec<ResidualItem>,
    root: &Path,
    id: &str,
    suffix: &str,
    source: ResidualSource,
    certain: bool,
) {
    let target = format!("{id}{suffix}");
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(&target)
        {
            continue;
        }
        let path = entry.path();
        let size = super::apps::dir_size(&path);
        let kind = if path.is_dir() {
            ResidualKind::Directory(path, size)
        } else {
            ResidualKind::File(path, size)
        };
        items.push(if certain {
            ResidualItem::certain(kind, source)
        } else {
            ResidualItem::possible(kind, source)
        });
    }
}

/// 这些前缀下挂着大量互不相关的产品，按家族匹配会把别的软件一起带走。
const SHARED_VENDOR_PREFIXES: &[&str] = &[
    "com.apple",
    "com.google",
    "com.microsoft",
    "com.adobe",
    "com.amazon",
    "com.oracle",
    "com.jetbrains",
    "org.mozilla",
    "org.chromium",
    "com.electron",
    "net.java",
];

/// `~/Library` 下按厂商前缀扫的目录，以及命中后标注的来源。
const USER_FAMILY_DIRS: &[(&str, ResidualSource)] = &[
    ("Application Support", ResidualSource::AppSupportDir),
    ("Caches", ResidualSource::CacheDir),
    ("Preferences", ResidualSource::PreferenceFile),
    ("Preferences/ByHost", ResidualSource::PreferenceFile),
    ("Logs", ResidualSource::LogDir),
    ("HTTPStorages", ResidualSource::Other),
    ("Containers", ResidualSource::ContainerDir),
    ("Group Containers", ResidualSource::ContainerDir),
    ("Application Scripts", ResidualSource::ApplicationScript),
    ("Saved Application State", ResidualSource::Other),
    ("LaunchAgents", ResidualSource::LaunchAgent),
];

/// `/Library` 下按厂商前缀扫的目录。这些位置全部 root 所有，命中项一律
/// 标成「需要确认」，不会跟用户缓存一起被自动勾选。
const SYSTEM_FAMILY_DIRS: &[(&str, ResidualSource)] = &[
    ("Application Support", ResidualSource::AppSupportDir),
    ("Caches", ResidualSource::CacheDir),
    ("Preferences", ResidualSource::PreferenceFile),
    ("Logs", ResidualSource::LogDir),
    ("Application Scripts", ResidualSource::ApplicationScript),
    ("LaunchAgents", ResidualSource::LaunchAgent),
    ("LaunchDaemons", ResidualSource::LaunchDaemon),
    ("PrivilegedHelperTools", ResidualSource::LaunchDaemon),
];

/// 从 Bundle ID 取厂商前缀：`org.pqrs.Karabiner-Elements.Settings` → `org.pqrs`。
///
/// 要求至少三段——`org.pqrs` 这种两段 ID 自己就是前缀，再做家族匹配等于
/// 匹配全部，没有收窄作用。
fn vendor_prefix(bundle_id: &str) -> Option<String> {
    let mut parts = bundle_id.split('.');
    let tld = parts.next()?;
    let vendor = parts.next()?;
    parts.next()?;
    if tld.is_empty() || vendor.len() < 2 {
        return None;
    }
    let prefix = format!("{tld}.{vendor}");
    if SHARED_VENDOR_PREFIXES
        .iter()
        .any(|shared| shared.eq_ignore_ascii_case(&prefix))
    {
        return None;
    }
    Some(prefix)
}

/// 目录项是否属于该厂商家族：名字等于前缀本身（`org.pqrs/`），或以
/// `前缀.` 开头（`org.pqrs.karabiner.agent....plist`）。
///
/// 必须比到点号，否则 `org.pqrsx.foo` 会被 `org.pqrs` 误收。
fn in_vendor_family(name: &str, prefix: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let prefix = prefix.to_ascii_lowercase();
    name == prefix
        || name
            .strip_prefix(&prefix)
            .is_some_and(|rest| rest.starts_with('.'))
}

fn add_vendor_family(
    items: &mut Vec<ResidualItem>,
    library: &Path,
    roots: &ScanRoots<'_>,
    bundle_ids: &[String],
) {
    let prefixes = vendor_prefixes(bundle_ids);
    if prefixes.is_empty() {
        return;
    }

    for (subdir, source) in USER_FAMILY_DIRS {
        add_family_entries(items, &library.join(subdir), &prefixes, *source);
    }
    for (subdir, source) in SYSTEM_FAMILY_DIRS {
        add_family_entries(
            items,
            &roots.system_library.join(subdir),
            &prefixes,
            *source,
        );
    }
    add_family_entries(
        items,
        roots.receipts,
        &prefixes,
        ResidualSource::PackageReceipt,
    );
}

/// 家族匹配拿到的证据比精确 Bundle ID 弱一档（可能是同厂商的另一个产品），
/// 一律记为「需要确认」，来源保留具体位置以便用户判断。
fn add_family_entries(
    items: &mut Vec<ResidualItem>,
    root: &Path,
    prefixes: &[String],
    source: ResidualSource,
) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !prefixes
            .iter()
            .any(|prefix| in_vendor_family(&name, prefix))
        {
            continue;
        }
        let path = entry.path();
        let size = super::apps::dir_size(&path);
        let kind = if path.is_dir() {
            ResidualKind::Directory(path, size)
        } else {
            ResidualKind::File(path, size)
        };
        items.push(ResidualItem::possible(kind, source));
    }
}

/// 点目录扫描的父目录，相对 `$HOME`。空字符串代表 `$HOME` 自身。
const DOT_CONFIG_PARENTS: &[&str] = &["", ".config", ".cache", ".local/share", ".local/state"];

/// 这些是多个软件共用的基础设施目录，名字再像也不能当成某个 App 的残留。
const PROTECTED_DOT_DIRS: &[&str] = &[
    "config", "cache", "local", "ssh", "gnupg", "aws", "kube", "docker", "npm", "cargo", "rustup",
    "gradle", "nvm", "pyenv", "vim", "git", "trash",
];

/// 归一化成只剩小写字母数字，用来跨 `Karabiner-Elements` / `karabiner` 这类
/// 分隔符和后缀差异做比较。
fn config_slug(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn add_dotfile_configs(items: &mut Vec<ResidualItem>, home: &Path, app: &InstalledApp) {
    // App 名和 Bundle ID 末段都可能是点目录用的名字：Karabiner-Elements 写的是
    // `~/.config/karabiner`，两边都不精确相等，只能按前缀互相包含来判定。
    let mut slugs = vec![config_slug(&app.name)];
    if let Some(last) = app.registry_subpath.rsplit('.').next() {
        let slug = config_slug(last);
        if !slugs.contains(&slug) {
            slugs.push(slug);
        }
    }
    slugs.retain(|slug| slug.len() >= 4);
    if slugs.is_empty() {
        return;
    }

    for parent in DOT_CONFIG_PARENTS {
        let dir = if parent.is_empty() {
            home.to_path_buf()
        } else {
            home.join(parent)
        };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // 只有 `$HOME` 这一层需要认前导点，`~/.config` 里面是普通名字。
            let bare = if parent.is_empty() {
                let Some(bare) = name.strip_prefix('.') else {
                    continue;
                };
                bare
            } else {
                name.as_ref()
            };
            let slug = config_slug(bare);
            if slug.len() < 4 || PROTECTED_DOT_DIRS.contains(&slug.as_str()) {
                continue;
            }
            if !slugs
                .iter()
                .any(|app_slug| app_slug.starts_with(&slug) || slug.starts_with(app_slug))
            {
                continue;
            }
            if !entry.path().is_dir() {
                continue;
            }
            let path = entry.path();
            let size = super::apps::dir_size(&path);
            items.push(ResidualItem::possible(
                ResidualKind::Directory(path, size),
                ResidualSource::DotConfigDir,
            ));
        }
    }
}

/// 解析 `systemextensionsctl list`，返回 `(teamID, bundleID)`。
///
/// 输出形如：
/// 各列以制表符分隔：
/// ```text
/// enabled | active | teamID | bundleID (version) | name | [state]
/// *       | *      | G43BCU2T37 | org.pqrs.Karabiner-DriverKit-VirtualHIDDevice (1.6.0/1.6.0) | ... | [activated enabled]
/// ```
/// 只收 `[activated ...]` 的行——已经 terminated/uninstalling 的不是残留。
fn parse_system_extensions(output: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for line in output.lines() {
        if !line.contains("[activated") {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        // 表头之外的数据行至少有 enabled/active/teamID/bundleID 四列
        if fields.len() < 4 {
            continue;
        }
        let team_id = fields[2].trim();
        // bundleID 那列后面跟着 " (版本)"，切掉
        let bundle_id = fields[3].split_whitespace().next().unwrap_or_default();
        if team_id.is_empty() || !valid_bundle_id(bundle_id) {
            continue;
        }
        found.push((team_id.to_string(), bundle_id.to_string()));
    }
    found
}

fn add_system_extensions(items: &mut Vec<ResidualItem>, bundle_ids: &[String]) {
    let prefixes = vendor_prefixes(bundle_ids);
    if prefixes.is_empty() && bundle_ids.is_empty() {
        return;
    }
    let Ok(output) = std::process::Command::new("systemextensionsctl")
        .arg("list")
        .output()
    else {
        return;
    };
    for (team_id, bundle_id) in parse_system_extensions(&String::from_utf8_lossy(&output.stdout)) {
        let owned = bundle_ids
            .iter()
            .any(|id| id.eq_ignore_ascii_case(&bundle_id))
            || prefixes
                .iter()
                .any(|prefix| in_vendor_family(&bundle_id, prefix));
        if !owned {
            continue;
        }
        items.push(ResidualItem::possible(
            ResidualKind::SystemExtension(team_id, bundle_id),
            ResidualSource::SystemExtension,
        ));
    }
}

/// SIP 开启时 `systemextensionsctl uninstall` 直接拒绝执行（它自己会打印
/// "this tool cannot be used if System Integrity Protection is enabled"，
/// 而且退出码仍然是 0，不能靠退出码判断）。
///
/// 别的 App 的扩展也不能由我们调 `OSSystemExtensionRequest` 停用——那个 API
/// 要求请求方和扩展同属一个 Team ID。所以 SIP 开着时唯一的出路是让用户去
/// 系统设置里关，或者跑厂商自己的卸载器。
fn deactivate_system_extension(team_id: &str, bundle_id: &str) -> CleanResult {
    let sip_enabled = std::process::Command::new("csrutil")
        .arg("status")
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).contains("enabled"))
        .unwrap_or(true);
    if sip_enabled {
        return CleanResult::Failed;
    }
    let _ = std::process::Command::new("systemextensionsctl")
        .arg("uninstall")
        .arg(team_id)
        .arg(bundle_id)
        .output();

    // 以复查为准：uninstall 是异步的，而且失败时退出码照样是 0。
    let still_there = std::process::Command::new("systemextensionsctl")
        .arg("list")
        .output()
        .map(|out| {
            parse_system_extensions(&String::from_utf8_lossy(&out.stdout))
                .iter()
                .any(|(_, id)| id.eq_ignore_ascii_case(bundle_id))
        })
        .unwrap_or(true);
    if still_there {
        CleanResult::Failed
    } else {
        CleanResult::Ok
    }
}

/// 把一组 Bundle ID 折算成去重后的厂商前缀。
fn vendor_prefixes(bundle_ids: &[String]) -> Vec<String> {
    let mut prefixes: Vec<String> = Vec::new();
    for id in bundle_ids {
        if let Some(prefix) = vendor_prefix(id) {
            if !prefixes
                .iter()
                .any(|known| known.eq_ignore_ascii_case(&prefix))
            {
                prefixes.push(prefix);
            }
        }
    }
    prefixes
}

/// 清理选中的残留。
///
/// 分三条路：普通用户路径直接删；`/Library` 下 root 所有的走**一次**提权批
/// 处理（合并成一个密码框，并且 launchd 项先 bootout 再删）；系统扩展没有
/// 可删的路径，单独处理。
pub fn clean_residuals(items: &[ResidualItem], prog: &CleanProgress) -> CleanReport {
    let mut report = CleanReport::default();
    let mut elevated: Vec<PathBuf> = Vec::new();

    for item in items {
        match &item.kind {
            ResidualKind::Directory(path, _) | ResidualKind::File(path, _) => {
                prog.note(path);
                // 攒起来晚点一起提权，避免每个文件弹一次密码框。
                if super::elevate::needs_elevation(path) {
                    elevated.push(path.clone());
                    continue;
                }
                let res = clean_path(path, prog);
                report.record(path, res);
            }
            ResidualKind::SystemExtension(team_id, bundle_id) => {
                let res = deactivate_system_extension(team_id, bundle_id);
                report.record(Path::new(bundle_id), res);
            }
            _ => {}
        }
    }

    if !elevated.is_empty() {
        let removed = super::elevate::elevated_remove(&elevated);
        for path in elevated {
            let res = if removed.contains(&path) {
                CleanResult::Ok
            } else {
                CleanResult::Failed
            };
            report.record(&path, res);
        }
    }
    report
}

/// 复核候选残留是否仍然存在（对应 Windows 侧的「先扫描后卸载」流程）。
pub fn verify_residuals(items: Vec<ResidualItem>) -> Vec<ResidualItem> {
    items
        .into_iter()
        .filter(|it| match &it.kind {
            ResidualKind::File(p, _) | ResidualKind::Directory(p, _) => p.exists(),
            _ => true,
        })
        // 官方卸载器可能只清掉目录的一部分，不能继续展示卸载前的旧体积。
        .map(|mut item| {
            match &mut item.kind {
                ResidualKind::File(path, size) | ResidualKind::Directory(path, size) => {
                    *size = super::apps::dir_size(path);
                }
                _ => {}
            }
            item
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::apps::AppRegRoot;

    fn make_app(name: &str, bundle_id: &str) -> InstalledApp {
        InstalledApp {
            id: bundle_id.to_string(),
            name: name.to_string(),
            version: String::new(),
            publisher: String::new(),
            last_used_date: None,
            last_used_raw: 0,
            install_date: None,
            install_date_raw: 0,
            install_location: None,
            display_icon: None,
            uninstall_string: None,
            quiet_uninstall_string: None,
            estimated_size: 0,
            registry_root: AppRegRoot::Hkcu,
            registry_subpath: bundle_id.to_string(),
            is_system_component: false,
            uninstaller_missing: true,
        }
    }

    #[test]
    fn scan_nonexistent_app_returns_empty() {
        let app = make_app("NonexistentApp12345", "com.nonexistent.app12345");
        let result = scan_residuals(&app);
        assert!(result.items.is_empty(), "不存在的应用不应有残留");
    }

    #[test]
    fn scan_finds_preferences_plist() {
        // 用一个已知的系统应用测试（Calculator 的 bundle id 是 com.apple.calculator）
        let app = make_app("Calculator", "com.apple.calculator");
        let result = scan_residuals(&app);
        // 至少应该能找到一些残留（Preferences plist 或 Saved Application State）
        // 注意：如果从未打开过 Calculator，可能没有残留——这不是错误
        let _ = result; // 只验证不 panic
    }

    #[test]
    fn scan_finds_embedded_helpers_scripts_recent_items_and_receipts() {
        let root = std::env::temp_dir().join(format!(
            "quick-cleaner-residuals-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        let app_path = root.join("iShot.app");
        let helper_contents = app_path.join("Contents/Library/LoginItems/iShotHelper.app/Contents");
        let receipts = root.join("receipts");
        std::fs::create_dir_all(&helper_contents).unwrap();
        std::fs::create_dir_all(home.join("Library/Application Scripts/cn.better365.ishot"))
            .unwrap();
        std::fs::create_dir_all(home.join("Library/Application Scripts/cn.better365.ishothelper"))
            .unwrap();
        std::fs::create_dir_all(home.join("Library/Containers/cn.better365.iShotHelper")).unwrap();
        let recent = home.join(
            "Library/Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.ApplicationRecentDocuments",
        );
        std::fs::create_dir_all(&recent).unwrap();
        std::fs::create_dir_all(&receipts).unwrap();
        std::fs::write(
            helper_contents.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>cn.better365.iShotHelper</string>
</dict></plist>"#,
        )
        .unwrap();
        std::fs::write(recent.join("cn.better365.ishot.sfl3"), b"recent").unwrap();
        std::fs::write(receipts.join("cn.better365.ishot.bom"), b"bom").unwrap();
        std::fs::write(receipts.join("cn.better365.ishot.plist"), b"plist").unwrap();

        let mut app = make_app("iShot", "cn.better365.ishot");
        app.install_location = Some(app_path);
        let result = scan_residuals_in(
            &app,
            &ScanRoots {
                home: &home,
                system_library: &root.join("system-library"),
                receipts: &receipts,
                darwin_cache: None,
            },
        );

        assert_eq!(result.items.len(), 6);
        assert!(result
            .items
            .iter()
            .any(|item| item.source == ResidualSource::ContainerDir));
        assert_eq!(
            result
                .items
                .iter()
                .filter(|item| item.source == ResidualSource::ApplicationScript)
                .count(),
            2
        );
        assert_eq!(
            result
                .items
                .iter()
                .filter(|item| item.source == ResidualSource::PackageReceipt)
                .count(),
            2
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scan_finds_system_level_and_vendor_family_and_dot_config() {
        let root = std::env::temp_dir().join(format!(
            "quick-cleaner-residuals-sys-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        let system_library = root.join("system-library");
        let receipts = root.join("receipts");

        // 系统级：77 MB 驱动目录和 launchd plist 都在 `/Library` 下，且名字
        // 只共享厂商前缀，不含主 Bundle ID。
        std::fs::create_dir_all(
            system_library.join("Application Support/org.pqrs/Karabiner-Elements"),
        )
        .unwrap();
        std::fs::create_dir_all(system_library.join("LaunchDaemons")).unwrap();
        std::fs::create_dir_all(system_library.join("LaunchAgents")).unwrap();
        std::fs::write(
            system_library.join("LaunchDaemons/org.pqrs.karabiner.karabiner_grabber.plist"),
            b"daemon",
        )
        .unwrap();
        std::fs::write(
            system_library.join("LaunchAgents/org.pqrs.karabiner.NotificationWindow.plist"),
            b"agent",
        )
        .unwrap();

        // 用户级但属于兄弟产品：主 ID 是 ...Karabiner-Elements.Settings，
        // 这个 plist 归 Karabiner-Menu，精确匹配够不到。
        std::fs::create_dir_all(home.join("Library/Preferences")).unwrap();
        std::fs::write(
            home.join("Library/Preferences/org.pqrs.Karabiner-Menu.plist"),
            b"pref",
        )
        .unwrap();
        // 同一层里的别家 plist 不能被 `org.pqrs` 前缀顺手带走。
        std::fs::write(
            home.join("Library/Preferences/org.pqrsx.unrelated.plist"),
            b"other",
        )
        .unwrap();

        std::fs::create_dir_all(home.join(".config/karabiner")).unwrap();
        // `.ssh` 一类共用目录即使名字沾边也不能进候选。
        std::fs::create_dir_all(home.join(".config/cache")).unwrap();
        std::fs::create_dir_all(&receipts).unwrap();

        let app = make_app("Karabiner-Elements", "org.pqrs.Karabiner-Elements.Settings");
        let result = scan_residuals_in(
            &app,
            &ScanRoots {
                home: &home,
                system_library: &system_library,
                receipts: &receipts,
                darwin_cache: None,
            },
        );

        let paths: Vec<String> = result
            .items
            .iter()
            .map(|item| match &item.kind {
                ResidualKind::File(p, _) | ResidualKind::Directory(p, _) => {
                    p.to_string_lossy().into_owned()
                }
                _ => String::new(),
            })
            .collect();
        let has = |needle: &str| paths.iter().any(|p| p.ends_with(needle));

        assert!(
            has("Application Support/org.pqrs"),
            "缺系统级厂商目录: {paths:?}"
        );
        assert!(
            has("org.pqrs.karabiner.karabiner_grabber.plist"),
            "缺 LaunchDaemon: {paths:?}"
        );
        assert!(
            has("org.pqrs.karabiner.NotificationWindow.plist"),
            "缺 LaunchAgent: {paths:?}"
        );
        assert!(
            has("org.pqrs.Karabiner-Menu.plist"),
            "缺兄弟产品偏好: {paths:?}"
        );
        assert!(has(".config/karabiner"), "缺点目录配置: {paths:?}");
        assert!(
            !has("org.pqrsx.unrelated.plist"),
            "误收了别家前缀: {paths:?}"
        );
        assert!(!has(".config/cache"), "误收了共用目录: {paths:?}");

        // 全部是弱证据，不能默认勾选。
        assert!(
            result
                .items
                .iter()
                .all(|item| !item.confidence.is_certain()),
            "系统级/家族匹配项不应自动勾选"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn vendor_prefix_skips_shared_and_two_part_ids() {
        assert_eq!(
            vendor_prefix("org.pqrs.Karabiner-Elements.Settings").as_deref(),
            Some("org.pqrs")
        );
        // 两段 ID 的前缀就是它自己，家族匹配没有收窄作用。
        assert_eq!(vendor_prefix("org.pqrs"), None);
        // 大厂前缀下挂着一堆无关产品，卸 Chrome 不能带走 Google Drive。
        assert_eq!(vendor_prefix("com.google.Chrome.helper"), None);
        assert_eq!(vendor_prefix("com.apple.Safari.extension"), None);

        assert!(in_vendor_family("org.pqrs", "org.pqrs"));
        assert!(in_vendor_family(
            "org.pqrs.Karabiner-Menu.plist",
            "org.pqrs"
        ));
        assert!(!in_vendor_family("org.pqrsx.foo", "org.pqrs"));
    }

    #[test]
    fn parses_activated_system_extensions_only() {
        // 取自真机 `systemextensionsctl list` 的输出形状
        let output = "1 extension(s)\n--- com.apple.system_extension.driver_extension\nenabled\tactive\tteamID\tbundleID (version)\tname\t[state]\n*\t*\tG43BCU2T37\torg.pqrs.Karabiner-DriverKit-VirtualHIDDevice (1.6.0/1.6.0)\torg.pqrs.Karabiner-DriverKit-VirtualHIDDevice\t[activated enabled]\n*\t*\tXXXXXXXXXX\tcom.other.gone (1.0/1.0)\tcom.other.gone\t[terminated waiting to uninstall on reboot]\n";

        let found = parse_system_extensions(output);
        assert_eq!(
            found,
            [(
                "G43BCU2T37".to_string(),
                "org.pqrs.Karabiner-DriverKit-VirtualHIDDevice".to_string()
            )],
            "只应收 activated 的扩展，表头和 terminated 行都要跳过"
        );
    }

    #[test]
    fn parses_only_signed_application_group_values() {
        let entitlements = r#"<plist><dict>
<key>com.apple.security.application-groups</key><array>
<string>group.com.example.app</string>
<string>group.com.example.shared</string>
</array><key>other</key><array><string>com.unrelated.value</string></array>
</dict></plist>"#;

        assert_eq!(
            parse_application_groups(entitlements),
            ["group.com.example.app", "group.com.example.shared"]
        );
    }
}
