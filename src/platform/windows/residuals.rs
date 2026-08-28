//! Windows 软件残留（文件与注册表）深度探测与清理
//!
//! 扫描位置参照开源的 Bulk Crap Uninstaller 的 junk finder 清单整理而来
//! （`Junk/Finders/Registry`、`Misc`、`Drive`），关键注册表路径逐个核对过。
//! 在 BCU 那一套之上补了 COM/右键菜单、计划任务、Prefetch、崩溃转储，
//! 清理前会结束占用安装目录的进程并尝试停止对应服务。
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
    is_safe_app_token, split_command, AppRegRoot, Confidence, InstalledApp, ResidualItem,
    ResidualKind, ResidualScanResult, ResidualSource,
};
use crate::core::cleaner::{CleanFailure, CleanProgress, CleanReport};
use crate::core::safety::{is_protected_residual_path, is_system_root_dir};
use crate::platform::windows::apps::dir_or_file_size;
use crate::platform::windows::registry::{
    delete_reg_tree, delete_reg_value, enum_string_values, enum_subkeys, from_wide,
    read_reg_string, to_wide,
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

/// 公共目录 / 骨架文件夹的归一化名。拿它们做反向包含匹配会把桌面、
/// 开始菜单根目录整棵当成某软件的残留。
fn is_generic_folder_name(normed: &str) -> bool {
    matches!(
        normed,
        "desktop"
            | "documents"
            | "downloads"
            | "pictures"
            | "videos"
            | "music"
            | "public"
            | "users"
            | "programs"
            | "startup"
            | "startmenu"
            | "windows"
            | "system"
            | "system32"
            | "temp"
            | "tmp"
            | "appdata"
            | "programdata"
            | "programfiles"
            | "programfilesx86"
            | "commonfiles"
            | "common"
            | "shared"
    )
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
    ///
    /// 只认两种方向：候选包含完整软件名（`同花顺远航版` 对上 `同花顺`），
    /// 或软件名包含候选——但候选必须够长，且不是 Desktop / Programs 这种
    /// 公共目录名。否则 `Remote Desktop Manager` 会把整个
    /// `C:\Users\Public\Desktop` 报成残留。
    fn name_matches(&self, candidate: &str) -> bool {
        if !self.name_is_matchable() {
            return false;
        }
        let c = norm(candidate);
        let c_len = c.chars().count();
        if c_len < 3 {
            return false;
        }
        if c.contains(&self.name_norm) {
            return true;
        }
        self.name_norm.contains(&c)
            && c_len * 2 >= self.name_norm.chars().count()
            && !is_generic_folder_name(&c)
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

    for v in [&app.display_icon, &app.uninstall_string]
        .into_iter()
        .flatten()
    {
        let clean = v.split(',').next().unwrap_or("").trim_matches('"').trim();
        if let Some(f) = Path::new(clean).file_name().and_then(|f| f.to_str()) {
            push(f, &mut out);
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
    // SAFETY: wide 以 NUL 结尾且活到调用结束。只是探测键存不存在，
    // 打开成功后立刻关闭。
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
    // SAFETY: 同 reg_key_exists；读值本身走的是已经封装好的
    // `registry::read_reg_string`，句柄在返回前关闭。
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
    scan_com(&ctx, &mut items);
    scan_scheduled_tasks(&ctx, &mut items);
    scan_prefetch(&ctx, &mut items);
    scan_crash_dumps(&ctx, &mut items);
    scan_uninstaller_leftover(app, &ctx, &mut items);

    dedup_items(&mut items);
    // 「确定」的排在前面，用户先看到的就是可以放心删的
    items.sort_by_key(|b| std::cmp::Reverse(b.confidence));

    let total_file_size = items.iter().map(|i| i.size()).sum();
    ResidualScanResult {
        app_name: app.name.clone(),
        app_id: app.id.clone(),
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
            ResidualSource::UninstallEntry,
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
    out.push(ResidualItem::certain(kind, ResidualSource::InstallDir));
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
                ResidualSource::EmptyInstallParent,
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
                    ResidualSource::VendorRegKey,
                ));
                continue; // 命中就不再往下钻，整棵删掉即可
            }
            for lvl2 in enum_subkeys(h, &p1, sam).into_iter().take(200) {
                let p2 = format!(r"{p1}\{lvl2}");
                if values_mention(h, &p2, sam, ctx) {
                    out.push(ResidualItem::certain(
                        ResidualKind::RegistryKey(reg_root, p2),
                        ResidualSource::VendorRegKey,
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
    roots.extend(
        [dirs::data_dir(), dirs::data_local_dir()]
            .into_iter()
            .flatten(),
    );
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
                push_dir(out, p, Confidence::Certain, ResidualSource::AppDataDir);
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
                    push_dir(
                        out,
                        p,
                        Confidence::Possible,
                        ResidualSource::LikelyAppDataDir,
                    );
                }
            }
        }
    }
}

/// 开始菜单、桌面、启动文件夹里的快捷方式。
///
/// 厂商常把快捷方式放在「开始菜单\厂商名\产品.lnk」，只扫一层会漏。
fn scan_shortcuts(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    if !ctx.name_is_matchable() {
        return;
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Desktop"));
        roots.push(home.join(r"AppData\Roaming\Microsoft\Windows\Start Menu\Programs"));
        roots.push(home.join(r"AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup"));
    }
    if let Some(roaming) = dirs::data_dir() {
        roots.push(roaming.join(r"Microsoft\Windows\Start Menu\Programs"));
        roots.push(roaming.join(r"Microsoft\Windows\Start Menu\Programs\Startup"));
    }
    if let Ok(pd) = std::env::var("ProgramData") {
        let pd = PathBuf::from(pd);
        roots.push(pd.join(r"Microsoft\Windows\Start Menu\Programs"));
        roots.push(pd.join(r"Microsoft\Windows\Start Menu\Programs\Startup"));
    }
    if let Ok(public) = std::env::var("PUBLIC") {
        roots.push(PathBuf::from(public).join("Desktop"));
    }

    let mut seen = std::collections::HashSet::new();
    for root in roots {
        if !seen.insert(crate::core::safety::norm(&root)) {
            continue;
        }
        // min_depth(1)：搜索根本身（桌面、开始菜单\Programs）不是残留。
        // WalkDir 默认会把起点也枚举出来，`Public\Desktop` 的文件名是
        // Desktop，会被「Remote Desktop Manager」这种名字误伤。
        for entry in walkdir::WalkDir::new(&root)
            .min_depth(1)
            .max_depth(4)
            .into_iter()
            .flatten()
            .take(4000)
        {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let stem = name.trim_end_matches(".lnk");
            if !ctx.name_matches(stem) {
                continue;
            }
            let p = entry.path();
            if is_protected_residual_path(p) {
                continue;
            }
            let conf = if norm(stem) == ctx.name_norm {
                Confidence::Certain
            } else {
                Confidence::Possible
            };
            if entry.file_type().is_dir() {
                push_dir(out, p.to_path_buf(), conf, ResidualSource::StartMenuDir);
            } else if name.to_ascii_lowercase().ends_with(".lnk") {
                let size = dir_or_file_size(p);
                out.push(ResidualItem {
                    kind: ResidualKind::File(p.to_path_buf(), size),
                    confidence: conf,
                    source: ResidualSource::Shortcut,
                    identity: crate::core::model::capture_identity(p),
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
            direct.push(format!(
                "{base}\\{}\\{}",
                ctx.publisher_token, ctx.name_token
            ));
        }
        for sub in direct {
            if reg_key_exists(h, &sub, sam) {
                out.push(ResidualItem::certain(
                    ResidualKind::RegistryKey(reg_root, sub),
                    ResidualSource::ConfigRegKey,
                ));
            }
        }

        // 模糊：枚举一层子键
        for sub in enum_subkeys(h, base, sam) {
            if sub.eq_ignore_ascii_case(&ctx.name_token) || sub.eq_ignore_ascii_case("WOW6432Node")
            {
                continue;
            }
            if ctx.name_matches(&sub) {
                out.push(ResidualItem::possible(
                    ResidualKind::RegistryKey(reg_root, format!("{base}\\{sub}")),
                    ResidualSource::LikelyConfigRegKey,
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
                    ResidualSource::AppPathsEntry,
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
                    source: ResidualSource::StartupEntry,
                    identity: None,
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
                ResidualSource::Service,
            ));
        } else if !image.is_empty() && ctx.name_matches(&svc) {
            out.push(ResidualItem::possible(
                ResidualKind::RegistryKey(AppRegRoot::Hklm, full),
                ResidualSource::LikelyService,
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
                ResidualSource::FirewallRule,
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
                ResidualSource::RasTrace,
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
                ResidualSource::LeakDiagnostics,
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
                        ResidualSource::CompatSetting,
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
                ResidualSource::InstallerFolderEntry,
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
                ResidualSource::ProgramNameCache,
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
                ResidualSource::DefaultProgramsEntry,
            ));
        }
    }
}

/// COM 类、TypeLib、以及指向这些 CLSID 的右键菜单 / 外壳扩展。
///
/// 对齐 BCU 的 `ComScanner`：只认 InprocServer32 / LocalServer32 / TypeLib
/// 文件路径落在安装目录里的条目，不靠名字模糊匹配一整棵 HKCR。
fn scan_com(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    if ctx.install_dir.is_empty() {
        return;
    }
    let mut guids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let classes = [
        (
            AppRegRoot::Hklm,
            r"SOFTWARE\Classes",
            KEY_READ | KEY_WOW64_64KEY,
        ),
        (
            AppRegRoot::Hklm,
            r"SOFTWARE\Classes\WOW6432Node",
            KEY_READ | KEY_WOW64_64KEY,
        ),
        (AppRegRoot::Hkcu, r"Software\Classes", KEY_READ),
    ];
    for (root, base, sam) in classes {
        scan_clsid_key(root, &format!(r"{base}\CLSID"), sam, ctx, out, &mut guids);
        scan_typelib_key(root, &format!(r"{base}\TypeLib"), sam, ctx, out, &mut guids);
    }
    if guids.is_empty() {
        return;
    }
    scan_shell_extensions(&guids, out);
}

fn is_system_clsid(guid: &str) -> bool {
    let g = guid.trim();
    g.is_empty() || !g.starts_with('{') || g.contains("-0000-")
}

fn expand_env_path(raw: &str) -> String {
    use winapi::um::processenv::ExpandEnvironmentStringsW;
    let trimmed = raw.trim().trim_matches('"');
    if trimmed.is_empty() {
        return String::new();
    }
    let wide = to_wide(trimmed);
    let mut buf = [0u16; 1024];
    // SAFETY: wide / buf 都是本地缓冲，长度如实上报。
    let n = unsafe { ExpandEnvironmentStringsW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if n == 0 || (n as usize) > buf.len() {
        return trimmed.to_ascii_lowercase().replace('/', "\\");
    }
    from_wide(&buf[..n as usize])
        .to_ascii_lowercase()
        .replace('/', "\\")
}

fn is_windows_system_file(path: &str) -> bool {
    path.contains(r"\windows\system32\")
        || path.contains(r"\windows\syswow64\")
        || path.contains(r"\windows\winsxs\")
}

fn scan_clsid_key(
    root: AppRegRoot,
    clsid_path: &str,
    sam: DWORD,
    ctx: &Ctx,
    out: &mut Vec<ResidualItem>,
    guids: &mut std::collections::HashSet<String>,
) {
    let h = hkey_of(root);
    for guid in enum_subkeys(h, clsid_path, sam).into_iter().take(8000) {
        if is_system_clsid(&guid) {
            continue;
        }
        let guid_key = format!(r"{clsid_path}\{guid}");
        let mut server =
            open_and_read(h, &format!(r"{guid_key}\InprocServer32"), "", sam).unwrap_or_default();
        if server.is_empty() {
            server = open_and_read(h, &format!(r"{guid_key}\LocalServer32"), "", sam)
                .unwrap_or_default();
        }
        if server.is_empty() {
            continue;
        }
        let path = expand_env_path(&server);
        if path.is_empty() || is_windows_system_file(&path) || !ctx.mentions_install_dir(&path) {
            continue;
        }
        guids.insert(guid.to_ascii_lowercase());
        out.push(ResidualItem::certain(
            ResidualKind::RegistryKey(root, guid_key.clone()),
            ResidualSource::ComClass,
        ));
        if let Some(progid) = open_and_read(h, &format!(r"{guid_key}\ProgID"), "", sam) {
            let progid = progid.trim().to_string();
            if progid.len() >= 4 && progid.contains('.') {
                let classes = clsid_path.trim_end_matches("\\CLSID");
                let prog_path = format!(r"{classes}\{progid}");
                if reg_key_exists(h, &prog_path, sam) {
                    out.push(ResidualItem::certain(
                        ResidualKind::RegistryKey(root, prog_path),
                        ResidualSource::ComClass,
                    ));
                }
            }
        }
    }
}

fn scan_typelib_key(
    root: AppRegRoot,
    typelib_path: &str,
    sam: DWORD,
    ctx: &Ctx,
    out: &mut Vec<ResidualItem>,
    guids: &mut std::collections::HashSet<String>,
) {
    let h = hkey_of(root);
    for guid in enum_subkeys(h, typelib_path, sam).into_iter().take(4000) {
        if is_system_clsid(&guid) {
            continue;
        }
        let guid_key = format!(r"{typelib_path}\{guid}");
        for ver in enum_subkeys(h, &guid_key, sam).into_iter().take(8) {
            for platform in ["win32", "win64"] {
                let file_key = format!(r"{guid_key}\{ver}\0\{platform}");
                let server = open_and_read(h, &file_key, "", sam).unwrap_or_default();
                if server.is_empty() {
                    continue;
                }
                let path = expand_env_path(&server);
                if path.is_empty()
                    || is_windows_system_file(&path)
                    || !ctx.mentions_install_dir(&path)
                {
                    continue;
                }
                guids.insert(guid.to_ascii_lowercase());
                out.push(ResidualItem::certain(
                    ResidualKind::RegistryKey(root, guid_key.clone()),
                    ResidualSource::ComClass,
                ));
                break;
            }
        }
    }
}

const SHELLEX_PARENTS: &[&str] = &[
    r"SOFTWARE\Classes\*\shellex\ContextMenuHandlers",
    r"SOFTWARE\Classes\*\shellex\PropertySheetHandlers",
    r"SOFTWARE\Classes\Directory\shellex\ContextMenuHandlers",
    r"SOFTWARE\Classes\Directory\Background\shellex\ContextMenuHandlers",
    r"SOFTWARE\Classes\Folder\shellex\ContextMenuHandlers",
    r"SOFTWARE\Classes\AllFilesystemObjects\shellex\ContextMenuHandlers",
    r"SOFTWARE\Classes\Drive\shellex\ContextMenuHandlers",
    r"SOFTWARE\Classes\lnkfile\shellex\ContextMenuHandlers",
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\ShellIconOverlayIdentifiers",
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Browser Helper Objects",
];

fn scan_shell_extensions(guids: &std::collections::HashSet<String>, out: &mut Vec<ResidualItem>) {
    let views = [
        (AppRegRoot::Hklm, KEY_READ | KEY_WOW64_64KEY),
        (AppRegRoot::Hklm32, KEY_READ | KEY_WOW64_32KEY),
        (AppRegRoot::Hkcu, KEY_READ),
    ];
    for (root, sam) in views {
        let h = hkey_of(root);
        for parent in SHELLEX_PARENTS {
            let parent_path = if root == AppRegRoot::Hkcu {
                parent.replacen("SOFTWARE", "Software", 1)
            } else {
                (*parent).to_string()
            };
            for sub in enum_subkeys(h, &parent_path, sam) {
                let full = format!(r"{parent_path}\{sub}");
                let def = open_and_read(h, &full, "", sam).unwrap_or_default();
                let hit = guid_in_set(guids, &sub) || guid_in_set(guids, &def);
                if hit {
                    out.push(ResidualItem::certain(
                        ResidualKind::RegistryKey(root, full),
                        ResidualSource::ShellExtension,
                    ));
                }
            }
        }
        let approved = if root == AppRegRoot::Hkcu {
            r"Software\Microsoft\Windows\CurrentVersion\Shell Extensions\Approved"
        } else {
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Shell Extensions\Approved"
        };
        for (name, _) in enum_string_values(h, approved, sam) {
            if guid_in_set(guids, &name) {
                out.push(ResidualItem::certain(
                    ResidualKind::RegistryValue(root, approved.to_string(), name),
                    ResidualSource::ShellExtension,
                ));
            }
        }
    }
}

fn guid_in_set(guids: &std::collections::HashSet<String>, raw: &str) -> bool {
    let t = raw.trim();
    if t.is_empty() {
        return false;
    }
    guids.contains(&t.to_ascii_lowercase())
}

/// 计划任务：读 `System32\Tasks` 下的 XML，看 Command 是否指向安装目录。
fn scan_scheduled_tasks(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    if ctx.install_dir.is_empty() && ctx.exe_names.is_empty() {
        return;
    }
    let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let tasks_root = PathBuf::from(sysroot).join(r"System32\Tasks");
    if !tasks_root.exists() {
        return;
    }
    for entry in walkdir::WalkDir::new(&tasks_root)
        .max_depth(6)
        .into_iter()
        .flatten()
        .take(4000)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = match entry.path().strip_prefix(&tasks_root) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let rel_str = rel.to_string_lossy().replace('/', "\\");
        if rel_str
            .to_ascii_lowercase()
            .starts_with(r"microsoft\windows\")
        {
            continue;
        }
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        let blob = task_xml_text(&bytes);
        let hit_dir = ctx.mentions_install_dir(&blob);
        let hit_exe = ctx.mentions_exe(&blob);
        if !hit_dir && !hit_exe {
            continue;
        }
        if !hit_dir && is_windows_system_file(&blob.to_ascii_lowercase()) {
            continue;
        }
        let name = format!(r"\{rel_str}");
        out.push(ResidualItem {
            kind: ResidualKind::ScheduledTask(name),
            confidence: if hit_dir {
                Confidence::Certain
            } else {
                Confidence::Possible
            },
            source: ResidualSource::ScheduledTask,
            identity: None,
        });
    }
}

fn task_xml_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let body = &bytes[2..];
        let u16s: Vec<u16> = body
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        String::from_utf16_lossy(&u16s)
    } else if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let body = &bytes[2..];
        let u16s: Vec<u16> = body
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        String::from_utf16_lossy(&u16s)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Prefetch：每次启动都会留下 `APP.EXE-HASH.pf`。
fn scan_prefetch(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    if ctx.exe_names.is_empty() {
        return;
    }
    let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let dir = PathBuf::from(sysroot).join("Prefetch");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return;
    };
    for ent in rd.flatten() {
        let name = ent.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(exe) = super::apps::prefetch_exe_name(name) else {
            continue;
        };
        if !ctx.exe_names.iter().any(|e| e.eq_ignore_ascii_case(&exe)) {
            continue;
        }
        let p = ent.path();
        if is_protected_residual_path(&p) {
            continue;
        }
        let size = dir_or_file_size(&p);
        out.push(ResidualItem::certain(
            ResidualKind::File(p, size),
            ResidualSource::PrefetchFile,
        ));
    }
}

/// `%LOCALAPPDATA%\CrashDumps\<exe>.*.dmp`
fn scan_crash_dumps(ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    if ctx.exe_names.is_empty() {
        return;
    }
    let Some(local) = dirs::data_local_dir() else {
        return;
    };
    let dir = local.join("CrashDumps");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return;
    };
    for ent in rd.flatten() {
        let name = ent.file_name();
        let Some(name) = name.to_str() else { continue };
        let lower = name.to_ascii_lowercase();
        if !lower.ends_with(".dmp") {
            continue;
        }
        if !ctx.exe_names.iter().any(|e| lower.starts_with(e.as_str())) {
            continue;
        }
        let p = ent.path();
        let size = dir_or_file_size(&p);
        out.push(ResidualItem::certain(
            ResidualKind::File(p, size),
            ResidualSource::CrashDump,
        ));
    }
}

/// Inno/NSIS 把卸载器放在安装目录以外时，安装目录清掉后这块还在。
fn scan_uninstaller_leftover(app: &InstalledApp, ctx: &Ctx, out: &mut Vec<ResidualItem>) {
    let Some(cmd) = app
        .quiet_uninstall_string
        .as_ref()
        .or(app.uninstall_string.as_ref())
    else {
        return;
    };
    if cmd.to_ascii_lowercase().contains("msiexec") {
        return;
    }
    let (exe, _) = split_command(cmd);
    if exe.is_empty() {
        return;
    }
    let p = Path::new(&exe);
    let Some(parent) = p.parent() else { return };
    if !parent.exists() || is_protected_residual_path(parent) || is_system_root_dir(parent) {
        return;
    }
    let parent_norm = crate::core::safety::norm(parent);
    if !ctx.install_dir.is_empty() && parent_norm == ctx.install_dir {
        return;
    }
    let fname = p
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let looks =
        fname.starts_with("unins") || fname.contains("uninstall") || fname.contains("uninst");
    if !looks {
        return;
    }
    push_dir(
        out,
        parent.to_path_buf(),
        Confidence::Certain,
        ResidualSource::UninstallerLeftover,
    );
}

// ---------------------------------------------------------------------------
// 小工具
// ---------------------------------------------------------------------------

fn push_dir(out: &mut Vec<ResidualItem>, path: PathBuf, conf: Confidence, source: ResidualSource) {
    let size = dir_or_file_size(&path);
    let identity = crate::core::model::capture_identity(&path);
    out.push(ResidualItem {
        kind: ResidualKind::Directory(path, size),
        confidence: conf,
        source,
        identity,
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
            ResidualKind::ScheduledTask(name) => scheduled_task_exists(name),
            // macOS 专用，Windows 侧的扫描器不会产出，但枚举是共用的。
            ResidualKind::SystemExtension(..) => false,
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

    // 先结束占用安装目录的进程、停掉即将删除的服务，否则删文件/服务键会失败。
    let lock_dirs: Vec<String> = items
        .iter()
        .filter_map(|it| match &it.kind {
            ResidualKind::Directory(p, _) => Some(crate::core::safety::norm(p)),
            ResidualKind::File(p, _) => p.parent().map(crate::core::safety::norm),
            _ => None,
        })
        .filter(|s| s.len() > 3)
        .collect();
    if !lock_dirs.is_empty() {
        let _ = crate::platform::windows::process::terminate_processes_under(&lock_dirs);
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    for item in items {
        if let ResidualKind::RegistryKey(_, subpath) = &item.kind {
            if item.source == ResidualSource::Service
                || item.source == ResidualSource::LikelyService
            {
                if let Some(name) = subpath.rsplit('\\').next() {
                    let _ = stop_windows_service(name);
                }
            }
        }
    }

    for item in items {
        if prog.cancelled() {
            break;
        }

        match &item.kind {
            ResidualKind::Directory(path, _) | ResidualKind::File(path, _) => {
                prog.note(path);
                // 残留走回收站，不永久删（与 macOS 侧同一条理由，见
                // `platform::macos::residuals::clean_residuals`）：判据是
                // 「这个 app 已经不在任何位置装着了」，判错的代价是活应用
                // 的配置与登录态，收益通常只有几十 MB。
                //
                // 注册表键值、计划任务、服务不走这条——它们没有回收站语义，
                // 维持原来的直接删除。
                let res = crate::core::cleaner::dispose(
                    path,
                    crate::core::cleaner::Disposal::RecycleBin,
                    prog,
                );
                report.record(path, res);
            }
            ResidualKind::RegistryKey(root, subpath) => {
                if delete_reg_tree(hkey_of(*root), subpath) {
                    report.ok += 1;
                } else {
                    report
                        .failed
                        .push(CleanFailure::Id(format!("{}\\{}", root.label(), subpath)));
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
                    report.failed.push(CleanFailure::Id(format!(
                        "{}\\{} → {}",
                        root.label(),
                        subpath,
                        name
                    )));
                }
            }
            ResidualKind::ScheduledTask(name) => {
                if delete_scheduled_task(name) {
                    report.ok += 1;
                } else {
                    report.failed.push(CleanFailure::Id(name.clone()));
                }
            }
            // macOS 专用，Windows 侧的扫描器不会产出。
            ResidualKind::SystemExtension(..) => {}
        }
    }

    report
}

fn scheduled_task_exists(name: &str) -> bool {
    let rel = name.trim_start_matches('\\').replace('/', "\\");
    if rel.is_empty() {
        return false;
    }
    let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    PathBuf::from(sys)
        .join(r"System32\Tasks")
        .join(rel)
        .exists()
}

fn delete_scheduled_task(name: &str) -> bool {
    use std::os::windows::process::CommandExt;
    let status = std::process::Command::new("schtasks.exe")
        .args(["/Delete", "/TN", name, "/F"])
        .creation_flags(winapi::um::winbase::CREATE_NO_WINDOW)
        .status();
    matches!(status, Ok(s) if s.success()) || !scheduled_task_exists(name)
}

fn stop_windows_service(name: &str) -> bool {
    use winapi::um::winsvc::{
        CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW, QueryServiceStatus,
        SC_MANAGER_CONNECT, SERVICE_CONTROL_STOP, SERVICE_QUERY_STATUS, SERVICE_STATUS,
        SERVICE_STOP, SERVICE_STOPPED,
    };
    let wide = to_wide(name);
    // SAFETY: SCM / 服务句柄只在非空时使用，函数出口关闭。
    unsafe {
        let scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        if scm.is_null() {
            return false;
        }
        let svc = OpenServiceW(scm, wide.as_ptr(), SERVICE_STOP | SERVICE_QUERY_STATUS);
        if svc.is_null() {
            CloseServiceHandle(scm);
            return false;
        }
        let mut status: SERVICE_STATUS = std::mem::zeroed();
        let _ = ControlService(svc, SERVICE_CONTROL_STOP, &mut status);
        for _ in 0..20 {
            if QueryServiceStatus(svc, &mut status) != 0 && status.dwCurrentState == SERVICE_STOPPED
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        CloseServiceHandle(svc);
        CloseServiceHandle(scm);
        true
    }
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

        // 短名对长名：候选是软件名的实质前缀，仍然认
        let long = Ctx::new(&app("同花顺远航版", "浙江核新同花顺"));
        assert!(long.name_matches("同花顺"));
    }

    #[test]
    fn remote_desktop_manager_does_not_claim_desktop() {
        let app = Ctx::new(&app("Remote Desktop Manager", "Devolutions"));
        assert!(app.name_is_matchable());
        assert!(app.name_matches("Remote Desktop Manager"));
        assert!(app.name_matches("Remote Desktop Manager 2024"));
        assert!(!app.name_matches("Desktop"));
        assert!(!app.name_matches("Remote"));
        assert!(!app.name_matches("Manager"));
        assert!(!app.name_matches("Public"));
        assert!(!app.name_matches("Programs"));
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
            ResidualItem::possible(k.clone(), ResidualSource::LikelyConfigRegKey),
            ResidualItem::certain(k.clone(), ResidualSource::ConfigRegKey),
        ];
        dedup_items(&mut items);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].confidence, Confidence::Certain);
        assert_eq!(items[0].source, ResidualSource::ConfigRegKey);
    }

    #[test]
    fn system_clsid_filter() {
        assert!(is_system_clsid(""));
        assert!(is_system_clsid("NotAGuid"));
        assert!(is_system_clsid("{00000000-0000-0000-0000-000000000000}"));
        assert!(!is_system_clsid("{12345678-1234-1234-1234-1234567890AB}"));
    }

    #[test]
    fn task_xml_reads_utf16_le() {
        let mut bytes = vec![0xFF, 0xFE];
        for c in "<Command>C:\\App\\foo.exe</Command>".encode_utf16() {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        let text = task_xml_text(&bytes);
        assert!(text.contains(r"C:\App\foo.exe"));
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
                .any(|i| i.source == ResidualSource::UninstallEntry
                    && i.confidence == Confidence::Certain),
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
        println!(
            "共 {} 项（确定 {} 项）:",
            res.items.len(),
            res.certain_count()
        );
        for it in &res.items {
            println!(
                "  [{}][{}] {}",
                it.confidence.label(),
                it.source.label(),
                it.kind.display_label()
            );
        }
    }
}
