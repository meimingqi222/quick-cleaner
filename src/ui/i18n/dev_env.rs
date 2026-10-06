//! 开发环境与包管理视图国际化文案

use crate::core::dev_env::remove::RemovalRefusal;
use crate::core::dev_env::{DevAssetKind, SiteScope, TimestampSource};
use crate::core::i18n::Language;

pub fn tr_apps_tab_desktop(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "桌面应用",
        Language::En => "Desktop Apps",
    }
}

pub fn tr_apps_tab_dev(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "开发环境",
        Language::En => "Dev Environments",
    }
}

pub fn tr_dev_overview_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "开发环境与包管理",
        Language::En => "Dev Environments & Packages",
    }
}

pub fn tr_dev_overview_desc(lang: Language) -> &'static str {
    match lang {
        Language::Zh => {
            "审查已安装的 Conda 虚拟环境、Node 全局包与 Python 独立 CLI 工具。\
移除一律交给各生态自己的命令执行，不做文件删除"
        }
        Language::En => {
            "Inspect installed Conda environments, Node global packages, and Python CLI tools. \
Removal always runs the ecosystem's own command, never a file delete"
        }
    }
}

pub fn tr_dev_section_conda(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "Python 与 Conda 环境",
        Language::En => "Python & Conda Environments",
    }
}

pub fn tr_dev_section_node(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "Node / JS 全局包",
        Language::En => "Node Global Packages",
    }
}

pub fn tr_dev_section_tools(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "Python 独立 CLI 工具",
        Language::En => "Python CLI Tools",
    }
}

pub fn tr_dev_badge_base(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "Base 基础环境",
        Language::En => "Base Protected",
    }
}

pub fn tr_dev_badge_active(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "当前激活",
        Language::En => "Active",
    }
}

pub fn tr_dev_badge_busy(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "进程占用",
        Language::En => "In Use",
    }
}

pub fn tr_dev_badge_unknown(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "占用未知",
        Language::En => "Unknown Status",
    }
}

pub fn tr_dev_badge_clear(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "空闲",
        Language::En => "Idle",
    }
}

pub fn tr_dev_col_name(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "名称与路径",
        Language::En => "Name & Path",
    }
}

pub fn tr_dev_col_exclusive(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "独占可释放",
        Language::En => "Exclusive",
    }
}

pub fn tr_dev_col_logical(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "逻辑总大小",
        Language::En => "Logical Size",
    }
}

pub fn tr_dev_col_size(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "占用大小",
        Language::En => "Size",
    }
}

pub fn tr_dev_col_shared_logical(lang: Language, logical_str: &str) -> String {
    match lang {
        Language::Zh => format!("逻辑 {logical_str}"),
        Language::En => format!("logical {logical_str}"),
    }
}

pub fn tr_dev_col_last_modified(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "最后变更",
        Language::En => "Last Modified",
    }
}

pub fn tr_dev_btn_rescan(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "重新检测",
        Language::En => "Rescan",
    }
}

pub fn tr_dev_empty(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "未检测到相关环境或全局包",
        Language::En => "No environments or global packages detected",
    }
}

pub fn tr_dev_total_assets(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "环境与工具总数",
        Language::En => "Total Assets",
    }
}

/// 「可释放」只统计**进得了移除通道**的部分：只读资产（Python 解释器与虚拟
/// 环境、bun、降级扫描来源）这个页面不会删，算进去就是在报一个做不到的数。
pub fn tr_dev_total_exclusive(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "可移除项可释放",
        Language::En => "Reclaimable (removable)",
    }
}

pub fn tr_dev_total_logical(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "逻辑总规模",
        Language::En => "Total Logical Size",
    }
}

// ---- 移除通道 ----

/// 这一条没有移除入口时挂的徽章。
///
/// 「只展示」四个字是给用户的承诺，不是内部状态：它表示这个条目要么来自
/// 按目录的降级扫描、要么所在生态没有核实过的卸载命令、要么它本身就不是
/// 删除单位（解释器的包目录行——可移除的是它里面的 pip 包），总之**不会**
/// 被这个页面删掉。
pub fn tr_dev_badge_display_only(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "只展示",
        Language::En => "Display only",
    }
}

pub fn tr_dev_btn_remove(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "移除",
        Language::En => "Remove",
    }
}

/// 展开一个 site 行，看它里面的 pip 包。
pub fn tr_dev_btn_expand(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "展开",
        Language::En => "Expand",
    }
}

pub fn tr_dev_btn_collapse(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "收起",
        Language::En => "Collapse",
    }
}

/// 展开后一个可识别的包都没有。正常只会在登记损坏时出现——目录枚举得出
/// dist-info，却读不出任何一个可信的名字。
pub fn tr_dev_site_empty(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "没有可识别的已安装包",
        Language::En => "No identifiable installed packages",
    }
}

pub fn tr_dev_site_loading(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "正在读取包列表…",
        Language::En => "Loading packages…",
    }
}

/// 时间列的表头。措辞是**最后变更**而不是「最近使用」：
///
/// 这些时间点来自生态自己写的记录（conda 的事务记录、包的 package.json、
/// 工具的元数据文件），它们只说明「什么时候装过/改过」，不说明「什么时候
/// 用过」。写成「最近使用」就是在替这些数据编一个它没有的含义。
pub fn tr_dev_col_last_change(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "最后变更",
        Language::En => "Last changed",
    }
}

/// 时间点来源的说明，跟在时间后面，让「最后变更」具体指什么一眼可见。
pub fn tr_dev_timestamp_source(lang: Language, source: TimestampSource) -> &'static str {
    match (lang, source) {
        (Language::Zh, TimestampSource::CondaTransaction) => "conda 事务",
        (Language::En, TimestampSource::CondaTransaction) => "conda transaction",
        (Language::Zh, TimestampSource::PackageManifest) => "安装版本",
        (Language::En, TimestampSource::PackageManifest) => "installed version",
        (Language::Zh, TimestampSource::ToolMetadata) => "工具记录",
        (Language::En, TimestampSource::ToolMetadata) => "tool record",
        (Language::Zh, TimestampSource::DirectoryEntry) => "目录项",
        (Language::En, TimestampSource::DirectoryEntry) => "directory entry",
        (Language::Zh, TimestampSource::SitePackages) => "包目录",
        (Language::En, TimestampSource::SitePackages) => "package dir",
    }
}

pub fn tr_dev_timestamp_unknown(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "无记录",
        Language::En => "no record",
    }
}

/// 确认弹窗里对「要移除的是什么」的描述。
///
/// 带上类别而不只是名字：全局包和环境可能有重名，用户需要知道自己在删的是
/// 一个 npm 包还是一个 conda 环境。
pub fn tr_dev_what(lang: Language, kind: &DevAssetKind) -> String {
    match (lang, kind) {
        (Language::Zh, DevAssetKind::CondaEnv { name, .. }) => {
            format!(" conda 环境「{name}」")
        }
        (Language::En, DevAssetKind::CondaEnv { name, .. }) => {
            format!(" the conda environment \"{name}\"")
        }
        (Language::Zh, DevAssetKind::NodeGlobalPackage { manager, name, .. }) => {
            format!(" {manager} 全局包「{name}」")
        }
        (Language::En, DevAssetKind::NodeGlobalPackage { manager, name, .. }) => {
            format!(" the {manager} global package \"{name}\"")
        }
        (Language::Zh, DevAssetKind::PythonTool { name, .. }) => {
            format!(" CLI 工具「{name}」")
        }
        (Language::En, DevAssetKind::PythonTool { name, .. }) => {
            format!(" the CLI tool \"{name}\"")
        }
        // 解释器的包目录行是只读的，措辞上也不承诺能删（可移除的是它里面的
        // pip 包条目，那有自己的描述）。
        (Language::Zh, DevAssetKind::PythonInterpreter { name, .. }) => {
            format!(" Python 解释器「{name}」的包目录")
        }
        (Language::En, DevAssetKind::PythonInterpreter { name, .. }) => {
            format!(" the package directory of Python \"{name}\"")
        }
        (Language::Zh, DevAssetKind::VirtualEnv { name, .. }) => {
            format!("虚拟环境「{name}」")
        }
        (Language::En, DevAssetKind::VirtualEnv { name, .. }) => {
            format!("the virtualenv \"{name}\"")
        }
        (Language::Zh, DevAssetKind::PipPackage { name, .. }) => {
            format!("pip 包「{name}」")
        }
        (Language::En, DevAssetKind::PipPackage { name, .. }) => {
            format!("the pip package \"{name}\"")
        }
    }
}

pub fn tr_dev_confirm_title(lang: Language, name: &str) -> String {
    match lang {
        Language::Zh => format!("移除 {name}？"),
        Language::En => format!("Remove {name}?"),
    }
}

/// 确认弹窗的正文：说清「谁来删」和「删掉会怎样」。
pub fn tr_dev_confirm_body(lang: Language, what: &str) -> String {
    match lang {
        Language::Zh => format!(
            "将调用该生态自己的卸载命令移除{what}。\
这不是清理缓存：移除后需要重新安装才能恢复，且不会回退成直接删除目录。"
        ),
        Language::En => format!(
            "Runs the ecosystem's own uninstall command to remove {what}. \
This is not cache cleanup: reinstalling is the only way back, and it never falls back to deleting files."
        ),
    }
}

/// venv 整体删除的正文：它走的不是卸载命令，是核验过凭据的整目录删除，
/// 措辞必须照实说——「不会回退成直接删除目录」对它不成立。
pub fn tr_dev_confirm_body_directory(lang: Language, what: &str) -> String {
    match lang {
        Language::Zh => format!(
            "将整目录删除{what}。它已通过 pyvenv.cfg 验明是虚拟环境，\
且不在任何包管理器的登记里，目录本身就是全部状态。\
删除后可随时用 venv / poetry 重建，但里面的包需要重新安装。"
        ),
        Language::En => format!(
            "Deletes {what} together with its whole directory. It is verified as a virtualenv \
by pyvenv.cfg and registered in no package manager—the directory is its whole state. \
It can be recreated with venv / poetry, but its packages need reinstalling."
        ),
    }
}

pub fn tr_dev_confirm_detail(lang: Language, path: &str, size: &str) -> String {
    match lang {
        Language::Zh => format!("位置：{path}\n独占可释放：{size}"),
        Language::En => format!("Location: {path}\nExclusive reclaimable: {size}"),
    }
}

/// pip 包条目的确认详情：包的体积在扫描期没有单独测算（测它要把 site 再走
/// 一遍），如实给版本号，不拿 0 冒充。
pub fn tr_dev_confirm_detail_version(lang: Language, path: &str, version: &str) -> String {
    match lang {
        Language::Zh => format!("位置：{path}\n版本：{version}"),
        Language::En => format!("Location: {path}\nVersion: {version}"),
    }
}

/// 拒绝原因。每一条都要让用户知道**下一步该做什么**，而不是只说「不行」。
pub fn tr_dev_refusal(lang: Language, reason: RemovalRefusal) -> &'static str {
    match (lang, reason) {
        (Language::Zh, RemovalRefusal::Protected) => "该路径受核心安全规则保护，不能移除",
        (Language::En, RemovalRefusal::Protected) => {
            "This path is protected by the core safety rules"
        }
        (Language::Zh, RemovalRefusal::ProtectedEnvironment) => "base 环境与当前激活的环境不能移除",
        (Language::En, RemovalRefusal::ProtectedEnvironment) => {
            "The base environment and the active environment cannot be removed"
        }
        (Language::Zh, RemovalRefusal::Busy) => "该目标正被占用，或占用状态无法确认",
        (Language::En, RemovalRefusal::Busy) => {
            "The target is in use, or its occupancy cannot be determined"
        }
        (Language::Zh, RemovalRefusal::UnsupportedTool) => "该生态的卸载命令尚未核实，只支持查看",
        (Language::En, RemovalRefusal::UnsupportedTool) => {
            "This ecosystem's uninstall command is not verified yet; view only"
        }
        (Language::Zh, RemovalRefusal::UnverifiedSource) => {
            "该条目来自按目录的降级扫描，未经生态确认，只支持查看"
        }
        (Language::En, RemovalRefusal::UnverifiedSource) => {
            "This entry came from a directory fallback scan, not the ecosystem itself; view only"
        }
        (Language::Zh, RemovalRefusal::ToolUnavailable) => {
            "该生态的命令不可用（未安装或不在 PATH 上），无法安全移除"
        }
        (Language::En, RemovalRefusal::ToolUnavailable) => {
            "The ecosystem's command is unavailable (not installed or not on PATH)"
        }
        (Language::Zh, RemovalRefusal::ToolUnreadable) => "该生态的命令输出无法解析，暂不移除",
        (Language::En, RemovalRefusal::ToolUnreadable) => {
            "The ecosystem's command output could not be parsed"
        }
        (Language::Zh, RemovalRefusal::NotRegistered) => {
            "该目标已不在生态自己的清单里，或位置与扫描时不一致"
        }
        (Language::En, RemovalRefusal::NotRegistered) => {
            "The target is no longer in the ecosystem's own inventory, or its location changed"
        }
    }
}

pub fn tr_dev_status_removing(lang: Language, name: &str) -> String {
    match lang {
        Language::Zh => format!("正在移除 {name}…"),
        Language::En => format!("Removing {name}…"),
    }
}

pub fn tr_dev_status_removed(lang: Language, name: &str) -> String {
    match lang {
        Language::Zh => format!("已移除 {name}"),
        Language::En => format!("Removed {name}"),
    }
}

pub fn tr_dev_status_refused(lang: Language, name: &str, reason: RemovalRefusal) -> String {
    match lang {
        Language::Zh => format!("未移除 {name}：{}", tr_dev_refusal(Language::Zh, reason)),
        Language::En => format!(
            "Did not remove {name}: {}",
            tr_dev_refusal(Language::En, reason)
        ),
    }
}

/// 命令跑了但没通过核验。此时状态未知，必须让用户知道要去手工确认。
pub fn tr_dev_status_failed(lang: Language, name: &str, detail: &str) -> String {
    match lang {
        Language::Zh => format!("移除 {name} 未完成，状态未知，请手工确认：{detail}"),
        Language::En => {
            format!("Removing {name} did not complete; state is unknown, verify manually: {detail}")
        }
    }
}

// ---- 条目明细（原先写死在视图里） ----

pub fn tr_dev_detail_python(lang: Language, version: &str) -> String {
    match lang {
        Language::Zh => format!("Python {version}"),
        Language::En => format!("Python {version}"),
    }
}

pub fn tr_dev_detail_packages(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("{count} 个包"),
        Language::En => format!("{count} packages"),
    }
}

pub fn tr_dev_detail_manager_version(lang: Language, manager: &str, version: &str) -> String {
    match lang {
        Language::Zh => format!("{manager} · v{version}"),
        Language::En => format!("{manager} · v{version}"),
    }
}

/// 虚拟环境的管理者。只影响展示归类，两者的目录结构与可删性没有区别。
pub fn tr_dev_detail_venv_manager(lang: Language, is_poetry: bool) -> &'static str {
    match (lang, is_poetry) {
        (Language::Zh, true) => "Poetry",
        (Language::En, true) => "Poetry",
        (Language::Zh, false) => "virtualenvwrapper",
        (Language::En, false) => "virtualenvwrapper",
    }
}

/// 虚拟环境建在哪个解释器上（取 `pyvenv.cfg` 的 `home` 的末段，整条路径太长）。
pub fn tr_dev_detail_base_python(lang: Language, name: &str) -> String {
    match lang {
        Language::Zh => format!("基于 {name}"),
        Language::En => format!("based on {name}"),
    }
}

/// 这一行的包目录是哪一级的。
///
/// 必须写清楚：`pip list` 把两级加起来报，而界面上它们是两行；不标出来用户会
/// 以为重复了，或者以为其中一行是错的。
pub fn tr_dev_detail_site_scope(lang: Language, scope: SiteScope) -> &'static str {
    match (lang, scope) {
        (Language::Zh, SiteScope::Install) => "解释器自带包目录",
        (Language::En, SiteScope::Install) => "interpreter packages",
        (Language::Zh, SiteScope::User) => "用户包目录",
        (Language::En, SiteScope::User) => "user packages",
    }
}

pub fn tr_dev_detail_tool_kind(lang: Language, is_uv: bool) -> &'static str {
    match (lang, is_uv) {
        (Language::Zh, true) => "uv 工具",
        (Language::En, true) => "uv tool",
        (Language::Zh, false) => "pipx 环境",
        (Language::En, false) => "pipx venv",
    }
}

// ---- 批量多选、底栏与生态分类 Tab ----

pub fn tr_dev_filter_all(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "全部",
        Language::En => "All",
    }
}

pub fn tr_dev_filter_python(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "Python",
        Language::En => "Python",
    }
}

pub fn tr_dev_filter_node(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "Node",
        Language::En => "Node",
    }
}

pub fn tr_dev_filter_tools(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "工具",
        Language::En => "Tools",
    }
}

pub fn tr_dev_search_packages_placeholder(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "搜索环境或包名...",
        Language::En => "Search envs or packages...",
    }
}

pub fn tr_dev_batch_bar_selected(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "已选择移除",
        Language::En => "Selected for removal",
    }
}

pub fn tr_dev_batch_items_count(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("{count} 项"),
        Language::En => format!("{count} items"),
    }
}

pub fn tr_dev_batch_bar_remove_btn(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("批量移除 ({count})"),
        Language::En => format!("Remove ({count})"),
    }
}

pub fn tr_dev_batch_confirm_title(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("批量移除选中的 {count} 个项？"),
        Language::En => format!("Remove {count} selected items?"),
    }
}

pub fn tr_dev_batch_confirm_body(lang: Language) -> &'static str {
    match lang {
        Language::Zh => {
            "将依次调用各生态的原生卸载命令移除选中的环境或包。\
这不是清理缓存：移除后需要重新安装，且不会直接删除文件。"
        }
        Language::En => {
            "Runs the ecosystem's own uninstall command for each selected environment or package. \
This is not cache cleanup: reinstalling is required to restore, and it never falls back to direct deletion."
        }
    }
}

pub fn tr_dev_batch_select_all(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "全选",
        Language::En => "All",
    }
}

pub fn tr_dev_batch_invert(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "反选",
        Language::En => "Invert",
    }
}

pub fn tr_dev_batch_clear(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "清空",
        Language::En => "Clear",
    }
}

pub fn tr_dev_site_select_all(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "全选本环境",
        Language::En => "Select all",
    }
}

pub fn tr_dev_site_matched(lang: Language, matched: usize, total: usize) -> String {
    match lang {
        Language::Zh => format!("匹配 {matched} / 共 {total} 个包"),
        Language::En => format!("{matched} of {total} packages"),
    }
}

pub fn tr_dev_detail_empty_selection(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "在左侧选择环境或工具以查看详情",
        Language::En => "Select an environment or tool on the left to view details",
    }
}

pub fn tr_dev_detail_profile_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "环境与资产档案",
        Language::En => "Environment & Asset Profile",
    }
}

pub fn tr_dev_detail_path(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "安装路径",
        Language::En => "Install Path",
    }
}

pub fn tr_dev_detail_executables(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "关联可执行程序",
        Language::En => "Associated Executables",
    }
}

pub fn tr_dev_detail_sub_packages(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("已安装的第三方包（共 {count} 个）"),
        Language::En => format!("Installed packages ({count} total)"),
    }
}
