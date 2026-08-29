//! 软件管理与残留清理数据模型与核心排序过滤

use crate::core::i18n::Language;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::PathBuf;

/// 软件所属注册表根分支
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppRegRoot {
    Hklm,
    Hklm32,
    Hkcu,
    SystemApp,
}

impl AppRegRoot {
    /// 注册表根的**语言无关**写法，用来拼注册表路径（`HKLM\Software\…`）。
    ///
    /// 以前这里返回的是 `label_lang(Zh)`，于是英文界面下的残留路径会显示成
    /// `HKLM (64位)\Software\…`。给人看的带修饰说明请用 [`label_lang`]。
    ///
    /// [`label_lang`]: AppRegRoot::label_lang
    pub fn label(&self) -> &'static str {
        match self {
            AppRegRoot::Hklm => "HKLM",
            AppRegRoot::Hklm32 => "HKLM32",
            AppRegRoot::Hkcu => "HKCU",
            AppRegRoot::SystemApp => "SystemApp",
        }
    }

    pub fn label_lang(&self, lang: Language) -> &'static str {
        match lang {
            Language::Zh => match self {
                AppRegRoot::Hklm => "HKLM (64位)",
                AppRegRoot::Hklm32 => "HKLM (32位)",
                AppRegRoot::Hkcu => "HKCU (当前用户)",
                AppRegRoot::SystemApp => "系统/UWP",
            },
            Language::En => match self {
                AppRegRoot::Hklm => "HKLM (64-bit)",
                AppRegRoot::Hklm32 => "HKLM (32-bit)",
                AppRegRoot::Hkcu => "HKCU (Current User)",
                AppRegRoot::SystemApp => "System / UWP",
            },
        }
    }
}

/// 已安装软件信息模型
#[derive(Clone, Debug)]
pub struct InstalledApp {
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher: String,
    /// 最后一次运行/使用时间 (如 "2026-08-15" 或 "从未使用")
    pub last_used_date: Option<String>,
    /// 最后使用时间戳 (Unix 秒数，0 代表未记录/从未使用)
    pub last_used_raw: u64,
    pub install_date: Option<String>,
    pub install_date_raw: u64,
    pub install_location: Option<PathBuf>,
    pub display_icon: Option<String>,
    pub uninstall_string: Option<String>,
    pub quiet_uninstall_string: Option<String>,
    pub estimated_size: u64,
    pub registry_root: AppRegRoot,
    pub registry_subpath: String,
    pub is_system_component: bool,
    /// 卸载命令指向的可执行文件已经不存在。
    ///
    /// 这类软件在「程序和功能」里点卸载同样会失败，是真正意义上的
    /// 「无官方卸载器」，只能靠强力清理。枚举时算一次，避免渲染时反复
    /// 碰磁盘。
    pub uninstaller_missing: bool,
}

impl InstalledApp {
    /// 该软件是否支持常规卸载操作。
    ///
    /// - macOS: 非系统组件应用均支持卸载（移入废纸篓或调用自带卸载程序）。
    /// - Windows: 拥有有效的卸载命令行（且卸载器文件存在）的应用支持常规卸载。
    pub fn can_uninstall(&self) -> bool {
        if self.is_system_component {
            return false;
        }
        #[cfg(target_os = "macos")]
        {
            true
        }
        #[cfg(not(target_os = "macos"))]
        {
            (self.uninstall_string.is_some() || self.quiet_uninstall_string.is_some())
                && !self.uninstaller_missing
        }
    }

    /// 用来取图标的路径。
    ///
    /// Windows 注册表 `DisplayIcon` 指向 exe/dll/ico（常带 `,0` 资源下标），
    /// 比安装目录更准。其它平台用 `.app` 安装路径。
    pub fn icon_cache_key(&self) -> Option<PathBuf> {
        if let Some(icon) = self.display_icon.as_deref() {
            if let Some(path) = display_icon_path(icon) {
                return Some(path);
            }
        }
        self.install_location.clone()
    }
}

/// 去掉 DisplayIcon 末尾的 `,0` / `,-1` 资源下标，并剥掉引号。
pub fn display_icon_path(raw: &str) -> Option<PathBuf> {
    let s = raw.trim().trim_matches('"');
    if s.is_empty() {
        return None;
    }
    let without_index = if let Some(comma) = s.rfind(',') {
        let suffix = s[comma + 1..].trim();
        if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit() || c == '-') {
            s[..comma].trim().trim_matches('"')
        } else {
            s
        }
    } else {
        s
    };
    let p = without_index.trim();
    if p.is_empty() {
        None
    } else {
        Some(PathBuf::from(p))
    }
}

/// 排序字段
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppSortColumn {
    Name,
    Publisher,
    LastUsed,
    InstallDate,
    Size,
}

impl AppSortColumn {
    /// 列的稳定标识，用作表头元素 ID。
    ///
    /// 表头**显示**的文字走 `ui::i18n` 的 `tr_th_*`，不从这里取——ID 必须
    /// 与语言无关，否则切一次语言，GPUI 眼里的元素就换了一个。
    pub fn id(&self) -> &'static str {
        match self {
            AppSortColumn::Name => "name",
            AppSortColumn::Publisher => "publisher",
            AppSortColumn::LastUsed => "last-used",
            AppSortColumn::InstallDate => "install-date",
            AppSortColumn::Size => "size",
        }
    }
}

/// 排序方向
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// 表格排序状态
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppSortState {
    pub column: AppSortColumn,
    pub direction: SortDirection,
}

impl Default for AppSortState {
    fn default() -> Self {
        Self {
            column: AppSortColumn::Size,
            direction: SortDirection::Descending,
        }
    }
}

impl AppSortState {
    pub fn new(column: AppSortColumn, direction: SortDirection) -> Self {
        Self { column, direction }
    }

    /// 点击某列切换排序状态：相同列翻转方向，不同列切换并设置合理默认方向
    pub fn toggle(&mut self, col: AppSortColumn) {
        if self.column == col {
            self.direction = match self.direction {
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::Ascending,
            };
        } else {
            self.column = col;
            self.direction = match col {
                AppSortColumn::Name | AppSortColumn::Publisher => SortDirection::Ascending,
                AppSortColumn::Size | AppSortColumn::LastUsed | AppSortColumn::InstallDate => {
                    SortDirection::Descending
                }
            };
        }
    }

    /// 获取某列的指示图标（仅在当前排序列展示）
    pub fn indicator(&self, col: AppSortColumn) -> &'static str {
        if self.column != col {
            ""
        } else {
            match self.direction {
                SortDirection::Ascending => " ▲",
                SortDirection::Descending => " ▼",
            }
        }
    }
}

/// 快速分类预设
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppFilterPreset {
    All,
    Large,
    Unused,
    Orphan,
}

/// 「长期未用」的判定门槛：从来没被记录过使用，或者最后一次使用距今超过 90 天。
pub const RARELY_USED_SECS: u64 = 90 * 86400;

/// 当前 Unix 时间戳（秒）。系统时间异常时退化成 0，此时所有软件都算「长期未用」，
/// 与「没有使用记录」的处理保持一致。
pub fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 该软件是否属于「长期未用」。统计卡片与快速分类共用这一套判定，避免两处口径不一致。
pub fn is_rarely_used(app: &InstalledApp, now_secs: u64) -> bool {
    app.last_used_raw == 0 || now_secs.saturating_sub(app.last_used_raw) > RARELY_USED_SECS
}

impl AppFilterPreset {
    pub const ALL: [AppFilterPreset; 4] = [
        AppFilterPreset::All,
        AppFilterPreset::Large,
        AppFilterPreset::Unused,
        AppFilterPreset::Orphan,
    ];

    /// 中文文案。**仅供日志与命令行**——界面上一律用 `label_lang(lang)`，
    /// 否则英文模式下会漏出中文。
    pub fn label(&self) -> &'static str {
        self.label_lang(Language::Zh)
    }

    pub fn label_lang(&self, lang: Language) -> &'static str {
        match lang {
            Language::Zh => match self {
                AppFilterPreset::All => "全部软件",
                AppFilterPreset::Large => "大型软件 (>500MB)",
                AppFilterPreset::Unused => "长期未用 (>90天)",
                AppFilterPreset::Orphan => "卸载器失效",
            },
            Language::En => match self {
                AppFilterPreset::All => "All Apps",
                AppFilterPreset::Large => "Large Apps (>500MB)",
                AppFilterPreset::Unused => "Rarely Used (>90d)",
                AppFilterPreset::Orphan => "Invalid Uninstaller",
            },
        }
    }

    /// 某个软件是否落在该预设分类里。
    pub fn matches(&self, app: &InstalledApp, now_secs: u64) -> bool {
        match self {
            AppFilterPreset::All => true,
            AppFilterPreset::Large => app.estimated_size >= 500 * 1024 * 1024,
            AppFilterPreset::Unused => is_rarely_used(app, now_secs),
            // 光看「有没有卸载命令」没有意义——注册表里几乎每一项都有。
            // 真正需要关注的是命令跑不起来的：可执行文件已经没了。
            AppFilterPreset::Orphan => {
                (app.uninstall_string.is_none() && app.quiet_uninstall_string.is_none())
                    || app.uninstaller_missing
            }
        }
    }
}

/// 某一项残留与该软件关联的把握程度。
///
/// 决定它是否默认勾选。照搬 Bulk Crap Uninstaller 的思路：靠名字模糊匹配
/// 出来的东西不能和「明确指向安装目录」的东西一视同仁，否则一次误删就
/// 可能带走别的软件的数据。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    /// 名字相近，但没有指向该软件的硬证据。默认不勾选。
    Possible,
    /// 有明确证据：路径就是安装目录、注册表值直接指向安装目录、
    /// 或者本来就是该软件自己的卸载登记项。默认勾选。
    Certain,
}

impl Confidence {
    pub fn label(&self) -> &'static str {
        self.label_lang(Language::Zh)
    }

    pub fn label_lang(&self, lang: Language) -> &'static str {
        match lang {
            Language::Zh => match self {
                Confidence::Certain => "确定",
                Confidence::Possible => "可能",
            },
            Language::En => match self {
                Confidence::Certain => "Certain",
                Confidence::Possible => "Possible",
            },
        }
    }

    pub fn is_certain(&self) -> bool {
        *self == Confidence::Certain
    }
}

/// 关联残留项目分类
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResidualKind {
    File(PathBuf, u64),
    Directory(PathBuf, u64),
    /// 整个注册表键
    RegistryKey(AppRegRoot, String),
    /// 某个键下的单个值：(根, 键路径, 值名)
    ///
    /// 启动项、防火墙规则、App Paths 这些是以「值」的形式存在的，
    /// 删掉整个键会波及其它软件。
    RegistryValue(AppRegRoot, String, String),
    /// 计划任务。字符串是 Task Scheduler 路径，例如 `\Vendor\AppUpdate`。
    ///
    /// 任务文件在 `System32\Tasks` 下，整棵 System32 是保护目录，不能当
    /// 普通文件删，必须走 `schtasks /Delete`。
    ScheduledTask(String),
    /// macOS 系统扩展（DriverKit / 网络扩展）：`(teamID, bundleID)`。
    ///
    /// 磁盘上的 `.dext` 由 macOS 自己 staged 在 `/Library/SystemExtensions`
    /// 并登记在扩展数据库里，删文件既删不动（SIP）也只会留下不一致状态，
    /// 必须走 `systemextensionsctl` 或系统设置。
    SystemExtension(String, String),
}

impl ResidualKind {
    pub fn size(&self) -> u64 {
        match self {
            ResidualKind::File(_, s) | ResidualKind::Directory(_, s) => *s,
            ResidualKind::RegistryKey(..)
            | ResidualKind::RegistryValue(..)
            | ResidualKind::ScheduledTask(..)
            | ResidualKind::SystemExtension(..) => 0,
        }
    }

    /// 中文文案。**仅供日志与命令行**，界面上用 `kind_label_lang(lang)`。
    pub fn kind_label(&self) -> &'static str {
        self.kind_label_lang(Language::Zh)
    }

    pub fn kind_label_lang(&self, lang: Language) -> &'static str {
        match lang {
            Language::Zh => match self {
                ResidualKind::File(..) => "文件",
                ResidualKind::Directory(..) => "目录",
                ResidualKind::RegistryKey(..) => "注册表项",
                ResidualKind::RegistryValue(..) => "注册表值",
                ResidualKind::ScheduledTask(..) => "计划任务",
                ResidualKind::SystemExtension(..) => "系统扩展",
            },
            Language::En => match self {
                ResidualKind::File(..) => "File",
                ResidualKind::Directory(..) => "Directory",
                ResidualKind::RegistryKey(..) => "Registry Key",
                ResidualKind::RegistryValue(..) => "Registry Value",
                ResidualKind::ScheduledTask(..) => "Scheduled task",
                ResidualKind::SystemExtension(..) => "System extension",
            },
        }
    }

    pub fn display_label(&self) -> String {
        match self {
            ResidualKind::File(p, _) | ResidualKind::Directory(p, _) => {
                p.to_string_lossy().into_owned()
            }
            ResidualKind::RegistryKey(root, sub) => {
                format!("{}\\{}", root.label(), sub)
            }
            ResidualKind::RegistryValue(root, sub, name) => {
                format!("{}\\{} → {}", root.label(), sub, name)
            }
            ResidualKind::ScheduledTask(name) => name.clone(),
            ResidualKind::SystemExtension(_, bundle_id) => bundle_id.clone(),
        }
    }
}

/// 这条残留是被哪个扫描器发现的。
///
/// 以前直接存中文字符串，界面上的来源徽章因此在英文模式下也是中文；
/// 而且测试要拿字符串字面量去比对，改一个字就断。改成枚举后文案统一在
/// [`ResidualSource::label_lang`] 里翻译。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResidualSource {
    UninstallEntry,
    InstallDir,
    EmptyInstallParent,
    VendorRegKey,
    AppDataDir,
    LikelyAppDataDir,
    AppSupportDir,
    StartMenuDir,
    Shortcut,
    ConfigRegKey,
    LikelyConfigRegKey,
    AppPathsEntry,
    StartupEntry,
    Service,
    LikelyService,
    FirewallRule,
    RasTrace,
    LeakDiagnostics,
    CompatSetting,
    InstallerFolderEntry,
    ProgramNameCache,
    DefaultProgramsEntry,
    ComClass,
    ShellExtension,
    ScheduledTask,
    PrefetchFile,
    CrashDump,
    UninstallerLeftover,
    // macOS 专用
    CacheDir,
    LogDir,
    PreferenceFile,
    ContainerDir,
    ApplicationScript,
    RecentDocumentList,
    PackageReceipt,
    /// `LaunchAgents` 下的 plist：随用户登录启动，删前要先 `launchctl bootout`。
    LaunchAgent,
    /// `LaunchDaemons` / `PrivilegedHelperTools`：root 身份运行，需要提权才能删。
    LaunchDaemon,
    /// `~/.config` 等点目录下按名字命中的配置。
    DotConfigDir,
    /// 已激活的系统扩展。删不掉，只能引导用户去系统设置里关。
    SystemExtension,
    Other,
}

impl ResidualSource {
    pub fn label(&self) -> &'static str {
        self.label_lang(Language::Zh)
    }

    pub fn label_lang(&self, lang: Language) -> &'static str {
        match lang {
            Language::Zh => match self {
                ResidualSource::UninstallEntry => "卸载登记项",
                ResidualSource::InstallDir => "安装目录",
                ResidualSource::EmptyInstallParent => "空的安装父目录",
                ResidualSource::VendorRegKey => "厂商配置项",
                ResidualSource::AppDataDir => "应用数据目录",
                ResidualSource::LikelyAppDataDir => "疑似应用数据目录",
                ResidualSource::AppSupportDir => "应用支持目录",
                ResidualSource::StartMenuDir => "开始菜单目录",
                ResidualSource::Shortcut => "快捷方式",
                ResidualSource::ConfigRegKey => "配置注册表项",
                ResidualSource::LikelyConfigRegKey => "疑似配置注册表项",
                ResidualSource::AppPathsEntry => "App Paths 登记",
                ResidualSource::StartupEntry => "开机启动项",
                ResidualSource::Service => "服务",
                ResidualSource::LikelyService => "疑似服务",
                ResidualSource::FirewallRule => "防火墙规则",
                ResidualSource::RasTrace => "RAS 跟踪记录",
                ResidualSource::LeakDiagnostics => "内存泄漏诊断记录",
                ResidualSource::CompatSetting => "兼容性设置",
                ResidualSource::InstallerFolderEntry => "安装器目录登记",
                ResidualSource::ProgramNameCache => "程序名缓存",
                ResidualSource::DefaultProgramsEntry => "默认程序登记",
                ResidualSource::ComClass => "COM 组件",
                ResidualSource::ShellExtension => "右键菜单/外壳扩展",
                ResidualSource::ScheduledTask => "计划任务",
                ResidualSource::PrefetchFile => "Prefetch 预读取",
                ResidualSource::CrashDump => "崩溃转储",
                ResidualSource::UninstallerLeftover => "卸载器残骸",
                ResidualSource::CacheDir => "缓存目录",
                ResidualSource::LogDir => "日志目录",
                ResidualSource::PreferenceFile => "偏好设置文件",
                ResidualSource::ContainerDir => "沙盒容器",
                ResidualSource::ApplicationScript => "应用脚本目录",
                ResidualSource::RecentDocumentList => "最近使用记录",
                ResidualSource::PackageReceipt => "安装收据",
                ResidualSource::LaunchAgent => "登录启动项",
                ResidualSource::LaunchDaemon => "系统守护进程",
                ResidualSource::DotConfigDir => "配置目录",
                ResidualSource::SystemExtension => "系统扩展（需在系统设置中关闭）",
                ResidualSource::Other => "其他残留",
            },
            Language::En => match self {
                ResidualSource::UninstallEntry => "Uninstall entry",
                ResidualSource::InstallDir => "Install directory",
                ResidualSource::EmptyInstallParent => "Empty install parent",
                ResidualSource::VendorRegKey => "Vendor registry key",
                ResidualSource::AppDataDir => "App data directory",
                ResidualSource::LikelyAppDataDir => "Likely app data directory",
                ResidualSource::AppSupportDir => "Application Support directory",
                ResidualSource::StartMenuDir => "Start menu folder",
                ResidualSource::Shortcut => "Shortcut",
                ResidualSource::ConfigRegKey => "Config registry key",
                ResidualSource::LikelyConfigRegKey => "Likely config registry key",
                ResidualSource::AppPathsEntry => "App Paths entry",
                ResidualSource::StartupEntry => "Startup entry",
                ResidualSource::Service => "Service",
                ResidualSource::LikelyService => "Likely service",
                ResidualSource::FirewallRule => "Firewall rule",
                ResidualSource::RasTrace => "RAS tracing entry",
                ResidualSource::LeakDiagnostics => "Leak diagnostics entry",
                ResidualSource::CompatSetting => "Compatibility setting",
                ResidualSource::InstallerFolderEntry => "Installer folder entry",
                ResidualSource::ProgramNameCache => "Program name cache",
                ResidualSource::DefaultProgramsEntry => "Default programs entry",
                ResidualSource::ComClass => "COM class",
                ResidualSource::ShellExtension => "Shell extension",
                ResidualSource::ScheduledTask => "Scheduled task",
                ResidualSource::PrefetchFile => "Prefetch file",
                ResidualSource::CrashDump => "Crash dump",
                ResidualSource::UninstallerLeftover => "Uninstaller leftover",
                ResidualSource::CacheDir => "Cache directory",
                ResidualSource::LogDir => "Log directory",
                ResidualSource::PreferenceFile => "Preference file",
                ResidualSource::ContainerDir => "Sandbox container",
                ResidualSource::ApplicationScript => "Application scripts",
                ResidualSource::RecentDocumentList => "Recent document list",
                ResidualSource::PackageReceipt => "Package receipt",
                ResidualSource::LaunchAgent => "Launch agent",
                ResidualSource::LaunchDaemon => "Launch daemon",
                ResidualSource::DotConfigDir => "Dotfile config directory",
                ResidualSource::SystemExtension => "System extension (turn off in System Settings)",
                ResidualSource::Other => "Other residual",
            },
        }
    }
}

/// 一条残留记录：内容 + 把握程度 + 给用户看的来源说明。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResidualItem {
    pub kind: ResidualKind,
    pub confidence: Confidence,
    /// 这条是被哪个扫描器发现的
    pub source: ResidualSource,
    /// 扫描/卸载后复核时的文件系统身份。注册表、任务和系统扩展没有路径，
    /// 值为 `None`；真实路径清理时必须有快照且复验一致。
    pub identity: Option<crate::core::model::TargetIdentity>,
}

impl ResidualItem {
    pub fn certain(kind: ResidualKind, source: ResidualSource) -> Self {
        let identity = residual_identity(&kind);
        Self {
            kind,
            confidence: Confidence::Certain,
            source,
            identity,
        }
    }

    pub fn possible(kind: ResidualKind, source: ResidualSource) -> Self {
        let identity = residual_identity(&kind);
        Self {
            kind,
            confidence: Confidence::Possible,
            source,
            identity,
        }
    }

    pub fn size(&self) -> u64 {
        self.kind.size()
    }

    /// 中文文案。**仅供日志与命令行**，界面上用 `display_label_lang(lang)`。
    pub fn display_label(&self) -> String {
        self.display_label_lang(Language::Zh)
    }

    pub fn display_label_lang(&self, lang: Language) -> String {
        format!(
            "[{}] {}",
            self.source.label_lang(lang),
            self.kind.display_label()
        )
    }
}

fn residual_identity(kind: &ResidualKind) -> Option<crate::core::model::TargetIdentity> {
    match kind {
        ResidualKind::File(path, _) | ResidualKind::Directory(path, _) => {
            crate::core::model::capture_identity(path)
        }
        _ => None,
    }
}

/// 残留扫描时发现的「软件仍被占用」证据：运行中的进程、launchd 里仍
/// 登记的任务。只提示不阻断——活库删除有 cleaner 的 live-database 闸门
/// 兜底，但用户该在点「彻底清除」**之前**就知道为什么数据库类残留删不掉。
///
/// 实测案例（iStat Menus 7）：卸载后 gui 域的 KeepAlive 代理把进程反复
/// 拉起，HTTPStorages 里的「主库+伴随文件」每次清理都命中闸门，用户把
/// 同一个失败重试了五轮——缺的就是这块证据。
#[derive(Clone, Debug, Default)]
pub struct ResidualOccupancy {
    /// 仍在运行的进程（`pid 命令行摘要`）。
    pub processes: Vec<String>,
    /// launchd 用户域里仍登记**且未被禁用**的任务标签（无论此刻是否在跑
    /// ——登记着就随时可能被拉起）。已禁用的如实不报：不会自启，报了会
    /// 把「一切已收拾干净」的用户吓唬错。
    pub launchd_labels: Vec<String>,
}

impl ResidualOccupancy {
    pub fn is_occupied(&self) -> bool {
        !self.processes.is_empty() || !self.launchd_labels.is_empty()
    }
}

/// 关联残留深度扫描结果
#[derive(Clone, Debug, Default)]
pub struct ResidualScanResult {
    pub app_name: String,
    /// 对应 [`InstalledApp::id`]，清理完残留后用来从内存列表里拿掉这款软件。
    pub app_id: String,
    pub items: Vec<ResidualItem>,
    pub total_file_size: u64,
    /// 扫描时刻的进程/launchd 占用证据。空证据 = 没测到占用或当前平台
    /// 未实现探测，两种情况都不拦清理。
    pub occupancy: ResidualOccupancy,
}

/// 清理残留之后，这款软件还该不该留在「已安装」列表里。
///
/// 卸载登记项或安装目录已经不在剩余列表里，说明软件本身没了，只是列表
/// 还没刷新。只清了缓存/配置的，软件还在，要留着。
pub fn app_gone_after_residual_clean(
    original: &[ResidualItem],
    remaining: &[ResidualItem],
) -> bool {
    let gone = |src: ResidualSource| {
        original.iter().any(|i| i.source == src) && remaining.iter().all(|i| i.source != src)
    };
    gone(ResidualSource::UninstallEntry) || gone(ResidualSource::InstallDir)
}

/// 「彻底清除所选」之后对话框该怎么收尾。
///
/// `retry_items` 才会再弹一次：只有勾选了却没清掉的。用户没勾的项视为
/// 这次不处理，不能再弹第二次。`leftover_for_app` 仍包含未勾选项，用来
/// 判断软件是否还该留在已安装列表——没清就不能当成已经卸干净。
pub struct ResidualCleanFollowUp {
    pub retry_items: Vec<ResidualItem>,
    pub retry_selected: HashSet<usize>,
    pub leftover_for_app: Vec<ResidualItem>,
}

pub fn residual_clean_follow_up(
    items: &[ResidualItem],
    selected: &HashSet<usize>,
    still_present: impl Fn(&ResidualItem) -> bool,
) -> ResidualCleanFollowUp {
    let mut retry_selected = HashSet::new();
    let mut retry_items = Vec::new();
    let mut leftover_for_app = Vec::new();

    for (idx, item) in items.iter().enumerate() {
        if !selected.contains(&idx) {
            leftover_for_app.push(item.clone());
            continue;
        }
        if still_present(item) {
            retry_selected.insert(retry_items.len());
            leftover_for_app.push(item.clone());
            retry_items.push(item.clone());
        }
    }

    ResidualCleanFollowUp {
        retry_items,
        retry_selected,
        leftover_for_app,
    }
}

impl ResidualScanResult {
    /// 默认应当勾选的条目下标——只勾「确定」的。
    pub fn default_selection(&self) -> std::collections::HashSet<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, it)| it.confidence.is_certain())
            .map(|(i, _)| i)
            .collect()
    }

    pub fn certain_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.confidence.is_certain())
            .count()
    }
}

/// ASCII/Unicode 大小写不敏感的字典序比较，不分配。
///
/// 旧实现在比较器里对每个操作数调 `to_lowercase()`，`sort_by` 会调用比较器
/// O(n log n) 次，等于每次排序产生上万次临时 String。
fn cmp_ci(a: &str, b: &str) -> Ordering {
    let mut x = a.chars().flat_map(char::to_lowercase);
    let mut y = b.chars().flat_map(char::to_lowercase);
    loop {
        match (x.next(), y.next()) {
            (Some(p), Some(q)) => match p.cmp(&q) {
                Ordering::Equal => continue,
                other => return other,
            },
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
        }
    }
}

fn contains_ci(haystack: &str, needle_lower: &str) -> bool {
    haystack.to_lowercase().contains(needle_lower)
}

/// 时间戳排序用的键：0（无记录）在升序时要排到最后，而不是最前。
fn time_key_asc(raw: u64) -> u64 {
    if raw == 0 {
        u64::MAX
    } else {
        raw
    }
}

/// 预设过滤 + 关键词搜索 + 排序，返回 **`apps` 中的下标**。
///
/// 返回索引而不是 `Vec<InstalledApp>`：这个函数在界面渲染路径上，
/// 返回克隆意味着每帧要复制整张软件表（每个 `InstalledApp` 有 8 个
/// `String`/`PathBuf` 字段）。索引让调用方按需借用。
pub fn filter_and_sort_apps(
    apps: &[InstalledApp],
    preset: AppFilterPreset,
    search_keyword: &str,
    sort_state: AppSortState,
) -> Vec<usize> {
    let kw = search_keyword.trim().to_lowercase();
    let now_secs = now_unix_secs();
    let mut idx: Vec<usize> = apps
        .iter()
        .enumerate()
        .filter(|(_, app)| {
            preset.matches(app, now_secs)
                && (kw.is_empty()
                    || contains_ci(&app.name, &kw)
                    || contains_ci(&app.publisher, &kw)
                    || contains_ci(&app.id, &kw))
        })
        .map(|(i, _)| i)
        .collect();

    let AppSortState { column, direction } = sort_state;
    let desc = direction == SortDirection::Descending;

    // 排序列在循环外就确定了，比较器内部不再重复 match。
    idx.sort_by(|&a, &b| {
        let (a, b) = (&apps[a], &apps[b]);
        let ord = match column {
            AppSortColumn::Name => cmp_ci(&a.name, &b.name),
            AppSortColumn::Publisher => {
                cmp_ci(&a.publisher, &b.publisher).then_with(|| cmp_ci(&a.name, &b.name))
            }
            AppSortColumn::LastUsed => {
                if desc {
                    b.last_used_raw.cmp(&a.last_used_raw)
                } else {
                    time_key_asc(a.last_used_raw).cmp(&time_key_asc(b.last_used_raw))
                }
            }
            AppSortColumn::InstallDate => {
                if desc {
                    b.install_date_raw.cmp(&a.install_date_raw)
                } else {
                    time_key_asc(a.install_date_raw).cmp(&time_key_asc(b.install_date_raw))
                }
            }
            AppSortColumn::Size => a.estimated_size.cmp(&b.estimated_size),
        };

        // 时间列已经在上面按方向算好了（0 值的处理在两个方向上不对称），
        // 其余列在这里统一翻转。
        let ord = match column {
            AppSortColumn::LastUsed | AppSortColumn::InstallDate => ord,
            _ if desc => ord.reverse(),
            _ => ord,
        };
        ord.then_with(|| cmp_ci(&a.name, &b.name))
    });

    idx
}

/// 判定一个名称是否足够具体，以防误将公用根目录判定为残留
pub fn is_safe_app_token(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    if lower.len() < 3 {
        return false;
    }
    const BLACKLIST: &[&str] = &[
        "app",
        "apps",
        "microsoft",
        "windows",
        "system",
        "system32",
        "program files",
        "program files (x86)",
        "common files",
        "appdata",
        "local",
        "roaming",
        "locallow",
        "programdata",
        "temp",
        "tmp",
        "users",
        "google",
        "apple",
        "intel",
        "amd",
        "nvidia",
        "adobe",
        "tencent",
    ];
    !BLACKLIST.contains(&lower.as_str())
}

/// 把命令行拆成 (可执行文件, 参数)。
///
/// 注册表里的 `UninstallString` **经常不给路径加引号**，比如
/// `C:\Program Files\DAUM\PotPlayer\unins000.exe /SILENT`。按空格切会得到
/// `C:\Program`，于是卸载直接跑不起来——本机 145 款软件里有 24 款中招。
///
/// Windows 自己的处理办法是：从左往右逐段拼接，第一个「拼出来确实存在
/// 的**文件**」就是可执行文件，其余算参数。带引号的路径按引号切，最省事。
///
/// 这里必须认文件、不能认目录：`C:\Program Files (x86)\pdfcvt\uninstall.exe`
/// 的前缀 `C:\Program Files` 是真实存在的文件夹，当成 exe 去 `CreateProcess`
/// 会直接「拒绝访问」，卸载窗口永远弹不出来。
///
/// `is_file` 注入是为了可测试——生产用 [`split_command`]。
pub fn split_command_with(cmd: &str, is_file: impl Fn(&str) -> bool) -> (String, Vec<String>) {
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return (String::new(), Vec::new());
    }

    // 带引号：优先按引号边界切
    if let Some(rest) = cmd.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            let exe = &rest[..end];
            // 但有些厂商会把「程序 + 参数」整条塞进一对引号里
            // （联想应用商店就是 `"...\StoreUninstaller.exe /SLIENT"`）。
            // 判据是引号内容**是否以可执行扩展名结尾**：格式正确的路径
            // 一定以 .exe/.bat 之类收尾，把参数也包进来的则不会。
            // 只用「文件是否存在」判断不行——卸载器真的丢失时也会落到
            // 错误分支，把好好的路径切碎。
            if is_file(exe) || ends_with_executable_ext(exe) {
                return (exe.to_string(), parse_cmd_line(rest[end + 1..].trim()));
            }
        }
    }

    // 不带引号（或引号内混着参数）：逐段延长，直到拼出一个真实存在的文件
    let cmd = cmd.trim_matches('"');
    let tokens: Vec<&str> = cmd.split(' ').filter(|t| !t.is_empty()).collect();
    for take in 1..=tokens.len() {
        let candidate = tokens[..take].join(" ");
        if is_file(&candidate) {
            let args = tokens[take..].iter().map(|s| s.to_string()).collect();
            return (candidate, args);
        }
    }

    // 都不存在（例如 winget / powershell 这类靠 PATH 解析的命令），
    // 退回「第一段是命令」的常规解释
    let exe = tokens.first().copied().unwrap_or("").to_string();
    let args = tokens[1.min(tokens.len())..]
        .iter()
        .map(|s| s.to_string())
        .collect();
    (exe, args)
}

/// 是否以 Windows 可执行文件扩展名结尾。
fn ends_with_executable_ext(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    [".exe", ".com", ".bat", ".cmd", ".msi", ".scr", ".ps1"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// [`split_command_with`] 的生产版本：只认文件，目录不算命中。
pub fn split_command(cmd: &str) -> (String, Vec<String>) {
    split_command_with(cmd, |p| std::path::Path::new(p).is_file())
}

/// 解析命令行参数
pub fn parse_cmd_line(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for c in cmd.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    out.push(current);
                    current = String::new();
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// 解析注册表安装日期字符串，返回 (展示日期, 原始时间戳/数值)
pub fn parse_install_date(raw: Option<String>) -> (Option<String>, u64) {
    let Some(s) = raw else {
        return (None, 0);
    };
    let s = s.trim();
    if s.len() == 8 && s.chars().all(|c| c.is_ascii_digit()) {
        let y = &s[0..4];
        let m = &s[4..6];
        let d = &s[6..8];
        let num: u64 = s.parse().unwrap_or(0);
        return (Some(format!("{y}-{m}-{d}")), num);
    }
    if s.len() >= 8 {
        let num: u64 = s
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(0);
        return (Some(s.to_string()), num);
    }
    (Some(s.to_string()), 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_app(
        name: &str,
        publisher: &str,
        size: u64,
        date_raw: u64,
        last_used_raw: u64,
    ) -> InstalledApp {
        InstalledApp {
            id: name.to_string(),
            name: name.to_string(),
            version: "1.0".to_string(),
            publisher: publisher.to_string(),
            last_used_date: if last_used_raw > 0 {
                Some("2026-08-15".to_string())
            } else {
                None
            },
            last_used_raw,
            install_date: if date_raw > 0 {
                Some("2023-01-01".to_string())
            } else {
                None
            },
            install_date_raw: date_raw,
            install_location: None,
            display_icon: None,
            uninstall_string: Some("uninstall.exe".to_string()),
            quiet_uninstall_string: None,
            estimated_size: size,
            registry_root: AppRegRoot::Hklm,
            registry_subpath: String::new(),
            is_system_component: false,
            uninstaller_missing: false,
        }
    }

    /// 把索引结果映射回软件名，方便断言。
    fn names(apps: &[InstalledApp], idx: &[usize]) -> Vec<String> {
        idx.iter().map(|&i| apps[i].name.clone()).collect()
    }

    #[test]
    fn display_icon_path_strips_resource_index() {
        assert_eq!(
            super::display_icon_path(r#""C:\Program Files\App\app.exe",0"#).as_deref(),
            Some(std::path::Path::new(r"C:\Program Files\App\app.exe"))
        );
        assert_eq!(
            super::display_icon_path(r"C:\Program Files\App\app.exe,-1").as_deref(),
            Some(std::path::Path::new(r"C:\Program Files\App\app.exe"))
        );
        assert_eq!(
            super::display_icon_path(r"C:\Program Files\App\app.exe").as_deref(),
            Some(std::path::Path::new(r"C:\Program Files\App\app.exe"))
        );
        assert_eq!(super::display_icon_path("").as_deref(), None);
    }

    #[test]
    fn icon_cache_key_prefers_display_icon() {
        let mut app = create_test_app("Chrome", "Google", 1, 0, 0);
        app.install_location = Some(std::path::PathBuf::from(r"C:\Program Files\Google\Chrome"));
        app.display_icon = Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe,0".into());
        assert_eq!(
            app.icon_cache_key().as_deref(),
            Some(std::path::Path::new(
                r"C:\Program Files\Google\Chrome\Application\chrome.exe"
            ))
        );
    }

    #[test]
    fn icon_cache_key_falls_back_to_install_location() {
        let mut app = create_test_app("坚果云", "上海亦存网络科技有限公司", 1, 0, 0);
        app.install_location = Some(std::path::PathBuf::from(r"C:\Program Files\Nutstore\"));
        app.display_icon = None;
        assert_eq!(
            app.icon_cache_key().as_deref(),
            Some(std::path::Path::new(r"C:\Program Files\Nutstore\"))
        );
    }

    #[test]
    fn test_filter_and_sort_by_size() {
        let apps = vec![
            create_test_app("Chrome", "Google", 200 * 1024 * 1024, 20230101, 100),
            create_test_app("Steam", "Valve", 50 * 1024 * 1024, 20220101, 200),
            create_test_app("VSCode", "Microsoft", 500 * 1024 * 1024, 20230501, 300),
        ];

        let state = AppSortState::new(AppSortColumn::Size, SortDirection::Descending);
        let sorted = filter_and_sort_apps(&apps, AppFilterPreset::All, "", state);
        assert_eq!(names(&apps, &sorted), ["VSCode", "Chrome", "Steam"]);

        let state_asc = AppSortState::new(AppSortColumn::Size, SortDirection::Ascending);
        let sorted_asc = filter_and_sort_apps(&apps, AppFilterPreset::All, "", state_asc);
        assert_eq!(names(&apps, &sorted_asc), ["Steam", "Chrome", "VSCode"]);
    }

    #[test]
    fn test_sort_by_last_used() {
        let apps = vec![
            create_test_app("AppOld", "Pub", 100, 20200101, 1000),
            create_test_app("AppNever", "Pub", 100, 20200101, 0),
            create_test_app("AppRecent", "Pub", 100, 20200101, 5000),
        ];

        let desc = AppSortState::new(AppSortColumn::LastUsed, SortDirection::Descending);
        assert_eq!(
            names(
                &apps,
                &filter_and_sort_apps(&apps, AppFilterPreset::All, "", desc)
            ),
            ["AppRecent", "AppOld", "AppNever"]
        );

        // 升序时「从未使用」(raw = 0) 依然要沉到最后
        let asc = AppSortState::new(AppSortColumn::LastUsed, SortDirection::Ascending);
        assert_eq!(
            names(
                &apps,
                &filter_and_sort_apps(&apps, AppFilterPreset::All, "", asc)
            ),
            ["AppOld", "AppRecent", "AppNever"]
        );
    }

    #[test]
    fn test_search_filter() {
        let apps = vec![
            create_test_app("Chrome", "Google", 200, 0, 0),
            create_test_app("Firefox", "Mozilla", 300, 0, 0),
            create_test_app("Visual Studio", "Microsoft", 500, 0, 0),
        ];

        let state = AppSortState::new(AppSortColumn::Name, SortDirection::Ascending);
        assert_eq!(
            names(
                &apps,
                &filter_and_sort_apps(&apps, AppFilterPreset::All, "fox", state)
            ),
            ["Firefox"]
        );
        // 关键词也匹配开发者字段
        assert_eq!(
            names(
                &apps,
                &filter_and_sort_apps(&apps, AppFilterPreset::All, "micro", state)
            ),
            ["Visual Studio"]
        );
    }

    #[test]
    fn test_preset_filter_is_applied() {
        let mut big = create_test_app("Big", "Pub", 600 * 1024 * 1024, 0, 0);
        big.uninstall_string = None;
        let apps = vec![big, create_test_app("Small", "Pub", 1024, 0, 0)];
        let state = AppSortState::default();

        assert_eq!(
            names(
                &apps,
                &filter_and_sort_apps(&apps, AppFilterPreset::Large, "", state)
            ),
            ["Big"]
        );
        assert_eq!(
            names(
                &apps,
                &filter_and_sort_apps(&apps, AppFilterPreset::Orphan, "", state)
            ),
            ["Big"]
        );
        assert_eq!(
            filter_and_sort_apps(&apps, AppFilterPreset::All, "", state).len(),
            2
        );
    }

    #[test]
    fn test_unused_preset_matches_stale_and_unrecorded() {
        let now = now_unix_secs();
        let apps = vec![
            // 从来没有使用记录 —— 算长期未用
            create_test_app("NeverUsed", "Pub", 1024, 0, 0),
            // 最后一次使用是 100 天前 —— 算长期未用
            create_test_app("Stale", "Pub", 1024, 0, now - 100 * 86400),
            // 昨天还在用 —— 不算
            create_test_app("Fresh", "Pub", 1024, 0, now - 86400),
        ];
        let state = AppSortState::new(AppSortColumn::Name, SortDirection::Ascending);

        assert_eq!(
            names(
                &apps,
                &filter_and_sort_apps(&apps, AppFilterPreset::Unused, "", state)
            ),
            ["NeverUsed", "Stale"]
        );
    }

    #[test]
    fn cmp_ci_ignores_case_without_allocating() {
        assert_eq!(cmp_ci("apple", "APPLE"), Ordering::Equal);
        assert_eq!(cmp_ci("Apple", "banana"), Ordering::Less);
        assert_eq!(cmp_ci("Zebra", "apple"), Ordering::Greater);
        assert_eq!(cmp_ci("app", "apple"), Ordering::Less);
    }

    #[test]
    fn test_residual_kind_size() {
        let f = ResidualKind::File(PathBuf::from("test.txt"), 1024);
        let d = ResidualKind::Directory(PathBuf::from("dir"), 2048);
        let rk = ResidualKind::RegistryKey(AppRegRoot::Hkcu, "Software\\App".into());
        let st = ResidualKind::ScheduledTask(r"\Vendor\AppUpdate".into());
        assert_eq!(f.size(), 1024);
        assert_eq!(d.size(), 2048);
        assert_eq!(rk.size(), 0);
        assert_eq!(st.size(), 0);
        assert_eq!(
            st.kind_label_lang(crate::core::i18n::Language::Zh),
            "计划任务"
        );
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;

    /// 未加引号的带空格路径必须整段识别出来。
    ///
    /// 这是让本机 24 款软件卸载失败的根因：按空格切会得到 `C:\Program`。
    #[test]
    fn unquoted_path_with_spaces_is_recovered() {
        let cmd = r"C:\Program Files\DAUM\PotPlayer\unins000.exe /SILENT";
        let real = r"C:\Program Files\DAUM\PotPlayer\unins000.exe";
        let (exe, args) = split_command_with(cmd, |p| p == real);
        assert_eq!(exe, real);
        assert_eq!(args, vec!["/SILENT"]);
    }

    #[test]
    fn quoted_path_is_split_on_quotes() {
        let cmd = r#""C:\Program Files\Foo\unins.exe" /S /NORESTART"#;
        let (exe, args) = split_command_with(cmd, |_| false);
        assert_eq!(exe, r"C:\Program Files\Foo\unins.exe");
        assert_eq!(args, vec!["/S", "/NORESTART"]);
    }

    /// winget / powershell 这类靠 PATH 解析的裸命令名，文件系统里查不到，
    /// 必须退回「第一段是命令」而不是把整行当成路径。
    #[test]
    fn bare_command_falls_back_to_first_token() {
        let (exe, args) = split_command_with("winget uninstall --id Foo", |_| false);
        assert_eq!(exe, "winget");
        assert_eq!(args, vec!["uninstall", "--id", "Foo"]);
    }

    #[test]
    fn no_args_is_handled() {
        let real = r"C:\Foo\unins.exe";
        let (exe, args) = split_command_with(real, |p| p == real);
        assert_eq!(exe, real);
        assert!(args.is_empty());
    }

    #[test]
    fn empty_command_yields_nothing() {
        let (exe, args) = split_command_with("   ", |_| true);
        assert!(exe.is_empty());
        assert!(args.is_empty());
    }

    /// 短前缀和长前缀都存在时取**短**的——与 Windows 从左往右
    /// 取第一个命中的解析规则一致，否则会把参数吃进路径里。
    #[test]
    fn shortest_existing_prefix_wins() {
        let short = r"C:\Foo.exe";
        let long = r"C:\Foo.exe bar";
        let (exe, args) = split_command_with(r"C:\Foo.exe bar --flag", |p| p == short || p == long);
        assert_eq!(exe, short);
        assert_eq!(args, vec!["bar", "--flag"]);
    }

    /// `C:\Program Files` 这个目录永远在。生产路径用 `is_file`，
    /// 所以即使用回调模拟「目录和 exe 都存在」，调用方也必须只把文件
    /// 报成真——`split_command` 的包装就是这么做的。
    #[test]
    fn directory_prefix_must_not_win_over_the_real_exe() {
        let cmd = r"C:\Program Files (x86)\pdfcvt\uninstall.exe";
        let real = r"C:\Program Files (x86)\pdfcvt\uninstall.exe";
        let (exe, args) = split_command_with(cmd, |p| p == real);
        assert_eq!(exe, real);
        assert!(args.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn unquoted_program_files_x86_is_not_a_directory() {
        let cmd = r"C:\Program Files (x86)\pdfcvt\uninstall.exe";
        let (exe, args) = split_command(cmd);
        assert_ne!(
            exe, r"C:\Program Files",
            "不能把卸载命令截成 Program Files 目录"
        );
        assert!(
            !std::path::Path::new(&exe).is_dir(),
            "解析出的可执行文件不能是目录: {exe}"
        );
        if std::path::Path::new(cmd).is_file() {
            assert_eq!(exe, cmd);
            assert!(args.is_empty());
        }
    }

    /// 带空格的路径不存在时不能误判：应退回第一段，而不是把整行当路径。
    #[test]
    fn nonexistent_path_does_not_swallow_arguments() {
        let (exe, args) = split_command_with(r"C:\Gone\unins.exe /S", |_| false);
        assert_eq!(exe, r"C:\Gone\unins.exe");
        assert_eq!(args, vec!["/S"]);
    }

    #[test]
    fn residual_clean_drops_app_when_uninstall_entry_is_gone() {
        let original = vec![
            ResidualItem::certain(
                ResidualKind::RegistryKey(AppRegRoot::Hklm, "Software\\Uninstall\\Foo".into()),
                ResidualSource::UninstallEntry,
            ),
            ResidualItem::possible(
                ResidualKind::Directory(PathBuf::from(r"C:\Users\me\AppData\Local\Foo"), 12),
                ResidualSource::AppDataDir,
            ),
        ];
        assert!(app_gone_after_residual_clean(&original, &original[1..]));
        assert!(!app_gone_after_residual_clean(&original, &original));
        assert!(!app_gone_after_residual_clean(&original[1..], &[]));
    }

    #[test]
    fn residual_clean_drops_app_when_install_dir_is_gone() {
        let original = vec![ResidualItem::certain(
            ResidualKind::Directory(PathBuf::from(r"C:\Program Files\Foo"), 100),
            ResidualSource::InstallDir,
        )];
        assert!(app_gone_after_residual_clean(&original, &[]));
    }

    fn sample_residual_items() -> Vec<ResidualItem> {
        vec![
            ResidualItem::certain(
                ResidualKind::Directory(PathBuf::from(r"C:\Program Files\Foo"), 100),
                ResidualSource::InstallDir,
            ),
            ResidualItem::certain(
                ResidualKind::RegistryKey(AppRegRoot::Hklm, "Software\\Uninstall\\Foo".into()),
                ResidualSource::UninstallEntry,
            ),
            ResidualItem::possible(
                ResidualKind::Directory(PathBuf::from(r"C:\Users\me\AppData\Local\Foo"), 12),
                ResidualSource::AppDataDir,
            ),
            ResidualItem::possible(
                ResidualKind::Directory(PathBuf::from(r"C:\Users\me\AppData\Roaming\Foo"), 8),
                ResidualSource::AppDataDir,
            ),
        ]
    }

    /// 勾选的 6 项清掉后，默认未勾选的 2 项不能再弹第二次。
    #[test]
    fn follow_up_does_not_reopen_dialog_for_unselected_items() {
        let items = sample_residual_items();
        let selected: HashSet<usize> = [0, 1].into_iter().collect();
        let follow = residual_clean_follow_up(&items, &selected, |_| false);
        assert!(
            follow.retry_items.is_empty(),
            "未勾选项不应再次进入残留弹窗"
        );
        assert!(follow.retry_selected.is_empty());
        assert_eq!(follow.leftover_for_app.len(), 2);
        assert!(app_gone_after_residual_clean(
            &items,
            &follow.leftover_for_app
        ));
    }

    /// 勾选了却没删掉的，才留在对话框里方便重试；未勾选项仍不进弹窗。
    #[test]
    fn follow_up_retries_only_failed_selected_items() {
        let items = sample_residual_items();
        let selected: HashSet<usize> = [0].into_iter().collect();
        let follow = residual_clean_follow_up(&items, &selected, |item| {
            item.source == ResidualSource::InstallDir
        });
        assert_eq!(follow.retry_items.len(), 1);
        assert_eq!(follow.retry_items[0].source, ResidualSource::InstallDir);
        assert_eq!(follow.retry_selected.len(), 1);
        assert_eq!(follow.leftover_for_app.len(), 4);
        assert!(!app_gone_after_residual_clean(
            &items,
            &follow.leftover_for_app
        ));
    }

    /// 只清了缓存、安装目录和卸载登记项都没勾，软件仍应留在已安装列表。
    #[test]
    fn follow_up_keeps_app_when_install_dir_was_left_unchecked() {
        let items = sample_residual_items();
        let selected: HashSet<usize> = [2].into_iter().collect();
        let follow = residual_clean_follow_up(&items, &selected, |_| false);
        assert!(follow.retry_items.is_empty());
        assert!(!app_gone_after_residual_clean(
            &items,
            &follow.leftover_for_app
        ));
    }
}

#[cfg(test)]
mod quoted_command_tests {
    use super::*;

    /// 厂商把「程序 + 参数」整条塞进一对引号里（联想应用商店的真实写法）。
    /// 引号内容不是真实文件，必须继续切分，否则会误报「卸载器不存在」。
    #[test]
    fn quotes_wrapping_whole_command_are_split_further() {
        let real = r"C:\Program Files (x86)\Lenovo\LeAppStore\StoreUninstaller.exe";
        let cmd = format!(r#""{real} /SLIENT""#);
        let (exe, args) = split_command_with(&cmd, |p| p == real);
        assert_eq!(exe, real);
        assert_eq!(args, vec!["/SLIENT"]);
    }

    /// 正常的引号写法不受影响：即使文件当前不存在也按引号切。
    #[test]
    fn well_formed_quotes_still_win_when_file_is_gone() {
        let cmd = r#""C:\Gone\unins 2.exe" /S"#;
        let (exe, args) = split_command_with(cmd, |_| false);
        assert_eq!(exe, r"C:\Gone\unins 2.exe");
        assert_eq!(args, vec!["/S"]);
    }
}
