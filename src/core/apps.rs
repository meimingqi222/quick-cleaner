//! 软件管理与残留清理数据模型与核心排序过滤

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
        match self {
            AppRegRoot::Hklm => "HKLM (64位)",
            AppRegRoot::Hklm32 => "HKLM (32位)",
            AppRegRoot::Hkcu => "HKCU (当前用户)",
            AppRegRoot::SystemApp => "系统/UWP",
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
        match self {
            AppFilterPreset::All => "全部软件",
            AppFilterPreset::Large => "大型软件 (>500MB)",
            AppFilterPreset::Recent => "有安装日期",
            AppFilterPreset::Orphan => "无官方卸载器",
        }
    }

    /// 某个软件是否落在该预设分类里。
    pub fn matches(&self, app: &InstalledApp) -> bool {
        match self {
            AppFilterPreset::All => true,
            AppFilterPreset::Large => app.estimated_size >= 500 * 1024 * 1024,
            AppFilterPreset::Recent => app.install_date.is_some(),
            AppFilterPreset::Orphan => {
                app.uninstall_string.is_none() && app.quiet_uninstall_string.is_none()
            }
        }
    }
}

/// 关联残留项目分类
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResidualKind {
    File(PathBuf, u64),
    Directory(PathBuf, u64),
    RegistryKey(AppRegRoot, String),
}

impl ResidualKind {
    pub fn size(&self) -> u64 {
        match self {
            ResidualKind::File(_, s) | ResidualKind::Directory(_, s) => *s,
            ResidualKind::RegistryKey(..) => 0,
        }
    }

    pub fn display_label(&self) -> String {
        match self {
            ResidualKind::File(p, _) => format!("📄 残留文件: {}", p.display()),
            ResidualKind::Directory(p, _) => format!("📁 残留目录: {}", p.display()),
            ResidualKind::RegistryKey(root, sub) => format!("🗝️ 注册表项: {}\\{}", root.label(), sub),
        }
    }
}

/// 关联残留深度扫描结果
#[derive(Clone, Debug, Default)]
pub struct ResidualScanResult {
    pub app_name: String,
    pub items: Vec<ResidualKind>,
    pub total_file_size: u64,
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
