//! Windows 软件残留（文件与注册表）深度探测与清理
//!
//! 扫描位置参照开源的 Bulk Crap Uninstaller 的 junk finder 清单整理而来
//! （`Junk/Finders/Registry` 与 `Junk/Finders/Misc`），关键注册表路径逐个
//! 核对过。旧实现只查了三处（安装目录、AppData 同名目录、
//! `Software\<厂商>\<名字>`），所以经常报「非常干净」——**连该软件自己
//! 的卸载登记项都没算进去**，于是清完之后它照样出现在软件列表里。
//!
//! # 匹配的把握程度
//!
//! 每条残留都带 [`Confidence`]：
//!
//! - **确定**：有硬证据——路径就是安装目录本身、注册表值直接指向安装
//!   目录、或者本来就是该软件的卸载登记项。默认勾选。
//! - **可能**：只是名字相近。默认**不**勾选，交给用户判断。
//!
//! 这个区分是必要的：靠名字模糊匹配去删注册表，一次误判就可能带走别的
//! 软件的配置。

use crate::core::apps::{
    is_safe_app_token, AppRegRoot, Confidence, InstalledApp, ResidualItem, ResidualKind,
    ResidualScanResult,
};
use crate::core::cleaner::{clean_path, CleanProgress, CleanReport};
use crate::core::safety::{is_protected_residual_path, is_system_root_dir};
use crate::platform::windows::apps::dir_or_file_size;
use crate::platform::windows::registry::{
    delete_reg_tree, delete_reg_value, enum_string_values, enum_subkeys, read_reg_string, to_wide,
};
use std::path::{Path, PathBuf};

use winapi::shared::minwindef::{DWORD, HKEY};
use winapi::shared::winerror::ERROR_SUCCESS;
use winapi::um::winnt::{KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
use winapi::um::winreg::{RegCloseKey, RegOpenKeyExW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

// ---------------------------------------------------------------------------
// 注册表路径常量
// ---------------------------------------------------------------------------

const APP_PATHS: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths";
const RUN_KEYS: &[&str] = &[
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce",
];
const SERVICES: &str = r"SYSTEM\CurrentControlSet\Services";
const FIREWALL_RULES: &str =
    r"SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters\FirewallPolicy\FirewallRules";
const TRACING: &str = r"SOFTWARE\Microsoft\Tracing";
const HEAP_LEAK: &str = r"SOFTWARE\Microsoft\RADAR\HeapLeakDetection\DiagnosedApplications";
const INSTALLER_FOLDERS: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\Folders";
const REGISTERED_APPS: &str = r"SOFTWARE\RegisteredApplications";
const MUI_CACHE: &str =
    r"Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache";
const APP_COMPAT: &[&str] = &[
    r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers",
    r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Store",
];

// ---------------------------------------------------------------------------
// 匹配辅助
// ---------------------------------------------------------------------------

/// 去掉标点与空白，只留字母数字（含中日韩），转小写。
///
/// 「同花顺 v9.50」与「同花顺」归一化后可以互相匹配。
fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn sanitize_token(s: &str) -> String {
    s.trim()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ' ')
        .collect::<String>()
        .trim()
        .to_string()
}

/// 一次扫描所需的上下文：软件本身 + 预先算好的匹配依据。
struct Ctx {
    /// 归一化后的软件名，用于模糊匹配
    name_norm: String,
    /// 清理过标点的软件名，用于拼接目录/键路径
    name_token: String,
    publisher_token: String,
    /// 归一化后的安装目录（小写、`/` 已换成 `\`），没有则为空
    install_dir: String,
    /// 该软件的可执行文件名（小写，含扩展名）
    exe_names: Vec<String>,
}

impl Ctx {
    fn new(app: &InstalledApp) -> Self {
        // 注意：这里**不**要求目录当前存在。
        //
        // 注册表里那些指向安装目录的值（厂商配置、服务 ImagePath、启动项）
        // 在目录被删掉之后依然留着原路径文本，正是靠它们才能反查出用英文
        // 厂商名建的键（同花顺 → `Software\HexinSoft`）。要求目录存在会让
        // 「已经卸载但残留还在」这种最需要清理的情况完全失效。
        // 真正需要目录存在的只有 `scan_install_dir`，它自己会判断。
        let install_dir = app
            .install_location
            .as_ref()
            .filter(|p| !is_system_root_dir(p))
            .map(|p| crate::core::safety::norm(p))
            .filter(|s| s.len() > 3)
            .unwrap_or_default();

        Self {
            name_norm: norm(&app.name),
            name_token: sanitize_token(&app.name),
            publisher_token: sanitize_token(&app.publisher),
            exe_names: collect_exe_names(app, &install_dir),
            install_dir,
        }
    }

    /// 软件名是否足够特征化，可以拿来做模糊匹配。
    ///
    /// 名字太短或太通用（"App"、"Microsoft"）时一律不做模糊匹配，
    /// 否则会把半个系统都当成残留。
    fn name_is_matchable(&self) -> bool {
        self.name_norm.chars().count() >= 3 && is_safe_app_token(&self.name_token)
    }

    /// 某个路径是否位于安装目录内（按路径分隔符对齐，不做纯前缀比较）。
    ///
    /// 新增扫描器判断「这个路径属于该软件吗」时应当用它，而不是
    /// `starts_with`——后者会把 `foobar` 误判成 `foo` 的子目录。
    #[allow(dead_code)]
    fn under_install_dir(&self, path_lower: &str) -> bool {
        !self.install_dir.is_empty()
            && (path_lower == self.install_dir
                || path_lower.starts_with(&format!("{}\\", self.install_dir)))
    }

    /// 候选名字是否与软件名相近。
    fn name_matches(&self, candidate: &str) -> bool {
        if !self.name_is_matchable() {
            return false;
        }
        let c = norm(candidate);
        if c.chars().count() < 3 {
            return false;
        }
        c.contains(&self.name_norm) || self.name_norm.contains(&c)
    }

    /// 候选文本里是否出现了安装目录路径（命令行、ImagePath 之类）。
    fn mentions_install_dir(&self, text: &str) -> bool {
        if self.install_dir.is_empty() {
            return false;
        }
        text.to_ascii_lowercase()
            .replace('/', "\\")
            .contains(&self.install_dir)
    }

    fn mentions_exe(&self, text: &str) -> bool {
        let lower = text.to_ascii_lowercase();
        self.exe_names.iter().any(|e| lower.contains(e.as_str()))
    }
}

/// 收集该软件的可执行文件名。
///
/// HeapLeakDetection、App Paths、MuiCache 这些位置都是以 exe 名为键的，
/// 没有这份清单就匹配不上。
fn collect_exe_names(app: &InstalledApp, install_dir: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let push = |s: &str, out: &mut Vec<String>| {
        let lower = s.to_ascii_lowercase();
        if lower.ends_with(".exe") && !out.contains(&lower) {
            out.push(lower);
        }
    };

    for src in [&app.display_icon, &app.uninstall_string] {
        if let Some(v) = src {
            let clean = v.split(',').next().unwrap_or("").trim_matches('"').trim();
            if let Some(f) = Path::new(clean).file_name().and_then(|f| f.to_str()) {
                push(f, &mut out);
            }
        }
    }

    // 安装目录顶层的 exe（只看一层，深层多是子组件）
    if !install_dir.is_empty() {
        if let Ok(rd) = std::fs::read_dir(install_dir) {
            for e in rd.flatten().take(300) {
                if let Some(f) = e.file_name().to_str() {
                    push(f, &mut out);
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 注册表访问小工具
// ---------------------------------------------------------------------------

fn hkey_of(root: AppRegRoot) -> HKEY {
    match root {
        AppRegRoot::Hkcu => HKEY_CURRENT_USER,
        _ => HKEY_LOCAL_MACHINE,
    }
}

fn sam_of(root: AppRegRoot) -> DWORD {
    match root {
        AppRegRoot::Hklm32 => KEY_READ | KEY_WOW64_32KEY,
        AppRegRoot::Hkcu => KEY_READ,
        _ => KEY_READ | KEY_WOW64_64KEY,
    }
}

fn reg_key_exists(root: HKEY, subpath: &str, sam: DWORD) -> bool {
    let wide = to_wide(subpath);
    let mut h_key: HKEY = std::ptr::null_mut();
    unsafe {
        if RegOpenKeyExW(root, wide.as_ptr(), 0, sam, &mut h_key) as u32 == ERROR_SUCCESS {
            RegCloseKey(h_key);
            true
        } else {
            false
        }
    }
}

fn open_and_read(root: HKEY, subpath: &str, value: &str, sam: DWORD) -> Option<String> {
    let wide = to_wide(subpath);
    let mut h: HKEY = std::ptr::null_mut();
    unsafe {
        if RegOpenKeyExW(root, wide.as_ptr(), 0, sam, &mut h) as u32 != ERROR_SUCCESS {
            return None;
        }
        let v = read_reg_string(h, value);
        RegCloseKey(h);
        v
    }
}

// ---------------------------------------------------------------------------
// 扫描
// ---------------------------------------------------------------------------

/// 扫描指定软件在磁盘与注册表中的残留项
pub fn scan_residuals(app: &InstalledApp) -> ResidualScanResult {
    let ctx = Ctx::new(app);
    let mut items: Vec<ResidualItem> = Vec::new();

    scan_uninstall_entry(app, &mut items);
    scan_install_dir(app, &mut items);
    scan_orphan_ancestors(app, &mut items);
    scan_data_dirs(&ctx, &mut items);
    scan_shortcuts(&ctx, &mut items);
    scan_software_keys(&ctx, &mut items);
    scan_vendor_keys_by_path(&ctx, &mut items);
    scan_app_paths(&ctx, &mut items);
    scan_run_keys(&ctx, &mut items);
    scan_services(&ctx, &mut items);
    scan_firewall_rules(&ctx, &mut items);
    scan_tracing(&ctx, &mut items);
    scan_heap_leak(&ctx, &mut items);
    scan_app_compat(&ctx, &mut items);
    scan_installer_folders(&ctx, &mut items);
    scan_mui_cache(&ctx, &mut items);
    scan_registered_apps(&ctx, &mut items);

    dedup_items(&mut items);
    // 「确定」的排在前面，用户先看到的就是可以放心删的
    items.sort_by(|a, b| b.confidence.cmp(&a.confidence));

    let total_file_size = items.iter().map(|i| i.size()).sum();
    ResidualScanResult {
        app_name: app.name.clone(),
        items,
        total_file_size,
    }
}

/// 该软件自己的卸载登记项。
///
/// 这一条是旧实现最大的窟窿：不删它，软件永远留在「已安装」列表里，
/// 用户清完残留一看列表还在，自然觉得没清干净。
fn scan_uninstall_entry(app: &InstalledApp, out: &mut Vec<ResidualItem>) {
    if app.registry_subpath.is_empty() {
        return;
    }
    if reg_key_exists(
        hkey_of(app.registry_root),
        &app.registry_subpath,
        sam_of(app.registry_root),
    ) {
        out.push(ResidualItem::certain(
            ResidualKind::RegistryKey(app.registry_root, app.registry_subpath.clone()),
            "卸载登记项",
        ));
    }
}

fn scan_install_dir(app: &InstalledApp, out: &mut Vec<ResidualItem>) {
    let Some(loc) = &app.install_location else {
        return;
    };
    if !loc.exists() || is_system_root_dir(loc) {
        return;
    }
    let size = dir_or_file_size(loc);
    let kind = if loc.is_dir() {
        ResidualKind::Directory(loc.clone(), size)
    } else {
        ResidualKind::File(loc.clone(), size)
    };
    out.push(ResidualItem::certain(kind, "安装目录"));
}

/// 安装目录被删掉后，上层那些只为它建的目录会空着留下来。
///
/// 例如同花顺装在 `C:\同花顺软件\同花顺\`，卸载后 `C:\同花顺软件` 变成
/// 空壳继续留在根目录下。沿父链往上找，遇到「存在且不含任何文件」的
/// 目录就报出来；一旦碰到非空目录或系统骨架目录就停，绝不越界。
fn scan_orphan_ancestors(app: &InstalledApp, out: &mut Vec<ResidualItem>) {
    let Some(loc) = &app.install_location else {
        return;
    };
    let mut cur = loc.parent();
    while let Some(p) = cur {
        if is_system_root_dir(p) || is_protected_residual_path(p) {
            break;
        }
        if p.exists() {
            if !dir_has_no_files(p) {
                break; // 还有别的东西在用，不能算残留
            }
            out.push(ResidualItem::certain(
                ResidualKind::Directory(p.to_path_buf(), 0),
                "空的安装父目录",
            ));
        }
        cur = p.parent();
    }
}

/// 目录里是否一个文件都没有（只有空子目录也算空）。
fn dir_has_no_files(dir: &Path) -> bool {
    !walkdir::WalkDir::new(dir)
        .into_iter()
        .flatten()
        .any(|e| e.file_type().is_file())
}

/// 靠「值里提到安装目录」反查厂商配置键。
///
/// 中文软件常用英文厂商名建键——同花顺的键是 `Software\HexinSoft`，
/// 光靠软件名/厂商名的字面匹配永远对不上。改为枚举 `Software` 下两层
/// 子键，看它们的值里有没有出现安装目录路径，这是硬证据。
fn scan_vendor_keys_by_path(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    if ctx.install_dir.is_empty() {
        return;
    }
    let bases = [
        (AppRegRoot::Hkcu, "Software"),
        (AppRegRoot::Hklm, "SOFTWARE"),
        (AppRegRoot::Hklm32, r"SOFTWARE\WOW6432Node"),
    ];

    for (reg_root, base) in bases {
        let h = hkey_of(reg_root);
        let sam = sam_of(reg_root);
        for lvl1 in enum_subkeys(h, base, sam).into_iter().take(1500) {
            // 64 位视图下枚举 SOFTWARE 会把 WOW6432Node 当成普通子键走进去，
            // 而它已经由上面的 Hklm32 分支覆盖，否则同一个物理键会报两遍
            if lvl1.eq_ignore_ascii_case("WOW6432Node") {
                continue;
            }
            let p1 = format!(r"{base}\{lvl1}");
            if values_mention(h, &p1, sam, ctx) {
                out.push(ResidualItem::certain(
                    ResidualKind::RegistryKey(reg_root, p1),
                    "厂商配置项",
                ));
                continue; // 命中就不再往下钻，整棵删掉即可
            }
            for lvl2 in enum_subkeys(h, &p1, sam).into_iter().take(200) {
                let p2 = format!(r"{p1}\{lvl2}");
                if values_mention(h, &p2, sam, ctx) {
                    out.push(ResidualItem::certain(
                        ResidualKind::RegistryKey(reg_root, p2),
                        "厂商配置项",
                    ));
                }
            }
        }
    }
}

fn values_mention(h: HKEY, subpath: &str, sam: DWORD, ctx: &Ctx) -> bool {
    enum_string_values(h, subpath, sam)
        .iter()
        .any(|(_, data)| ctx.mentions_install_dir(data))
}

/// `%AppData%` / `%LocalAppData%` / `%ProgramData%` / `Program Files` 下的数据目录。
///
/// 同名目录算「确定」，模糊匹配到的算「可能」。
fn scan_data_dirs(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    if !ctx.name_is_matchable() {
        return;
    }

    let mut roots: Vec<PathBuf> = Vec::new();
    for d in [dirs::data_dir(), dirs::data_local_dir()] {
        if let Some(p) = d {
            roots.push(p);
        }
    }
    if let Some(local) = dirs::data_local_dir() {
        roots.push(local.join("Programs"));
        roots.push(local.join(r"VirtualStore\Program Files"));
        roots.push(local.join(r"VirtualStore\Program Files (x86)"));
    }
    for var in ["ProgramData", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(v) = std::env::var(var) {
            roots.push(PathBuf::from(v));
        }
    }

    for root in roots {
        // 精确同名：<Root>\<AppName> 与 <Root>\<Publisher>\<AppName>
        let mut exact: Vec<PathBuf> = vec![root.join(&ctx.name_token)];
        if is_safe_app_token(&ctx.publisher_token) {
            exact.push(root.join(&ctx.publisher_token).join(&ctx.name_token));
        }
        for p in exact {
            if p.exists() && !is_protected_residual_path(&p) {
                push_dir(out, p, Confidence::Certain, "应用数据目录");
            }
        }

        // 模糊：枚举一层子目录，名字相近的报为「可能」
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in rd.flatten().take(4000) {
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.eq_ignore_ascii_case(&ctx.name_token) {
                continue; // 已作为精确项收录
            }
            if ctx.name_matches(name) {
                let p = entry.path();
                if !is_protected_residual_path(&p) {
                    push_dir(out, p, Confidence::Possible, "疑似应用数据目录");
                }
            }
        }
    }
}

/// 开始菜单与桌面的快捷方式。
fn scan_shortcuts(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    if !ctx.name_is_matchable() {
        return;
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Desktop"));
    }
    if let Some(roaming) = dirs::data_dir() {
        roots.push(roaming.join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Ok(pd) = std::env::var("ProgramData") {
        roots.push(PathBuf::from(pd).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Ok(public) = std::env::var("PUBLIC") {
        roots.push(PathBuf::from(public).join("Desktop"));
    }

    for root in roots {
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in rd.flatten().take(2000) {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let stem = name.trim_end_matches(".lnk");
            if !ctx.name_matches(stem) {
                continue;
            }
            let p = entry.path();
            let conf = if norm(stem) == ctx.name_norm {
                Confidence::Certain
            } else {
                Confidence::Possible
            };
            if p.is_dir() {
                push_dir(out, p, conf, "开始菜单目录");
            } else {
                let size = dir_or_file_size(&p);
                out.push(ResidualItem {
                    kind: ResidualKind::File(p, size),
                    confidence: conf,
                    source: "快捷方式",
                });
            }
        }
    }
}

/// `HKCU\Software\...` 与 `HKLM\SOFTWARE\...` 下的配置键。
fn scan_software_keys(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    if !ctx.name_is_matchable() {
        return;
    }
    let targets = [
        (AppRegRoot::Hkcu, "Software"),
        (AppRegRoot::Hklm, "SOFTWARE"),
        (AppRegRoot::Hklm32, r"SOFTWARE\WOW6432Node"),
    ];

    for (reg_root, base) in targets {
        let h = hkey_of(reg_root);
        let sam = sam_of(reg_root);

        let mut direct = vec![format!("{base}\\{}", ctx.name_token)];
        if is_safe_app_token(&ctx.publisher_token) {
            direct.push(format!("{base}\\{}\\{}", ctx.publisher_token, ctx.name_token));
        }
        for sub in direct {
            if reg_key_exists(h, &sub, sam) {
                out.push(ResidualItem::certain(
                    ResidualKind::RegistryKey(reg_root, sub),
                    "配置注册表项",
                ));
            }
        }

        // 模糊：枚举一层子键
        for sub in enum_subkeys(h, base, sam) {
            if sub.eq_ignore_ascii_case(&ctx.name_token)
                || sub.eq_ignore_ascii_case("WOW6432Node")
            {
                continue;
            }
            if ctx.name_matches(&sub) {
                out.push(ResidualItem::possible(
                    ResidualKind::RegistryKey(reg_root, format!("{base}\\{sub}")),
                    "疑似配置注册表项",
                ));
            }
        }
    }
}

/// `App Paths`：以可执行文件名为子键，默认值是完整路径。
fn scan_app_paths(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    for (reg_root, sam) in [
        (AppRegRoot::Hklm, KEY_READ | KEY_WOW64_64KEY),
        (AppRegRoot::Hklm32, KEY_READ | KEY_WOW64_32KEY),
    ] {
        for sub in enum_subkeys(HKEY_LOCAL_MACHINE, APP_PATHS, sam) {
            let full = format!("{APP_PATHS}\\{sub}");
            let target = open_and_read(HKEY_LOCAL_MACHINE, &full, "", sam).unwrap_or_default();
            let hit_dir = ctx.mentions_install_dir(&target);
            let hit_exe = ctx.exe_names.iter().any(|e| sub.eq_ignore_ascii_case(e));
            if hit_dir || hit_exe {
                out.push(ResidualItem::certain(
                    ResidualKind::RegistryKey(reg_root, full),
                    "App Paths 登记",
                ));
            }
        }
    }
}

/// 开机启动项。以「值」的形式存在，只能删值不能删键。
fn scan_run_keys(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    for key in RUN_KEYS {
        for (reg_root, h, sam) in [
            (AppRegRoot::Hkcu, HKEY_CURRENT_USER, KEY_READ),
            (
                AppRegRoot::Hklm,
                HKEY_LOCAL_MACHINE,
                KEY_READ | KEY_WOW64_64KEY,
            ),
            (
                AppRegRoot::Hklm32,
                HKEY_LOCAL_MACHINE,
                KEY_READ | KEY_WOW64_32KEY,
            ),
        ] {
            for (name, data) in enum_string_values(h, key, sam) {
                let certain = ctx.mentions_install_dir(&data) || ctx.mentions_exe(&data);
                let possible = ctx.name_matches(&name);
                if !certain && !possible {
                    continue;
                }
                out.push(ResidualItem {
                    kind: ResidualKind::RegistryValue(reg_root, key.to_string(), name),
                    confidence: if certain {
                        Confidence::Certain
                    } else {
                        Confidence::Possible
                    },
                    source: "开机启动项",
                });
            }
        }
    }
}

/// 服务：`ImagePath` 指向安装目录的算残留。
fn scan_services(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    if ctx.install_dir.is_empty() && !ctx.name_is_matchable() {
        return;
    }
    let sam = KEY_READ | KEY_WOW64_64KEY;
    for svc in enum_subkeys(HKEY_LOCAL_MACHINE, SERVICES, sam) {
        let full = format!("{SERVICES}\\{svc}");
        let image = open_and_read(HKEY_LOCAL_MACHINE, &full, "ImagePath", sam).unwrap_or_default();
        if ctx.mentions_install_dir(&image) {
            out.push(ResidualItem::certain(
                ResidualKind::RegistryKey(AppRegRoot::Hklm, full),
                "服务",
            ));
        } else if !image.is_empty() && ctx.name_matches(&svc) {
            out.push(ResidualItem::possible(
                ResidualKind::RegistryKey(AppRegRoot::Hklm, full),
                "疑似服务",
            ));
        }
    }
}

/// 防火墙规则：值的内容里含 `App=<路径>`。
fn scan_firewall_rules(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    if ctx.install_dir.is_empty() {
        return;
    }
    let sam = KEY_READ | KEY_WOW64_64KEY;
    for (name, data) in enum_string_values(HKEY_LOCAL_MACHINE, FIREWALL_RULES, sam) {
        if ctx.mentions_install_dir(&data) {
            out.push(ResidualItem::certain(
                ResidualKind::RegistryValue(AppRegRoot::Hklm, FIREWALL_RULES.to_string(), name),
                "防火墙规则",
            ));
        }
    }
}

/// `SOFTWARE\Microsoft\Tracing`：子键形如 `<程序名>_RASAPI32`。
fn scan_tracing(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    if !ctx.name_is_matchable() {
        return;
    }
    let sam = KEY_READ | KEY_WOW64_64KEY;
    for sub in enum_subkeys(HKEY_LOCAL_MACHINE, TRACING, sam) {
        // 去掉 _RASAPI32 / _RASMANCS 之类的后缀再比对
        let stem = match sub.rfind('_') {
            Some(i) if i > 0 => &sub[..i],
            _ => sub.as_str(),
        };
        if ctx.name_matches(stem) {
            out.push(ResidualItem::possible(
                ResidualKind::RegistryKey(AppRegRoot::Hklm, format!("{TRACING}\\{sub}")),
                "RAS 跟踪记录",
            ));
        }
    }
}

/// `RADAR\HeapLeakDetection`：子键就是可执行文件名。
fn scan_heap_leak(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    if ctx.exe_names.is_empty() {
        return;
    }
    let sam = KEY_READ | KEY_WOW64_64KEY;
    for sub in enum_subkeys(HKEY_LOCAL_MACHINE, HEAP_LEAK, sam) {
        if ctx.exe_names.iter().any(|e| sub.eq_ignore_ascii_case(e)) {
            out.push(ResidualItem::certain(
                ResidualKind::RegistryKey(AppRegRoot::Hklm, format!("{HEAP_LEAK}\\{sub}")),
                "内存泄漏诊断记录",
            ));
        }
    }
}

/// 兼容性设置：值名就是可执行文件的完整路径。
fn scan_app_compat(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    if ctx.install_dir.is_empty() {
        return;
    }
    for key in APP_COMPAT {
        for (reg_root, h, sam) in [
            (AppRegRoot::Hkcu, HKEY_CURRENT_USER, KEY_READ),
            (
                AppRegRoot::Hklm,
                HKEY_LOCAL_MACHINE,
                KEY_READ | KEY_WOW64_64KEY,
            ),
        ] {
            for (name, _) in enum_string_values(h, key, sam) {
                if ctx.mentions_install_dir(&name) {
                    out.push(ResidualItem::certain(
                        ResidualKind::RegistryValue(reg_root, key.to_string(), name),
                        "兼容性设置",
                    ));
                }
            }
        }
    }
}

/// `Installer\Folders`：值名是安装过程中创建过的目录路径。
fn scan_installer_folders(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    if ctx.install_dir.is_empty() {
        return;
    }
    let sam = KEY_READ | KEY_WOW64_64KEY;
    for (name, _) in enum_string_values(HKEY_LOCAL_MACHINE, INSTALLER_FOLDERS, sam) {
        if ctx.mentions_install_dir(&name) {
            out.push(ResidualItem::certain(
                ResidualKind::RegistryValue(AppRegRoot::Hklm, INSTALLER_FOLDERS.to_string(), name),
                "安装器目录登记",
            ));
        }
    }
}

/// MuiCache：值名是可执行文件的完整路径。
fn scan_mui_cache(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    if ctx.install_dir.is_empty() {
        return;
    }
    for (name, _) in enum_string_values(HKEY_CURRENT_USER, MUI_CACHE, KEY_READ) {
        if ctx.mentions_install_dir(&name) {
            out.push(ResidualItem::certain(
                ResidualKind::RegistryValue(AppRegRoot::Hkcu, MUI_CACHE.to_string(), name),
                "程序名缓存",
            ));
        }
    }
}

/// `RegisteredApplications`：值指向 `SOFTWARE\...\Capabilities`。
fn scan_registered_apps(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    if !ctx.name_is_matchable() {
        return;
    }
    let sam = KEY_READ | KEY_WOW64_64KEY;
    for (name, data) in enum_string_values(HKEY_LOCAL_MACHINE, REGISTERED_APPS, sam) {
        if ctx.name_matches(&name) || ctx.name_matches(&data) {
            out.push(ResidualItem::possible(
                ResidualKind::RegistryValue(AppRegRoot::Hklm, REGISTERED_APPS.to_string(), name),
                "默认程序登记",
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// 小工具
// ---------------------------------------------------------------------------

fn push_dir(out: &mut Vec<ResidualItem>, path: PathBuf, conf: Confidence, source: &'static str) {
    let size = dir_or_file_size(&path);
    out.push(ResidualItem {
        kind: ResidualKind::Directory(path, size),
        confidence: conf,
        source,
    });
}

/// 去重：`Vec::dedup` 只能消掉相邻重复项，而这里的 items 并非有序。
///
/// 同一目标被多个扫描器发现时，保留把握更高的那条。
fn dedup_items(items: &mut Vec<ResidualItem>) {
    let mut kept: Vec<ResidualItem> = Vec::with_capacity(items.len());
    for it in items.drain(..) {
        match kept.iter_mut().find(|k| k.kind == it.kind) {
            Some(existing) => {
                if it.confidence > existing.confidence {
                    *existing = it;
                }
            }
            None => kept.push(it),
        }
    }
    *items = kept;
}

/// 复核候选残留是否仍然存在，丢弃已经消失的。
///
/// 配合「先扫描后卸载」使用：真正的证据（安装目录、指向它的注册表值）
/// 只在卸载**之前**存在，所以候选集必须提前采集；官方卸载程序跑完之后
/// 再用这个函数筛一遍，剩下的才是它没清干净的部分。
pub fn verify_residuals(items: Vec<ResidualItem>) -> Vec<ResidualItem> {
    items
        .into_iter()
        .filter(|it| match &it.kind {
            ResidualKind::File(p, _) | ResidualKind::Directory(p, _) => p.exists(),
            ResidualKind::RegistryKey(root, sub) => {
                reg_key_exists(hkey_of(*root), sub, sam_of(*root))
            }
            ResidualKind::RegistryValue(root, sub, name) => {
                enum_string_values(hkey_of(*root), sub, sam_of(*root))
                    .iter()
                    .any(|(n, _)| n.eq_ignore_ascii_case(name))
            }
        })
        // 体积在卸载后会变（安装目录可能只剩残渣），重新算一遍
        .map(|mut it| {
            if let ResidualKind::Directory(p, size) = &mut it.kind {
                *size = dir_or_file_size(p);
            }
            it
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 清理
// ---------------------------------------------------------------------------

/// 执行残留清理
pub fn clean_residuals(items: &[ResidualItem], prog: &CleanProgress) -> CleanReport {
    let mut report = CleanReport::default();

    for item in items {
        if prog.cancelled() {
            break;
        }

        match &item.kind {
            ResidualKind::Directory(path, _) | ResidualKind::File(path, _) => {
                prog.note(path);
                let res = clean_path(path, prog);
                report.record(path, res);
            }
            ResidualKind::RegistryKey(root, subpath) => {
                if delete_reg_tree(hkey_of(*root), subpath) {
                    report.ok += 1;
                } else {
                    report
                        .failed
                        .push(PathBuf::from(format!("{}\\{}", root.label(), subpath)));
                }
            }
            ResidualKind::RegistryValue(root, subpath, name) => {
                let sam = match root {
                    AppRegRoot::Hklm32 => KEY_WOW64_32KEY,
                    AppRegRoot::Hkcu => 0,
                    _ => KEY_WOW64_64KEY,
                };
                if delete_reg_value(hkey_of(*root), subpath, name, sam) {
                    report.ok += 1;
                } else {
                    report.failed.push(PathBuf::from(format!(
                        "{}\\{} → {}",
                        root.label(),
                        subpath,
                        name
                    )));
                }
            }
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, publisher: &str) -> InstalledApp {
        InstalledApp {
            id: name.into(),
            name: name.into(),
            version: "1.0".into(),
            publisher: publisher.into(),
            last_used_date: None,
            last_used_raw: 0,
            install_date: None,
            install_date_raw: 0,
            install_location: None,
            display_icon: None,
            uninstall_string: None,
            quiet_uninstall_string: None,
            estimated_size: 0,
            registry_root: AppRegRoot::Hklm,
            registry_subpath: String::new(),
            is_system_component: false,
            uninstaller_missing: false,
        }
    }

    fn ctx_with_dir(name: &str, dir: &str) -> Ctx {
        Ctx {
            install_dir: dir.into(),
            ..Ctx::new(&app(name, "SomePublisher"))
        }
    }

    #[test]
    fn norm_keeps_cjk_and_drops_punctuation() {
        assert_eq!(norm("同花顺 v9.50"), "同花顺v950");
        assert_eq!(norm("Adobe Acrobat (64-bit)"), "adobeacrobat64bit");
        assert_eq!(norm("A-B_C"), "abc");
    }

    #[test]
    fn fuzzy_match_needs_a_specific_enough_name() {
        // 通用词不做模糊匹配，否则半个系统都会被当成残留
        let generic = Ctx::new(&app("App", "Microsoft"));
        assert!(!generic.name_is_matchable());
        assert!(!generic.name_matches("AppData"));

        let specific = Ctx::new(&app("同花顺", "浙江核新同花顺"));
        assert!(specific.name_is_matchable());
        assert!(specific.name_matches("同花顺"));
        assert!(specific.name_matches("同花顺远航版"));
        assert!(!specific.name_matches("Steam"));
    }

    #[test]
    fn install_dir_containment_respects_path_boundaries() {
        let c = ctx_with_dir("Foo", r"c:\program files\foo");
        assert!(c.under_install_dir(r"c:\program files\foo"));
        assert!(c.under_install_dir(r"c:\program files\foo\bin\a.exe"));
        // 不能把 foobar 误判成 foo 的子目录
        assert!(!c.under_install_dir(r"c:\program files\foobar"));
    }

    #[test]
    fn mentions_install_dir_is_case_and_separator_insensitive() {
        let c = ctx_with_dir("Foo", r"c:\program files\foo");
        assert!(c.mentions_install_dir(r#""C:\Program Files\Foo\app.exe" --run"#));
        assert!(c.mentions_install_dir("C:/Program Files/Foo/app.exe"));
        assert!(!c.mentions_install_dir(r"C:\Program Files\Other\app.exe"));
    }

    /// 没有安装目录时，凡是依赖路径匹配的扫描器都必须直接放弃，
    /// 否则 `contains("")` 会对任意文本返回 true，把整个注册表当成残留。
    #[test]
    fn empty_install_dir_never_matches_anything() {
        let c = Ctx::new(&app("Foo", "Bar"));
        assert!(c.install_dir.is_empty());
        assert!(!c.mentions_install_dir(r"C:\anything\at\all"));
        assert!(!c.under_install_dir(r"c:\anything"));
    }

    #[test]
    fn dedup_keeps_the_higher_confidence_record() {
        let k = ResidualKind::RegistryKey(AppRegRoot::Hklm, r"SOFTWARE\Foo".into());
        let mut items = vec![
            ResidualItem::possible(k.clone(), "疑似配置注册表项"),
            ResidualItem::certain(k.clone(), "配置注册表项"),
        ];
        dedup_items(&mut items);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].confidence, Confidence::Certain);
        assert_eq!(items[0].source, "配置注册表项");
    }

    #[test]
    fn uninstall_entry_is_not_invented_when_key_is_absent() {
        let mut a = app("Foo", "Bar");
        a.registry_subpath = r"SOFTWARE\NoSuchKey\QuickCleanerTest".into();
        let mut items = Vec::new();
        scan_uninstall_entry(&a, &mut items);
        assert!(items.is_empty());
    }

    /// 真机自检：拿一个真实安装的软件跑全套扫描器，确认能扫出东西。
    /// 旧实现在这台机器上对很多软件都报 0 项。
    #[test]
    fn live_scan_finds_the_uninstall_entry() {
        use std::sync::atomic::AtomicBool;
        let live = AtomicBool::new(true);
        let apps = crate::platform::windows::apps::list_installed_apps(&live);
        let Some(target) = apps.iter().find(|a| !a.registry_subpath.is_empty()) else {
            return; // 没有已安装软件，跳过
        };
        let res = scan_residuals(target);
        assert!(
            res.items
                .iter()
                .any(|i| i.source == "卸载登记项" && i.confidence == Confidence::Certain),
            "「{}」的卸载登记项没被识别出来",
            target.name
        );
    }
}

#[cfg(test)]
mod probe {
    use super::*;
    use std::sync::atomic::AtomicBool;

    /// 手动跑：打印某个软件扫出的全部残留。
    /// `cargo test --lib probe_residuals -- --ignored --nocapture 2>&1`
    #[test]
    #[ignore]
    fn probe_residuals() {
        let keyword = std::env::var("QC_APP").unwrap_or_else(|_| "同花顺".into());
        let live = AtomicBool::new(true);
        let apps = crate::platform::windows::apps::list_installed_apps(&live);
        let Some(app) = apps.iter().find(|a| a.name.contains(&keyword)) else {
            println!("未找到包含「{keyword}」的软件");
            return;
        };
        println!("软件: {} | 安装目录: {:?}", app.name, app.install_location);
        let res = scan_residuals(app);
        println!("共 {} 项（确定 {} 项）:", res.items.len(), res.certain_count());
        for it in &res.items {
            println!("  [{}][{}] {}", it.confidence.label(), it.source, it.kind.display_label());
        }
    }
}
