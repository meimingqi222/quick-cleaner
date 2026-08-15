//! 软件管理与残留清理数据模型与核心排序过滤

use crate::core::i18n::Language;
use std::cmp::Ordering;
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
    pub fn label(&self) -> &'static str {
        self.label_lang(Language::Zh)
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
    pub fn label(&self) -> &'static str {
        match self {
            AppSortColumn::Name => "应用名称与版本",
            AppSortColumn::Publisher => "开发者",
            AppSortColumn::LastUsed => "最后使用",
            AppSortColumn::InstallDate => "安装日期",
            AppSortColumn::Size => "占用大小",
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
    Recent,
    Orphan,
}

impl AppFilterPreset {
    pub const ALL: [AppFilterPreset; 4] = [
        AppFilterPreset::All,
        AppFilterPreset::Large,
        AppFilterPreset::Recent,
        AppFilterPreset::Orphan,
    ];

    pub fn label(&self) -> &'static str {
        self.label_lang(Language::Zh)
    }

    pub fn label_lang(&self, lang: Language) -> &'static str {
        match lang {
            Language::Zh => match self {
                AppFilterPreset::All => "全部软件",
                AppFilterPreset::Large => "大型软件 (>500MB)",
                AppFilterPreset::Recent => "有安装日期",
                AppFilterPreset::Orphan => "卸载器失效",
            },
            Language::En => match self {
                AppFilterPreset::All => "All Apps",
                AppFilterPreset::Large => "Large Apps (>500MB)",
                AppFilterPreset::Recent => "With Install Date",
                AppFilterPreset::Orphan => "Invalid Uninstaller",
            },
        }
    }

    /// 某个软件是否落在该预设分类里。
    pub fn matches(&self, app: &InstalledApp) -> bool {
        match self {
            AppFilterPreset::All => true,
            AppFilterPreset::Large => app.estimated_size >= 500 * 1024 * 1024,
            AppFilterPreset::Recent => app.install_date.is_some(),
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
}

impl ResidualKind {
    pub fn size(&self) -> u64 {
        match self {
            ResidualKind::File(_, s) | ResidualKind::Directory(_, s) => *s,
            ResidualKind::RegistryKey(..) | ResidualKind::RegistryValue(..) => 0,
        }
    }

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
            },
            Language::En => match self {
                ResidualKind::File(..) => "File",
                ResidualKind::Directory(..) => "Directory",
                ResidualKind::RegistryKey(..) => "Registry Key",
                ResidualKind::RegistryValue(..) => "Registry Value",
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
        }
    }
}

/// 一条残留记录：内容 + 把握程度 + 给用户看的来源说明。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResidualItem {
    pub kind: ResidualKind,
    pub confidence: Confidence,
    /// 这条是被哪个扫描器发现的，例如「开机启动项」「防火墙规则」
    pub source: &'static str,
}

impl ResidualItem {
    pub fn certain(kind: ResidualKind, source: &'static str) -> Self {
        Self {
            kind,
            confidence: Confidence::Certain,
            source,
        }
    }

    pub fn possible(kind: ResidualKind, source: &'static str) -> Self {
        Self {
            kind,
            confidence: Confidence::Possible,
            source,
        }
    }

    pub fn size(&self) -> u64 {
        self.kind.size()
    }

    pub fn display_label(&self) -> String {
        format!("[{}] {}", self.source, self.kind.display_label())
    }
}

/// 关联残留深度扫描结果
#[derive(Clone, Debug, Default)]
pub struct ResidualScanResult {
    pub app_name: String,
    pub items: Vec<ResidualItem>,
    pub total_file_size: u64,
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
        self.items.iter().filter(|i| i.confidence.is_certain()).count()
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
    haystack
        .to_lowercase()
        .contains(needle_lower)
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
    let mut idx: Vec<usize> = apps
        .iter()
        .enumerate()
        .filter(|(_, app)| {
            preset.matches(app)
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
        "app", "apps", "microsoft", "windows", "system", "system32",
        "program files", "program files (x86)", "common files", "appdata",
        "local", "roaming", "locallow", "programdata", "temp", "tmp",
        "users", "google", "apple", "intel", "amd", "nvidia", "adobe", "tencent",
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
/// 的文件」就是可执行文件，其余算参数。带引号的路径按引号切，最省事。
///
/// `exists` 注入是为了可测试——生产用 [`split_command`]。
pub fn split_command_with(cmd: &str, exists: impl Fn(&str) -> bool) -> (String, Vec<String>) {
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
            if exists(exe) || ends_with_executable_ext(exe) {
                return (exe.to_string(), parse_cmd_line(rest[end + 1..].trim()));
            }
        }
    }

    // 不带引号（或引号内混着参数）：逐段延长，直到拼出一个真实存在的文件
    let cmd = cmd.trim_matches('"');
    let tokens: Vec<&str> = cmd.split(' ').filter(|t| !t.is_empty()).collect();
    for take in 1..=tokens.len() {
        let candidate = tokens[..take].join(" ");
        if exists(&candidate) {
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

/// [`split_command_with`] 的生产版本：用真实文件系统判断存在性。
pub fn split_command(cmd: &str) -> (String, Vec<String>) {
    split_command_with(cmd, |p| std::path::Path::new(p).exists())
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
            names(&apps, &filter_and_sort_apps(&apps, AppFilterPreset::All, "", desc)),
            ["AppRecent", "AppOld", "AppNever"]
        );

        // 升序时「从未使用」(raw = 0) 依然要沉到最后
        let asc = AppSortState::new(AppSortColumn::LastUsed, SortDirection::Ascending);
        assert_eq!(
            names(&apps, &filter_and_sort_apps(&apps, AppFilterPreset::All, "", asc)),
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
            names(&apps, &filter_and_sort_apps(&apps, AppFilterPreset::All, "fox", state)),
            ["Firefox"]
        );
        // 关键词也匹配开发者字段
        assert_eq!(
            names(&apps, &filter_and_sort_apps(&apps, AppFilterPreset::All, "micro", state)),
            ["Visual Studio"]
        );
    }

    #[test]
    fn test_preset_filter_is_applied() {
        let mut big = create_test_app("Big", "Pub", 600 * 1024 * 1024, 0, 0);
        big.uninstall_string = None;
        let apps = vec![
            big,
            create_test_app("Small", "Pub", 1024, 0, 0),
        ];
        let state = AppSortState::default();

        assert_eq!(
            names(&apps, &filter_and_sort_apps(&apps, AppFilterPreset::Large, "", state)),
            ["Big"]
        );
        assert_eq!(
            names(&apps, &filter_and_sort_apps(&apps, AppFilterPreset::Orphan, "", state)),
            ["Big"]
        );
        assert_eq!(
            filter_and_sort_apps(&apps, AppFilterPreset::All, "", state).len(),
            2
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
        assert_eq!(f.size(), 1024);
        assert_eq!(d.size(), 2048);
        assert_eq!(rk.size(), 0);
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
        let (exe, args) = split_command_with(
            r"C:\Foo.exe bar --flag",
            |p| p == short || p == long,
        );
        assert_eq!(exe, short);
        assert_eq!(args, vec!["bar", "--flag"]);
    }

    /// 带空格的路径不存在时不能误判：应退回第一段，而不是把整行当路径。
    #[test]
    fn nonexistent_path_does_not_swallow_arguments() {
        let (exe, args) = split_command_with(r"C:\Gone\unins.exe /S", |_| false);
        assert_eq!(exe, r"C:\Gone\unins.exe");
        assert_eq!(args, vec!["/S"]);
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
