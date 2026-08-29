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
    is_safe_app_token, InstalledApp, ResidualItem, ResidualKind, ResidualOccupancy,
    ResidualScanResult, ResidualSource,
};
use crate::core::cleaner::{
    dispose, CleanFailure, CleanProgress, CleanReport, CleanResult, Disposal,
};
use crate::core::proc::run_with_timeout;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 扫描应用卸载后的残留文件和目录。
///
/// 以 `CFBundleIdentifier`（存在 `app.registry_subpath`）为主键，
/// 在 `~/Library`、`/Library` 和点目录下的多个已知位置搜索匹配的残留。
pub fn scan_residuals(app: &InstalledApp) -> ResidualScanResult {
    let Some(home) = super::user_env::user_home() else {
        return ResidualScanResult {
            app_name: app.name.clone(),
            app_id: app.id.clone(),
            ..Default::default()
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
            applications: Path::new("/Applications"),
        },
    )
}

/// 扫描起点。系统级路径独立成字段，测试才能把 `/Library` 重定向到临时目录。
struct ScanRoots<'a> {
    home: &'a Path,
    system_library: &'a Path,
    receipts: &'a Path,
    darwin_cache: Option<&'a Path>,
    /// 真·已安装应用目录（`/Applications`）。点目录归属判定要拿「还装着
    /// 的别家软件」排除误收，测试必须能把它重定向成空目录，否则在这台
    /// 机器上真装了同厂商软件时（如 Karabiner-Elements）测试就不密闭。
    applications: &'a Path,
}

fn scan_residuals_in(app: &InstalledApp, roots: &ScanRoots<'_>) -> ResidualScanResult {
    let mut items = Vec::new();

    let home = roots.home;
    let receipts = roots.receipts;
    let darwin_cache = roots.darwin_cache;
    let library = home.join("Library");
    let bundle_id = &app.registry_subpath;
    let app_name = &app.name;
    let bundle_ids = app_bundle_ids(app);

    // ---- 确定 ----
    // Login Item / XPC / appex 的 ID 必须在 .app 还在时从包里读。
    for (index, id) in bundle_ids.iter().enumerate() {
        let primary = index == 0;
        for (subdir, source) in SATELLITE_DIRS {
            add_bundle_satellites(&mut items, &library.join(subdir), id, *source);
        }
        if let Some(cache_root) = darwin_cache {
            add_bundle_satellites(&mut items, cache_root, id, ResidualSource::CacheDir);
        }

        add_named_entry(
            &mut items,
            &library.join("Application Scripts"),
            id,
            "",
            ResidualSource::ApplicationScript,
            primary,
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
    }

    // Application Support 不在卫星表里，避免把整段厂商目录抬成确定。
    for search_name in &[app_name.as_str(), bundle_id.as_str()] {
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

    // App 名（含 Electron 去空格的 productName）。卫星只认 Bundle ID。
    if !app_name.is_empty() {
        for (subdir, source) in [
            ("Caches", ResidualSource::CacheDir),
            ("Logs", ResidualSource::LogDir),
        ] {
            let path = library.join(subdir).join(app_name);
            if path.exists() {
                let size = super::apps::dir_size(&path);
                items.push(ResidualItem::certain(
                    ResidualKind::Directory(path, size),
                    source,
                ));
            }
            add_name_matched_entry(&mut items, &library.join(subdir), app_name, "", source);
        }
        add_name_matched_entry(
            &mut items,
            &library.join("Application Support"),
            app_name,
            "",
            ResidualSource::AppSupportDir,
        );
        add_name_matched_entry(
            &mut items,
            &library.join("Preferences"),
            app_name,
            ".plist",
            ResidualSource::PreferenceFile,
        );
    }

    add_crash_reporter_entries(&mut items, &library, app_name);

    // ---- 可能 ----
    for id in &bundle_ids {
        add_named_entry(
            &mut items,
            &library.join("Containers"),
            id,
            "",
            ResidualSource::ContainerDir,
            false,
        );
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
    }

    // App Group 名称不一定包含主 Bundle ID，必须从已签名 entitlements 读取。
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

    add_vendor_family(&mut items, &library, roots, &bundle_ids);
    add_dotfile_configs(&mut items, home, roots.applications, app);
    add_system_extensions(&mut items, &bundle_ids);

    // 前面的传统精确路径和扩展 Bundle ID 扫描可能指向同一项，去重后重新统计。
    //
    // 去重键必须是 `canonicalize` 的结果，不能是路径字符串。macOS 默认的 APFS
    // 是**大小写不敏感**的：App 叫「QoderWork CN」而磁盘上的目录叫
    // `qoderwork cn` 时，逐字拼接的 `Library/Logs/QoderWork CN` 和 `read_dir`
    // 读回的 `Library/Logs/qoderwork cn` 是两个不同的字符串、同一个目录。按
    // 字符串去重会让它在 UI 上列两遍、体积算两遍（实测能把 47.8 MB 报成
    // 95.6 MB）。大小写敏感卷上两者本就是不同目录，canonicalize 也会如实区分。
    let mut seen = std::collections::HashSet::<PathBuf>::new();
    items.retain(|item| match &item.kind {
        ResidualKind::File(path, _) | ResidualKind::Directory(path, _) => {
            let key = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            seen.insert(key)
        }
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
        // 占位：真值由 `scan_residuals` 在文件扫描之外单独探测后覆盖。
        ..Default::default()
    }
}

/// 检测这款软件是否仍被进程或 launchd 任务占用。两条只读证据链：
///
/// - `ps -axo pid=,args=`：命令行含 Bundle ID（大小写不敏感）或应用名
///   （大小写敏感、词边界命中）。Unix 允许 unlink 正在执行的二进制，
///   「卸载」不等于「退出」——应用删掉了，它的代理进程完全可能还活着，
///   launchd KeepAlive 更是会把它反复拉起（iStat Menus 实测案例）。
/// - `launchctl print gui/<uid>`：解析 `services` 摘要段与末尾 `disabled`
///   段，取登记且未被禁用的任务标签，见 [`parse_launchd_registered`]。
///
/// 两条命令都走 [`crate::core::proc::run_with_timeout`]：它们跑在「用户
/// 点了按钮正在等」的路径上，而 `launchctl print gui/<uid>` 要把整个 gui
/// 域倒出来（本机实测 488 个任务），`Command::output()` 没有超时可言。
///
/// 探测失败（跑不起来、超时、`ps` 输出不可信）宁缺毋滥：返回空证据，
/// 不猜。是否阻断清理由删除层的 live-database 闸门说了算，这里只负责让
/// 用户看得懂。
pub fn detect_occupancy(app: &InstalledApp) -> ResidualOccupancy {
    // macOS 上 `registry_subpath` 存的是 CFBundleIdentifier（字段名来自
    // Windows 的注册表子键）。这个平台差异只该在平台模块里知道，不能让
    // 调用方去解码，所以入口收成 `&InstalledApp`。
    let bundle_id_lower = app.registry_subpath.trim().to_ascii_lowercase();
    let display_name = app.name.trim();
    let mut occ = ResidualOccupancy::default();

    if let Some(run) = run_with_timeout("/bin/ps", &["-axo", "pid=,args="], PROBE_TIMEOUT) {
        // 空/截断/非零退出的 `ps` 是「测不出」，不是「没进程」，按
        // `macos::inuse` 同一套判据丢弃，免得半截记录被当成证据。
        if super::inuse::ps_output_is_usable(run.ok, &run.stdout) {
            occ.processes = String::from_utf8_lossy(&run.stdout)
                .lines()
                .filter(|line| process_args_match(line, &bundle_id_lower, display_name))
                .map(str::to_string)
                .collect();
        }
    }

    let uid = unsafe { libc::getuid() };
    if let Some(run) = run_with_timeout(
        "/bin/launchctl",
        &["print", &format!("gui/{uid}")],
        PROBE_TIMEOUT,
    ) {
        occ.launchd_labels =
            parse_launchd_registered(&String::from_utf8_lossy(&run.stdout), &bundle_id_lower);
    }

    occ
}

/// 两条占用探测命令的超时。取值对齐 `macos::inuse` 的 spot-check：用户正
/// 在等扫描结果，宁可少一条证据也不能让进度条挂住。
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// `ps` 输出的一行是否指向这款软件。`bundle_id_lower` 必须已经小写——
/// 它在整轮扫描里是常量，摊在每行上重复 `to_ascii_lowercase` 是白干。
///
/// Bundle ID 大小写不敏感匹配；应用名大小写敏感，且要求**词边界**命中
/// （命中处两侧不能紧邻字母数字）——否则 "Mail" 会撞上 "MailMate"、
/// "Code" 会撞上 "Encoded"，橙色警示条误报消耗的是信任。名字还要过
/// [`crate::core::apps::is_safe_app_token`]（长度下限 + 通用词黑名单），
/// 和残留扫描其余各处用同一把尺子，不另立一个只看长度的门槛。
fn process_args_match(line: &str, bundle_id_lower: &str, display_name: &str) -> bool {
    if !bundle_id_lower.is_empty() && contains_ignore_ascii_case(line, bundle_id_lower) {
        return true;
    }
    let name = display_name.trim();
    name.len() >= 4 && is_safe_app_token(name) && contains_as_word(line, name)
}

/// 不分配的 ASCII 大小写不敏感包含判断；`needle_lower` 必须已是小写。
fn contains_ignore_ascii_case(haystack: &str, needle_lower: &str) -> bool {
    let (hay, needle) = (haystack.as_bytes(), needle_lower.as_bytes());
    !needle.is_empty()
        && needle.len() <= hay.len()
        && hay
            .windows(needle.len())
            .any(|w| w.eq_ignore_ascii_case(needle))
}

/// 大小写敏感的「词边界」包含判断。
fn contains_as_word(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(start, _)| {
        !haystack[..start].ends_with(|c: char| c.is_ascii_alphanumeric())
            && !haystack[start + needle.len()..].starts_with(|c: char| c.is_ascii_alphanumeric())
    })
}

/// 从带引号的 `"标签" => …` 行里取标签。
fn quoted_label(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let rest = &line[start + 1..];
    let end = rest.find('"')?;
    let label = &rest[..end];
    (!label.is_empty()).then(|| label.to_string())
}

/// 从 `launchctl print gui/<uid>` 的输出里解析这款软件在 launchd 的驻留
/// 证据，返回仍登记**且未被禁用**的任务标签。`needle` 必须已是小写。
///
/// 输出有两个相关段落，形状不同，取标签的方式必须分开：
///
/// - `services = {` 摘要段（域内全部已登记任务）：行形如
///   `<pid> <上次退出状态> <标签>`，**不带引号**。首 token 是 pid（数字）
///   或 `-`，标签取最后一个空白分隔 token。本机实测这个段有 488 个任务，
///   旧解析器因要求引号整段漏掉。
/// - 末尾 `disabled services = {` 段：行形如 `"标签" => enabled|disabled`。
///   只对 `=> enabled` 报警——disabled 意味着不会自启，把它报成「后台
///   仍在运行」会得出与事实相反的结论（iStat Menus 实测：两个任务均已
///   disable、ps 零进程，旧解析器照样亮警示）。
///
/// 已登记但此刻 pid 为 0（未在跑）的也报：开机自启随时会把它拉起来，
/// 用户该知道「清了还会回来」。
fn parse_launchd_registered(output: &str, needle: &str) -> Vec<String> {
    if needle.is_empty() {
        return Vec::new();
    }
    let mut labels: Vec<String> = Vec::new();
    // `None` = 不在关心的段落里；`Some(true)` = disabled 段。三种状态用一个
    // 枚举值表示，免得两个 bool 之间出现「都为真」这种不存在的组合。
    let mut section: Option<bool> = None;
    for line in output.lines() {
        let trimmed = line.trim_start();
        match trimmed {
            "services = {" => {
                section = Some(false);
                continue;
            }
            "disabled services = {" => {
                section = Some(true);
                continue;
            }
            "}" => {
                section = None;
                continue;
            }
            _ => {}
        }
        // 两个分支只负责「把标签抠出来」，命中判定与去重共用下面一条尾巴。
        let label = match section {
            // `"标签" => enabled|disabled`
            Some(true) => trimmed
                .contains("=> enabled")
                .then(|| quoted_label(trimmed))
                .flatten(),
            // `<pid|-> <状态> <标签>`
            Some(false) => {
                let mut tokens = trimmed.split_whitespace();
                let first = tokens.next().unwrap_or_default();
                let is_pid = first == "-" || first.bytes().all(|b| b.is_ascii_digit());
                // 至少三段才是一条记录：少了说明是表头或分隔行。
                (is_pid && !first.is_empty() && tokens.clone().count() >= 2)
                    .then(|| tokens.next_back().map(str::to_string))
                    .flatten()
            }
            None => None,
        };
        let Some(label) = label else { continue };
        if contains_ignore_ascii_case(&label, needle) && !labels.contains(&label) {
            labels.push(label);
        }
    }
    labels
}

/// Electron / Sparkle / XPC 辅助进程用的目录名后缀。
///
/// 这些进程的缓存目录叫 `<主 Bundle ID><后缀>`，例如
/// `com.qoder.work.cn.helper.GPU`。只认这张固定表，**不能**改成通用的
/// 「以 Bundle ID 加点开头」前缀匹配——`com.qoder.work` 和 `com.qoder.work.cn`
/// 是两个各自独立的产品，前缀匹配会让前者把后者的数据一起带走。
///
/// 长后缀在前，避免 `.helper` 先吃掉 `.helper.gpu`。
const HELPER_ID_SUFFIXES: &[&str] = &[
    ".helper.renderer",
    ".helper.plugin",
    ".helper.alerts",
    ".helper.gpu",
    ".helper.np",
    ".helper",
    ".shipit",
    ".sparkle",
    ".xpc",
];

/// 用户级目录里按「本 Bundle ID 的卫星名」收，命中标确定。
const SATELLITE_DIRS: &[(&str, ResidualSource)] = &[
    ("Preferences", ResidualSource::PreferenceFile),
    ("Preferences/ByHost", ResidualSource::PreferenceFile),
    ("Caches", ResidualSource::CacheDir),
    ("Logs", ResidualSource::LogDir),
    ("HTTPStorages", ResidualSource::Other),
    ("Saved Application State", ResidualSource::Other),
    ("LaunchAgents", ResidualSource::LaunchAgent),
];

/// 文件/目录名是不是本 Bundle ID 的卫星残留。
///
/// 只认精确 ID，以及 ID 后面跟封闭后缀（可再跟本机 UUID）：
/// `com.augment.intent.ShipIt.{UUID}.plist` 是 Sparkle 更新器，确定；
/// `org.cindori.SenseiMonitor`、`com.qoder.work.cn` 是另一个产品，不是卫星。
fn is_bundle_satellite(name: &str, bundle_id: &str) -> bool {
    if !valid_bundle_id(bundle_id) {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    let mut stem = lower.as_str();
    for ext in [
        ".plist",
        ".binarycookies",
        ".savedstate",
        ".sfl2",
        ".sfl3",
        ".sfl4",
    ] {
        if let Some(stripped) = stem.strip_suffix(ext) {
            stem = stripped;
            break;
        }
    }
    let id = bundle_id.to_ascii_lowercase();
    if stem == id {
        return true;
    }
    let Some(rest) = stem.strip_prefix(&id).and_then(|r| r.strip_prefix('.')) else {
        return false;
    };
    is_uuid_like(rest) || helper_suffix_then_optional_uuid(rest)
}

fn helper_suffix_then_optional_uuid(rest: &str) -> bool {
    for suffix in HELPER_ID_SUFFIXES {
        let suffix = suffix.trim_start_matches('.');
        if rest == suffix {
            return true;
        }
        if let Some(after) = rest.strip_prefix(suffix) {
            if let Some(uuid) = after.strip_prefix('.') {
                if is_uuid_like(uuid) {
                    return true;
                }
            }
        }
    }
    false
}

fn add_bundle_satellites(
    items: &mut Vec<ResidualItem>,
    root: &Path,
    bundle_id: &str,
    source: ResidualSource,
) {
    if !valid_bundle_id(bundle_id) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !is_bundle_satellite(&name, bundle_id) {
            continue;
        }
        let path = entry.path();
        let size = super::apps::dir_size(&path);
        let kind = if path.is_dir() {
            ResidualKind::Directory(path, size)
        } else {
            ResidualKind::File(path, size)
        };
        items.push(ResidualItem::certain(kind, source));
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

/// `~/Library/Application Support/CrashReporter/{AppName}_{UUID}.plist`
///
/// 崩溃元数据，按显示名精确到 UUID，不会把 `SenseiMonitor` 算进 `Sensei`。
fn add_crash_reporter_entries(items: &mut Vec<ResidualItem>, library: &Path, app_name: &str) {
    if app_name.len() < 2 {
        return;
    }
    let root = library.join("Application Support/CrashReporter");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !crash_reporter_plist_for_app(&name, app_name) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            continue;
        }
        let size = super::apps::dir_size(&path);
        items.push(ResidualItem::certain(
            ResidualKind::File(path, size),
            ResidualSource::CrashDump,
        ));
    }
}

fn crash_reporter_plist_for_app(file_name: &str, app_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    let prefix = format!("{}_", app_name.to_ascii_lowercase());
    let Some(rest) = lower.strip_prefix(&prefix) else {
        return false;
    };
    let Some(uuid) = rest.strip_suffix(".plist") else {
        return false;
    };
    is_uuid_like(uuid)
}

fn is_uuid_like(value: &str) -> bool {
    let mut parts = value.split('-');
    for len in [8, 4, 4, 4, 12] {
        let Some(part) = parts.next() else {
            return false;
        };
        if part.len() != len || !part.bytes().all(|b| b.is_ascii_hexdigit()) {
            return false;
        }
    }
    parts.next().is_none()
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

/// 按「去掉分隔符后完全相等」在目录直接子项里找 App 名对应的残留。
///
/// 只做**相等**，不做前缀：`QoderCN` 和 `QoderWork CN` 归一化后分别是
/// `qodercn` 和 `qoderworkcn`，一字之差是两个产品，其中一个还装着。
fn add_name_matched_entry(
    items: &mut Vec<ResidualItem>,
    root: &Path,
    app_name: &str,
    suffix: &str,
    source: ResidualSource,
) {
    let wanted = config_slug(&format!("{app_name}{suffix}"));
    if wanted.len() < 3 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // 逐字相等的那条路径前面已经收过了，这里只补归一化后才相等的
        if name == format!("{app_name}{suffix}") || config_slug(&name) != wanted {
            continue;
        }
        let path = entry.path();
        let size = super::apps::dir_size(&path);
        let kind = if path.is_dir() {
            ResidualKind::Directory(path, size)
        } else {
            ResidualKind::File(path, size)
        };
        items.push(ResidualItem::certain(kind, source));
    }
}

/// 点目录 `dir_slug` 能不能算作这个 App 的残留。
///
/// - 完全相等是硬证据：`.qoder-cn` ↔「Qoder CN」
/// - 前缀相等只说明「同系列」：`.qoder` ↔「Qoder CN」。这种情况下只要还有
///   别的已安装 App 也姓这个姓（`QoderWork CN`），就谁都不算——`~/.qoder`
///   有 4.7 GB，误判成残留的代价太大
fn dotdir_belongs_to_app(
    dir_slug: &str,
    app_slugs: &[String],
    other_apps: &[String],
    has_exact_sibling: bool,
) -> bool {
    if app_slugs.iter().any(|app_slug| app_slug == dir_slug) {
        return true;
    }
    // 已经找到专属的同名点目录，再收同前缀的短名字就是在拿别人的东西。
    if has_exact_sibling {
        return false;
    }
    // 四字母的点目录名太容易撞车（`.note` 之于「Notebook」），而前缀匹配本来
    // 就只是弱证据，这里比相等匹配多要一个字符。
    if dir_slug.len() < 5 {
        return false;
    }
    // 方向只能是「点目录名 ⊂ App 名」：`karabiner` ⊂ `karabinerelements`。
    // 反过来（App 名 ⊂ 点目录名）会让叫「Disc」的 App 认领 `.config/discord`。
    let family = app_slugs
        .iter()
        .any(|app_slug| app_slug.starts_with(dir_slug));
    family && !other_apps.iter().any(|other| other.starts_with(dir_slug))
}

/// `/Applications` 和 `~/Applications` 里**其它**已安装 App 的归一化名字。
///
/// 用来否决点目录的前缀匹配：`~/.qoder` 到底属于正在卸载的「Qoder CN」还是
/// 仍然装着的「QoderWork CN」，无法确定，那就谁都不算。
fn other_installed_app_slugs(app: &InstalledApp, applications: &Path, home: &Path) -> Vec<String> {
    let self_slug = config_slug(&app.name);
    let mut slugs = Vec::new();
    for root in [applications.to_path_buf(), home.join("Applications")] {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some(stem) = name.strip_suffix(".app") else {
                continue;
            };
            let slug = config_slug(stem);
            if slug.is_empty() || slug == self_slug || slugs.contains(&slug) {
                continue;
            }
            slugs.push(slug);
        }
    }
    slugs
}

fn add_dotfile_configs(
    items: &mut Vec<ResidualItem>,
    home: &Path,
    applications: &Path,
    app: &InstalledApp,
) {
    // App 名和 Bundle ID 末段都可能是点目录用的名字：Karabiner-Elements 写的是
    // `~/.config/karabiner`，两边都不精确相等，只能按前缀包含来判定。
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
    let other_apps = other_installed_app_slugs(app, applications, home);

    // 先把所有候选收齐再判定。前缀匹配要用到「同一批候选里有没有精确命中」
    // 这个信息，边遍历边决定拿不到。
    let mut candidates: Vec<(PathBuf, String)> = Vec::new();
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
            if !entry.path().is_dir() {
                continue;
            }
            candidates.push((entry.path(), slug));
        }
    }

    // 存在精确同名的点目录，就说明这个 App 有自己专属的那一个，同前缀的更短
    // 名字属于系列里的别人：`.qoder-cn` 是「Qoder CN」的，`.qoder`（4.7 GB）
    // 是 qodercli 的。qodercli 没有 .app，靠 `other_apps` 查不出来。
    let has_exact = candidates
        .iter()
        .any(|(_, slug)| slugs.iter().any(|app_slug| app_slug == slug));

    for (path, slug) in candidates {
        if !dotdir_belongs_to_app(&slug, &slugs, &other_apps, has_exact) {
            continue;
        }
        let size = super::apps::dir_size(&path);
        items.push(ResidualItem::possible(
            ResidualKind::Directory(path, size),
            ResidualSource::DotConfigDir,
        ));
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
    for (team_id, bundle_id) in active_system_extensions() {
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
        // 不是失败，是这台机器上根本不允许——重试没有意义，得用户自己去
        // 系统设置 > 通用 > 登录项与扩展 里关。
        return CleanResult::ManualAction;
    }
    let _ = std::process::Command::new("systemextensionsctl")
        .arg("uninstall")
        .arg(team_id)
        .arg(bundle_id)
        .output();

    // 以复查为准：uninstall 是异步的，而且失败时退出码照样是 0。
    if system_extension_active(bundle_id) {
        CleanResult::Failed
    } else {
        CleanResult::Ok
    }
}

/// 该 Bundle ID 的系统扩展是否仍处于 activated 状态。
///
/// 查不到命令时返回 `true`：宁可让残留继续显示，也不要谎报已经清掉。
fn system_extension_active(bundle_id: &str) -> bool {
    keep_system_extension(bundle_id, list_system_extensions().as_deref())
}

/// `listed == None` 表示没查到（命令失败 / 非零退出 / 空输出），当作还在。
fn keep_system_extension(bundle_id: &str, listed: Option<&[(String, String)]>) -> bool {
    match listed {
        None => true,
        Some(active) => active
            .iter()
            .any(|(_, id)| id.eq_ignore_ascii_case(bundle_id)),
    }
}

/// 当前 activated 的系统扩展，命令不可用时返回空表。
///
/// 只给扫描侧用：查不到就不要凭空捏造条目。复核走 [`keep_system_extension`]，
/// 失败方向相反。
fn active_system_extensions() -> Vec<(String, String)> {
    list_system_extensions().unwrap_or_default()
}

fn list_system_extensions() -> Option<Vec<(String, String)>> {
    let output = std::process::Command::new("systemextensionsctl")
        .arg("list")
        .output()
        .ok()?;
    // `Command::output()` 进程能启动就是 Ok，非零退出也算成功拿到 Output。
    // 空 stdout 同样当没查到：正常会至少打出 `0 extension(s)`。
    if !output.status.success() || output.stdout.is_empty() {
        return None;
    }
    Some(parse_system_extensions(&String::from_utf8_lossy(
        &output.stdout,
    )))
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
    let spot_paths: Vec<PathBuf> = items
        .iter()
        .filter_map(|item| match &item.kind {
            ResidualKind::Directory(path, _) | ResidualKind::File(path, _) => Some(path.clone()),
            _ => None,
        })
        .collect();
    let spot = crate::core::inuse::spot_check(&spot_paths);

    for item in items {
        match &item.kind {
            ResidualKind::Directory(path, _) | ResidualKind::File(path, _) => {
                prog.note(path);
                if matches!(
                    spot.get(path),
                    Some(
                        crate::core::inuse::SpotCheck::Busy
                            | crate::core::inuse::SpotCheck::Unknown
                    )
                ) {
                    prog.failed
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    report.record(path, CleanResult::Failed);
                    continue;
                }
                if !item.identity.is_some_and(|identity| identity.recheck(path)) {
                    prog.failed
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    report.record(path, CleanResult::Failed);
                    continue;
                }
                // `/Library` 白名单路径就算已经是 root 也不能走 `clean_path`：
                // `is_protected` 会整棵挡住。攒起来走一次提权批次（root 下
                // 跳过 osascript，仍套白名单）。
                if super::elevate::needs_privileged_delete(path) {
                    elevated.push(path.clone());
                    continue;
                }
                // 残留走废纸篓，不永久删。这是所有清理路径里「删错了最疼、
                // 体积最小」的一条：判据是「这个 app 已经不在任何位置装着
                // 了」，一旦判错，删掉的是活应用的配置、登录态、许可证，
                // 而换来的通常只有几十 MB。缓存和构建产物维持永久删——那些
                // 本来就该重建，进废纸篓只是把占用挪个地方。
                let res = dispose(path, Disposal::RecycleBin, prog);
                report.record(path, res);
            }
            ResidualKind::SystemExtension(team_id, bundle_id) => {
                // 系统扩展没有可删的路径，记标识串——塞进 `PathBuf` 会让
                // 拿这个列表当路径用的代码（`exists()`、在 Finder 中显示）出错。
                let res = deactivate_system_extension(team_id, bundle_id);
                report.record_target(CleanFailure::Id(bundle_id.clone()), res);
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
    // `systemextensionsctl list` 的输出是全局的，一次拿到就够所有条目比对，
    // 不必每条都 fork 一个子进程。查不到时 `listed=None`，下面按「还在」保留，
    // 不能当成空表把扩展滤掉。
    let listed = if items
        .iter()
        .any(|it| matches!(it.kind, ResidualKind::SystemExtension(..)))
    {
        list_system_extensions()
    } else {
        Some(Vec::new())
    };

    items
        .into_iter()
        .filter(|it| match &it.kind {
            ResidualKind::File(p, _) | ResidualKind::Directory(p, _) => p.exists(),
            // 用户可能在两次扫描之间自己去系统设置关掉了扩展，不复查就会一直
            // 挂在残留列表里。查不到命令时保守保留，避免谎报已经清掉。
            ResidualKind::SystemExtension(_, bundle_id) => {
                keep_system_extension(bundle_id, listed.as_deref())
            }
            _ => true,
        })
        // 官方卸载器可能只清掉目录的一部分，不能继续展示卸载前的旧体积。
        .map(|mut item| {
            match &mut item.kind {
                ResidualKind::File(path, size) | ResidualKind::Directory(path, size) => {
                    *size = super::apps::dir_size(path);
                    // 卸载器可能合法地改写或重建了候选目录。旧快照属于卸载前
                    // 的对象，不能拿来授权之后的删除；以本轮复核后的对象重拍。
                    item.identity = crate::core::model::capture_identity(path);
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
                applications: &root.join("applications"),
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
        assert!(has("Application Scripts/cn.better365.ishot"), "{paths:?}");
        assert!(
            has("Application Scripts/cn.better365.ishothelper"),
            "{paths:?}"
        );
        assert!(has("Containers/cn.better365.iShotHelper"), "{paths:?}");
        assert!(has("cn.better365.ishot.sfl3"), "{paths:?}");
        assert!(has("cn.better365.ishot.bom"), "{paths:?}");
        assert!(has("cn.better365.ishot.plist"), "{paths:?}");
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

    /// 卫星用 read_dir + 大小写折叠，不能再靠 `join(bundle_id).exists()`。
    #[test]
    fn scan_finds_cache_dir_ignoring_ascii_case() {
        let root = std::env::temp_dir().join(format!(
            "quick-cleaner-residuals-case-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        std::fs::create_dir_all(home.join("Library/Caches/ORG.CINDORI.SENSEI")).unwrap();
        let receipts = root.join("receipts");
        std::fs::create_dir_all(&receipts).unwrap();

        let app = make_app("Sensei", "org.cindori.Sensei");
        let result = scan_residuals_in(
            &app,
            &ScanRoots {
                home: &home,
                system_library: &root.join("system-library"),
                receipts: &receipts,
                darwin_cache: None,
                applications: &root.join("applications"),
            },
        );
        let certain: Vec<String> = result
            .items
            .iter()
            .filter(|item| item.confidence.is_certain())
            .map(|item| match &item.kind {
                ResidualKind::File(p, _) | ResidualKind::Directory(p, _) => {
                    p.to_string_lossy().into_owned()
                }
                _ => String::new(),
            })
            .collect();
        assert!(
            certain.iter().any(|p| p
                .to_ascii_lowercase()
                .ends_with("library/caches/org.cindori.sensei")),
            "大小写不同的 Caches 目录应确定勾选: {certain:?}"
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
                applications: &root.join("applications"),
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
    fn bundle_satellite_is_this_app_not_a_sibling() {
        let id = "com.augment.intent";
        assert!(is_bundle_satellite("com.augment.intent", id));
        assert!(is_bundle_satellite("com.augment.intent.plist", id));
        assert!(is_bundle_satellite("com.augment.intent.helper", id));
        assert!(is_bundle_satellite(
            "com.augment.intent.C8D05D83-BF4F-55BA-A1CF-CE93688756A3.plist",
            id
        ));
        assert!(is_bundle_satellite(
            "com.augment.intent.ShipIt.C8D05D83-BF4F-55BA-A1CF-CE93688756A3.plist",
            id
        ));
        assert!(
            !is_bundle_satellite("org.cindori.SenseiMonitor", "org.cindori.Sensei"),
            "不能把另一个产品的粘连名字当成卫星"
        );
        assert!(
            !is_bundle_satellite("com.qoder.work.cn", "com.qoder.work"),
            "不能把 Bundle ID 多一段的独立产品当成卫星"
        );
        assert!(!is_bundle_satellite(
            "com.augment.other.C8D05D83-BF4F-55BA-A1CF-CE93688756A3.plist",
            id
        ));
        assert!(!is_bundle_satellite(
            "org.pqrs.Karabiner-Menu.plist",
            "org.pqrs.Karabiner-Elements.Settings"
        ));
    }

    /// ByHost 里 Sparkle ShipIt / 本机 UUID 偏好是本应用的，应默认勾选；
    /// 同厂商另一个产品的 ByHost 只能标「可能」。
    #[test]
    fn scan_marks_byhost_shipit_certain_but_not_sibling_products() {
        let root = std::env::temp_dir().join(format!(
            "quick-cleaner-residuals-byhost-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        let byhost = home.join("Library/Preferences/ByHost");
        std::fs::create_dir_all(&byhost).unwrap();
        let uuid = "C8D05D83-BF4F-55BA-A1CF-CE93688756A3";
        std::fs::write(
            byhost.join(format!("com.augment.intent.ShipIt.{uuid}.plist")),
            b"shipit",
        )
        .unwrap();
        std::fs::write(
            byhost.join(format!("com.augment.intent.{uuid}.plist")),
            b"byhost",
        )
        .unwrap();
        std::fs::write(
            byhost.join(format!("com.augment.other.{uuid}.plist")),
            b"sibling",
        )
        .unwrap();
        let receipts = root.join("receipts");
        std::fs::create_dir_all(&receipts).unwrap();

        let app = make_app("Intent by Augment", "com.augment.intent");
        let result = scan_residuals_in(
            &app,
            &ScanRoots {
                home: &home,
                system_library: &root.join("system-library"),
                receipts: &receipts,
                darwin_cache: None,
                applications: &root.join("applications"),
            },
        );

        let certain: Vec<String> = result
            .items
            .iter()
            .filter(|item| item.confidence.is_certain())
            .map(|item| match &item.kind {
                ResidualKind::File(p, _) | ResidualKind::Directory(p, _) => {
                    p.to_string_lossy().into_owned()
                }
                _ => String::new(),
            })
            .collect();
        let possible: Vec<String> = result
            .items
            .iter()
            .filter(|item| !item.confidence.is_certain())
            .map(|item| match &item.kind {
                ResidualKind::File(p, _) | ResidualKind::Directory(p, _) => {
                    p.to_string_lossy().into_owned()
                }
                _ => String::new(),
            })
            .collect();
        let has = |hay: &[String], needle: &str| hay.iter().any(|p| p.ends_with(needle));

        assert!(
            has(&certain, &format!("com.augment.intent.ShipIt.{uuid}.plist")),
            "ShipIt ByHost 应默认勾选: {certain:?}"
        );
        assert!(
            has(&certain, &format!("com.augment.intent.{uuid}.plist")),
            "本应用 ByHost 应默认勾选: {certain:?}"
        );
        assert!(
            !has(&certain, &format!("com.augment.other.{uuid}.plist")),
            "同厂商另一产品不能默认勾选: {certain:?}"
        );
        assert!(
            has(&possible, &format!("com.augment.other.{uuid}.plist")),
            "同厂商另一产品仍应列成可能: {possible:?}"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn crash_reporter_plist_requires_app_name_and_uuid() {
        assert!(crash_reporter_plist_for_app(
            "Sensei_C8D05D83-BF4F-55BA-A1CF-CE93688756A3.plist",
            "Sensei"
        ));
        assert!(
            crash_reporter_plist_for_app(
                "sensei_c8d05d83-bf4f-55ba-a1cf-ce93688756a3.plist",
                "Sensei"
            ),
            "文件名大小写不能挡住"
        );
        assert!(
            !crash_reporter_plist_for_app(
                "SenseiMonitor_C8D05D83-BF4F-55BA-A1CF-CE93688756A3.plist",
                "Sensei"
            ),
            "不能把辅助进程的崩溃报告算进主应用"
        );
        assert!(!crash_reporter_plist_for_app(
            "Notebook_C8D05D83-BF4F-55BA-A1CF-CE93688756A3.plist",
            "Note"
        ));
        assert!(!crash_reporter_plist_for_app("Sensei.plist", "Sensei"));
        assert!(!crash_reporter_plist_for_app(
            "Sensei_not-a-uuid.plist",
            "Sensei"
        ));
    }

    /// HTTPStorages 的 cookie 文件在目录外面；CrashReporter 按 App 名+UUID 收。
    #[test]
    fn scan_finds_httpstorage_cookies_and_crash_reporter() {
        let root = std::env::temp_dir().join(format!(
            "quick-cleaner-residuals-cookies-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        let http = home.join("Library/HTTPStorages");
        let crash = home.join("Library/Application Support/CrashReporter");
        std::fs::create_dir_all(&http).unwrap();
        std::fs::create_dir_all(&crash).unwrap();
        std::fs::create_dir_all(http.join("org.cindori.Sensei")).unwrap();
        std::fs::write(http.join("org.cindori.Sensei.binarycookies"), b"ck").unwrap();
        std::fs::write(http.join("org.cindori.SenseiMonitor.binarycookies"), b"m").unwrap();
        std::fs::write(
            crash.join("Sensei_C8D05D83-BF4F-55BA-A1CF-CE93688756A3.plist"),
            b"cr",
        )
        .unwrap();
        std::fs::write(
            crash.join("SenseiMonitor_C8D05D83-BF4F-55BA-A1CF-CE93688756A3.plist"),
            b"other",
        )
        .unwrap();
        let receipts = root.join("receipts");
        std::fs::create_dir_all(&receipts).unwrap();

        let app = make_app("Sensei", "org.cindori.Sensei");
        let result = scan_residuals_in(
            &app,
            &ScanRoots {
                home: &home,
                system_library: &root.join("system-library"),
                receipts: &receipts,
                darwin_cache: None,
                applications: &root.join("applications"),
            },
        );

        let certain: Vec<String> = result
            .items
            .iter()
            .filter(|item| item.confidence.is_certain())
            .map(|item| match &item.kind {
                ResidualKind::File(p, _) | ResidualKind::Directory(p, _) => {
                    p.to_string_lossy().into_owned()
                }
                _ => String::new(),
            })
            .collect();
        let has = |needle: &str| certain.iter().any(|p| p.ends_with(needle));

        assert!(
            has("org.cindori.Sensei.binarycookies"),
            "漏了 HTTPStorages 同级 cookie 文件: {certain:?}"
        );
        assert!(
            has("Sensei_C8D05D83-BF4F-55BA-A1CF-CE93688756A3.plist"),
            "漏了 CrashReporter: {certain:?}"
        );
        assert!(
            !has("SenseiMonitor.binarycookies"),
            "辅助进程 cookie 不能默认勾选: {certain:?}"
        );
        assert!(
            !has("SenseiMonitor_C8D05D83-BF4F-55BA-A1CF-CE93688756A3.plist"),
            "辅助进程崩溃报告不能算进主应用: {certain:?}"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    /// Electron 的 userData 目录用 productName，常常和 App 名差一个空格。
    /// 但差一个词就是另一个产品，而且那个产品可能还装着。
    #[test]
    fn name_match_normalizes_separators_but_not_sibling_products() {
        let root = std::env::temp_dir().join(format!(
            "quick-cleaner-residuals-name-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        let support = home.join("Library/Application Support");
        std::fs::create_dir_all(support.join("QoderCN")).unwrap();
        std::fs::create_dir_all(support.join("QoderWork CN")).unwrap();
        std::fs::create_dir_all(support.join("QoderWork")).unwrap();
        std::fs::create_dir_all(home.join("Library/Preferences")).unwrap();
        std::fs::write(home.join("Library/Preferences/QoderCN.plist"), b"p").unwrap();
        let receipts = root.join("receipts");
        std::fs::create_dir_all(&receipts).unwrap();

        let app = make_app("Qoder CN", "com.aliyun.lingma.ide");
        let result = scan_residuals_in(
            &app,
            &ScanRoots {
                home: &home,
                system_library: &root.join("system-library"),
                receipts: &receipts,
                darwin_cache: None,
                applications: &root.join("applications"),
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
            has("Application Support/QoderCN"),
            "漏了去空格的 userData 目录: {paths:?}"
        );
        assert!(
            has("Preferences/QoderCN.plist"),
            "漏了去空格的偏好文件: {paths:?}"
        );
        assert!(!has("QoderWork CN"), "带走了另一个产品的数据: {paths:?}");
        assert!(
            !has("Support/QoderWork"),
            "带走了另一个产品的数据: {paths:?}"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    /// `~/.qoder` 4.7 GB 属于仍然装着的 QoderWork CN，不能算到 Qoder CN 头上。
    /// `~/.qoder` 4.7 GB 不属于「Qoder CN」，两条独立的证据都能否掉它。
    #[test]
    fn dotdir_prefix_match_yields_to_stronger_owners() {
        let app_slugs = vec!["qodercn".to_string()];

        // 没有别的同系列 App，也没有更精确的兄弟目录：前缀匹配成立
        // （Karabiner-Elements ↔ .config/karabiner）
        assert!(dotdir_belongs_to_app(
            "karabiner",
            &["karabinerelements".to_string()],
            &[],
            false
        ));

        // 完全相等永远成立
        assert!(dotdir_belongs_to_app(
            "qodercn",
            &app_slugs,
            &["qoderworkcn".to_string()],
            true
        ));

        // 证据一：同系列的另一个 App 还装着（QoderWork CN）
        assert!(!dotdir_belongs_to_app(
            "qoder",
            &app_slugs,
            &["qoderworkcn".to_string()],
            false
        ));

        // 证据二：同一批候选里已经有精确同名的 `.qoder-cn`，说明 `.qoder`
        // 是系列里别人的。qodercli 是 CLI、没有 .app，只有这条能拦住它。
        assert!(!dotdir_belongs_to_app("qoder", &app_slugs, &[], true));

        // 两条证据都没有时仍然成立
        assert!(dotdir_belongs_to_app("qoder", &app_slugs, &[], false));

        // 压根不沾边
        assert!(!dotdir_belongs_to_app("safari", &app_slugs, &[], false));
    }

    /// 前缀匹配的方向只能是「点目录名 ⊂ App 名」。反过来会让短名字的 App
    /// 认领一堆同前缀的无关目录。
    #[test]
    fn dotdir_prefix_direction_and_length_floor() {
        // 叫「Disc」的 App 不能认领 Discord 的配置目录
        assert!(!dotdir_belongs_to_app(
            "discord",
            &["disc".to_string()],
            &[],
            false
        ));
        // 叫「Note」的 App 不能认领 .config/notes / .config/notebook
        assert!(!dotdir_belongs_to_app(
            "notes",
            &["note".to_string()],
            &[],
            false
        ));
        assert!(!dotdir_belongs_to_app(
            "notebook",
            &["note".to_string()],
            &[],
            false
        ));
        // 反方向且够长才算：`.note` 对「Notebook」仍然太短，不收
        assert!(!dotdir_belongs_to_app(
            "note",
            &["notebook".to_string()],
            &[],
            false
        ));
        // 五个字符起才允许前缀匹配
        assert!(dotdir_belongs_to_app(
            "noteb",
            &["notebook".to_string()],
            &[],
            false
        ));
        // 但完全相等不受长度下限影响
        assert!(dotdir_belongs_to_app(
            "note",
            &["note".to_string()],
            &[],
            false
        ));
    }

    #[test]
    fn verify_keeps_system_extension_when_listing_fails() {
        let id = "org.pqrs.Karabiner-DriverKit-VirtualHIDDevice";
        assert!(
            keep_system_extension(id, None),
            "查不到命令时必须当还在，不能把扩展从复核列表里丢掉"
        );
        assert!(
            !keep_system_extension(id, Some(&[])),
            "命令成功且列表为空才说明已经不在了"
        );
        assert!(keep_system_extension(
            id,
            Some(&[(
                "G43BCU2T37".to_string(),
                "org.pqrs.Karabiner-DriverKit-VirtualHIDDevice".to_string()
            )])
        ));
        assert!(
            keep_system_extension(
                id,
                Some(&[(
                    "G43BCU2T37".to_string(),
                    "ORG.PQRS.KARABINER-DRIVERKIT-VIRTUALHIDDEVICE".to_string()
                )])
            ),
            "bundle id 比较必须忽略大小写"
        );
        assert!(!keep_system_extension(
            id,
            Some(&[("TEAM".to_string(), "com.other.unrelated".to_string())])
        ));
    }

    #[test]
    fn residual_cleanup_rejects_path_replaced_after_scan() {
        let root = crate::core::testing::fixture("qc_residual_identity_swap");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("com.example.app.plist");
        std::fs::write(&path, b"old residual").unwrap();
        let item = ResidualItem::certain(
            ResidualKind::File(path.clone(), 12),
            ResidualSource::PreferenceFile,
        );

        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"new live configuration").unwrap();

        let report = clean_residuals(&[item], &CleanProgress::default());
        assert_eq!(report.failed, vec![CleanFailure::Path(path.clone())]);
        assert_eq!(std::fs::read(&path).unwrap(), b"new live configuration");
        let _ = std::fs::remove_dir_all(root);
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

    #[test]
    fn process_args_match_uses_bundle_id_or_long_name() {
        // Bundle ID：大小写不敏感也命中
        assert!(process_args_match(
            " 401 /Applications/iStat Menus.app/Contents/MacOS/agent COM.BJANGO.ISTATMENUS",
            "com.bjango.istatmenus",
            "iStat Menus",
        ));
        // 应用名：大小写敏感命中（binary 路径里没有 Bundle ID 的常见形态）
        assert!(process_args_match(
            " 402 /Applications/iStat Menus 7/Agent --flag",
            "com.bjango.istatmenus",
            "iStat Menus",
        ));
        // 短名（<4 字符）不参与匹配："Mail" 撞 "Gmail" 这类误报防不住
        assert!(!process_args_match(
            " 403 gmail-imap --fetch",
            "com.other.app",
            "Mail"
        ));
        // 无关行不命中
        assert!(!process_args_match(
            " 404 /usr/sbin/syslogd",
            "com.bjango.istatmenus",
            "iStat Menus",
        ));
    }

    #[test]
    fn launchd_parse_covers_services_section_and_ignores_disabled() {
        // 形状取自真机 `launchctl print gui/<uid>`：services 摘要段不带
        // 引号（<pid> <状态> <标签>），末尾 disabled 段才带引号。
        let output = "\
	services = {
	   61288      - 	application.com.quickcleaner.app.269898235.269898241
	       0      0 	com.bjango.istatmenus.helper
	     727      - 	com.apple.syncdefaultsd
	}
	some other section = {
	   1      - 	com.bjango.istatmenus.outside
	}
	disabled services = {
		\"com.bjango.istatmenus.agent\" => disabled
		\"com.bjango.istatmenus.status\" => disabled
		\"com.bjango.istatmenus.updater\" => enabled
	}
";
        let found = parse_launchd_registered(output, "com.bjango.istatmenus");
        // services 段：登记即报（pid 0 也是登记着、随时会被拉起）
        assert!(found.contains(&"com.bjango.istatmenus.helper".to_string()));
        // 别的段落不误收
        assert!(!found.contains(&"com.bjango.istatmenus.outside".to_string()));
        // disabled 段：只有 => enabled 才报——把已禁用报成「仍在运行」
        // 会得出与事实相反的结论
        assert!(!found.contains(&"com.bjango.istatmenus.agent".to_string()));
        assert!(!found.contains(&"com.bjango.istatmenus.status".to_string()));
        assert!(found.contains(&"com.bjango.istatmenus.updater".to_string()));
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn process_name_match_requires_word_boundaries() {
        // "Mail"（正好 4 字符）不得撞上 MailMate
        assert!(!process_args_match(
            " 403 /Applications/MailMate.app/Contents/MacOS/MailMate",
            "com.other.app",
            "Mail",
        ));
        // 自己的路径仍要命中：/Mail.app 和结尾 /Mail 都是词边界
        assert!(process_args_match(
            " 404 /Applications/Mail.app/Contents/MacOS/Mail",
            "com.other.app",
            "Mail",
        ));
        // "Code" 不撞 "Encoded"
        assert!(!process_args_match(
            " 405 Encoded --transcode file",
            "com.other.app",
            "Code",
        ));
    }

    #[test]
    fn detect_occupancy_finds_spawned_process_and_skips_unrelated() {
        // 端到端：起一个命令行里带 Bundle ID 的进程，探测必须看见它。
        // 前面垫一个 `true;` 防止 sh 把单命令 -c 优化成直接 exec——那样
        // 注释会从 argv 里消失（实测踩过）。
        let mut child = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("true; sleep 30 # com.qc.occupancy.probe")
            .spawn()
            .expect("spawn probe process");
        let found = detect_occupancy(&make_app("qc_occupancy_probe", "com.qc.occupancy.probe"));
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            found
                .processes
                .iter()
                .any(|p| p.contains("com.qc.occupancy.probe")),
            "探测应看见刚起的进程，实际 {:?}",
            found.processes
        );
        assert!(found.launchd_labels.is_empty(), "测试环境不该有登记任务");

        // 无关应用：两条链都应为空
        let none = detect_occupancy(&make_app("qc_nothing_here_app", "com.qc.nothing.here"));
        assert!(
            none.processes.is_empty() && none.launchd_labels.is_empty(),
            "procs={:?} labels={:?}",
            none.processes,
            none.launchd_labels
        );
    }
}
