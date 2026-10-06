//! 垃圾清理类别与扫描目标规则

mod browser;
mod cache;
pub(crate) mod chromium;
mod dev;
mod docker;
mod helpers;
mod macos;
mod system;
mod updater;

use crate::core::i18n::{Language, Text};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Safety {
    Safe,
    Caution,
    Danger,
}

impl Safety {
    pub fn label(&self) -> &'static str {
        self.label_lang(Language::Zh)
    }

    pub fn label_lang(&self, lang: Language) -> &'static str {
        match lang {
            Language::Zh => match self {
                Safety::Safe => "安全清理",
                Safety::Caution => "注意",
                Safety::Danger => "危险",
            },
            Language::En => match self {
                Safety::Safe => "Safe",
                Safety::Caution => "Caution",
                Safety::Danger => "Danger",
            },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CategoryId {
    SystemTemp,
    UserTemp,
    UserCache,
    BrowserCache,
    PackageCache,
    /// 应用更新器（electron-updater / Squirrel.Mac）留在缓存目录里的更新包。
    /// 条目靠探测目录顶层内容得到，不按应用名登记，所以同一目录里只有更新包
    /// 叶子进这一类，形态不明的子项仍然只是展示项。
    UpdaterPackages,
    Logs,
    RecycleBin,
    Thumbnails,
    BrokenLoginItems,
    // ---- 开发相关，默认不勾选 ----
    AiAgents,
    DevBuild,
    DevWorktrees,
    // ---- macOS 专用，默认不勾选 ----
    LocalSnapshots,
    IosBackup,
    /// `~/Library/Application Support/JetBrains/` 下除最新版本外的旧版
    /// IDE 数据目录。macOS 专属（Windows 的 JetBrains 布局不同）。
    OldIdeData,
    /// Docker 冗余镜像：悬空镜像、未被任何容器引用的镜像与同仓库旧版本
    /// 标签。条目是 `docker://image/<ref>` 虚拟路径，清理走
    /// `docker image rm`，docker 不可用时类别静默消失。
    DockerImages,
}

impl CategoryId {
    pub fn from_rule(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|category| format!("{category:?}") == value)
    }
    pub const ALL: [CategoryId; 17] = [
        CategoryId::SystemTemp,
        CategoryId::UserTemp,
        CategoryId::UserCache,
        CategoryId::BrowserCache,
        CategoryId::PackageCache,
        CategoryId::UpdaterPackages,
        CategoryId::Logs,
        CategoryId::RecycleBin,
        CategoryId::Thumbnails,
        CategoryId::BrokenLoginItems,
        CategoryId::AiAgents,
        CategoryId::DevBuild,
        CategoryId::DevWorktrees,
        CategoryId::LocalSnapshots,
        CategoryId::IosBackup,
        CategoryId::OldIdeData,
        CategoryId::DockerImages,
    ];

    /// 扫描完成后是否默认勾选。
    ///
    /// 规范：**一个目标可以被默认勾选，必须同时满足三条**——
    ///
    /// 1. **认得出**：有内容签名或明确的所有者证据说明这类文件就是它声称的
    ///    用途。目录名不算证据——本仓库曾按应用名列更新器目录，实测 6 个名字
    ///    里 5 个在机器上不存在，而真实存在的 4 个一个都没被列到。
    /// 2. **最坏情况能界定**：删除的代价止于「重新下载 / 重新生成」。只要可能
    ///    损失凭据、密钥、登录态、未提交改动、唯一副本（崩溃报告、待上传的诊
    ///    断包）、或内网里根本没有上游可拉的东西（`~/.m2`、`go/pkg/mod`、
    ///    `~/.gradle/caches`），这条就不成立。
    /// 3. **不在事务中间**：所有权程序可能正在用或马上要用它就不行。用年龄门
    ///    （`helpers::is_older_than`）判定，代价只是让「刚刚下好的东西」这一
    ///    轮不动，仍然整项展示。
    ///
    /// 任何一条不成立：**照样展示给用户**，只是不预选。展示不是成本，隐藏才
    /// 是——一个目标不进表，用户既看不见也清不掉。
    ///
    /// 类别只是这三条的缺省表达，判定单位始终是单个目标（见 `ScanTarget::
    /// recommended`）。所以同一类别里可以同时有 `PackageCache` 的公共 registry
    /// 缓存（三条都成立）和 `go/pkg/mod`（第 2 条不成立）。
    ///
    /// 这条规范会推翻直觉，两处已知例子：应用更新包看着像「正在用的东西」但
    /// 三条都成立；整个 `~/Library/Logs` 看着就是日志，实际是一个目标覆盖 N 个
    /// 所有者、里面躺着 `OneDrive/…/general.keystore` 和 `DiagnosticReports`。
    pub fn default_selected(&self) -> bool {
        self.safety() == Safety::Safe
    }

    /// 是否属于开发者类目。
    pub fn is_developer(&self) -> bool {
        matches!(
            self,
            CategoryId::AiAgents
                | CategoryId::DevBuild
                | CategoryId::DevWorktrees
                | CategoryId::LocalSnapshots
                | CategoryId::IosBackup
                | CategoryId::OldIdeData
                | CategoryId::DockerImages
        )
    }

    /// 清理时是否连目录本身一起删掉。
    ///
    /// 默认策略是「清空内容、保留目录」——`%TEMP%`、`Windows\Temp`、
    /// `.cargo\registry` 这些被大量程序假定存在，删掉目录本身会导致
    /// 后续写入失败。
    ///
    /// 但开发产物正相反：留一个空的 `.venv` 会让 Python 工具认成损坏的
    /// 虚拟环境，空的 `node_modules` 会让包管理器以为依赖已装好，空的
    /// worktree 目录纯粹是垃圾。这些必须整个删掉。
    /// 该类目删除时走永久删除还是废纸篓/回收站。
    ///
    /// 默认永久删除：缓存、临时文件、构建产物本来就该重建，进废纸篓
    /// 只是把占用从一个目录挪到另一个目录，用户还得再清一次。
    ///
    /// 例外是「删错了代价不对称」的类目——判据两条同时成立：
    ///
    /// 1. **误删的痛感远大于体积收益**：旧版 IDE 数据里躺着用户多年攒下
    ///    的配置、快捷键、插件设置，认错版本号删掉就没了；而它通常只有
    ///    几百 MB 到几个 GB。
    /// 2. **体积不足以撑爆废纸篓**：这一条把 `IosBackup` 排除在外——单个
    ///    备份动辄几十 GB，`recycle_path` 又刻意不往 `bytes` 上记账
    ///    （「已释放 X」必须是真的释放了才算），进废纸篓的结果是用户看到
    ///    「已释放 0 B」、磁盘一点没空出来，与他勾选这一项的目的直接相反。
    ///
    /// 注意 `BrokenLoginItems` 不在这里——它的 plist 早就在
    /// `clean_targets` 里单独走 `move_to_trash` 了，那条分支先于本字段
    /// 生效，这里不重复表达。
    #[cfg(test)]
    pub fn disposal(&self) -> crate::core::cleaner::Disposal {
        use crate::core::cleaner::Disposal;
        match self {
            CategoryId::OldIdeData => Disposal::RecycleBin,
            _ => Disposal::Permanent,
        }
    }

    #[cfg(test)]
    pub fn removes_directory(&self) -> bool {
        matches!(
            self,
            CategoryId::DevBuild | CategoryId::DevWorktrees | CategoryId::IosBackup
                | CategoryId::OldIdeData
                // 更新包是暂存产物：留一个空的 `pending/` 或
                // `update.<随机串>/` 纯粹是垃圾，更新器下次自己重建。
                | CategoryId::UpdaterPackages
                // 快照是整条虚拟路径即目标；不走 remove_dir 分支的话会被
                // clean_dir_contents 当普通目录跳过，tmutil 根本执行不到。
                | CategoryId::LocalSnapshots
                // 虚拟路径条目整体即目标，没有「目录与内容」之分；同时
                // 避免清理完成回调把非 remove_dir 目标当成「只清空了内容」
                // 而整树失效磁盘索引。
                | CategoryId::DockerImages
        )
    }

    /// 该类目是否靠发现式扫描产生（而非固定路径表）。
    ///
    /// 只有构建产物需要检索——它们散落在用户的代码目录里。AI agent
    /// 的缓存和 worktree 都在 agent 自己的目录下，走固定路径表。
    pub fn is_discovered(&self) -> bool {
        matches!(self, CategoryId::DevBuild)
    }

    /// 中文文案。**仅供日志与命令行**，界面上用 `name_lang(lang)`。
    pub fn name(&self) -> &'static str {
        self.name_lang(Language::Zh)
    }

    pub fn name_lang(&self, lang: Language) -> &'static str {
        match lang {
            Language::Zh => match self {
                CategoryId::SystemTemp => "系统临时文件",
                CategoryId::UserTemp => "用户临时文件",
                CategoryId::UserCache => "应用缓存",
                CategoryId::BrowserCache => "浏览器缓存",
                CategoryId::PackageCache => "包管理缓存",
                CategoryId::UpdaterPackages => "应用更新包",
                CategoryId::Logs => "日志与崩溃转储",
                CategoryId::RecycleBin => "回收站 / 废纸篓",
                CategoryId::Thumbnails => "缩略图缓存",
                CategoryId::BrokenLoginItems => "损坏的登录项",
                CategoryId::AiAgents => "AI 编程助手缓存",
                CategoryId::DevBuild => "项目构建产物与依赖",
                CategoryId::DevWorktrees => "AI agent 临时 worktree",
                CategoryId::LocalSnapshots => "APFS 本地快照",
                CategoryId::IosBackup => "iOS 设备备份",
                CategoryId::OldIdeData => "旧版 IDE 数据",
                CategoryId::DockerImages => "冗余 Docker 镜像",
            },
            Language::En => match self {
                CategoryId::SystemTemp => "System Temp Files",
                CategoryId::UserTemp => "User Temp Files",
                CategoryId::UserCache => "Application Cache",
                CategoryId::BrowserCache => "Browser Cache",
                CategoryId::PackageCache => "Package Manager Cache",
                CategoryId::UpdaterPackages => "Application Update Packages",
                CategoryId::Logs => "Logs & Crash Dumps",
                CategoryId::RecycleBin => "Recycle Bin / Trash",
                CategoryId::Thumbnails => "Thumbnail Cache",
                CategoryId::BrokenLoginItems => "Broken Login Items",
                CategoryId::AiAgents => "AI Assistant Cache",
                CategoryId::DevBuild => "Build Artifacts & Deps",
                CategoryId::DevWorktrees => "AI Agent Git Worktrees",
                CategoryId::LocalSnapshots => "APFS Local Snapshots",
                CategoryId::IosBackup => "iOS Device Backup",
                CategoryId::OldIdeData => "Old IDE Version Data",
                CategoryId::DockerImages => "Redundant Docker Images",
            },
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            CategoryId::SystemTemp => "🗑",
            CategoryId::UserTemp => "📂",
            CategoryId::UserCache => "📂",
            CategoryId::BrowserCache => "🌐",
            CategoryId::PackageCache => "📦",
            CategoryId::UpdaterPackages => "⬆️",
            CategoryId::Logs => "📝",
            CategoryId::RecycleBin => "♻️",
            CategoryId::Thumbnails => "🖼",
            CategoryId::BrokenLoginItems => "🚫",
            CategoryId::AiAgents => "🤖",
            CategoryId::DevBuild => "🛠",
            CategoryId::DevWorktrees => "🌿",
            CategoryId::LocalSnapshots => "📸",
            CategoryId::IosBackup => "📱",
            CategoryId::OldIdeData => "💻",
            CategoryId::DockerImages => "🐳",
        }
    }

    /// 中文文案。**仅供日志与命令行**，界面上用 `desc_lang(lang)`。
    pub fn desc(&self) -> &'static str {
        self.desc_lang(Language::Zh)
    }

    pub fn desc_lang(&self, lang: Language) -> &'static str {
        match lang {
            Language::Zh => match self {
                CategoryId::SystemTemp => "系统临时文件与系统更新残留",
                CategoryId::UserTemp => "用户主目录下的应用临时文件",
                CategoryId::UserCache => "应用明确存放在缓存目录中的可重建数据",
                CategoryId::BrowserCache => "Chrome / Edge / Safari 等浏览器的缓存数据",
                CategoryId::PackageCache => "npm / pnpm / cargo / go 等包管理器缓存",
                CategoryId::UpdaterPackages => {
                    "已下载的更新包与更新器暂存，删了最多多下一次更新，不丢任何数据"
                }
                CategoryId::Logs => "系统与应用日志、崩溃转储",
                CategoryId::RecycleBin => "回收站/废纸篓中已删除的文件",
                CategoryId::Thumbnails => "系统缩略图缓存，可安全重建",
                CategoryId::BrokenLoginItems => "引用目标已不存在或配置已失效的启动项",
                CategoryId::AiAgents => {
                    "Claude Code / Codex / Trae / Cursor 等 AI 编程工具的会话记录与缓存"
                }
                CategoryId::DevBuild => {
                    "代码目录下的 node_modules / target / .venv / bin·obj 等，可重新构建"
                }
                CategoryId::DevWorktrees => "AI agent 留下的临时 git worktree，可能含未提交改动",
                CategoryId::LocalSnapshots => "APFS 本地快照，macOS「磁盘莫名爆满」的头号原因",
                CategoryId::IosBackup => {
                    "iTunes / Finder 创建的 iOS 设备完整备份，单个可达 100 GB+"
                }
                CategoryId::OldIdeData => {
                    "Application Support 下 JetBrains 的按版本数据目录，当前版本之外的旧目录，移入废纸篓"
                }
                CategoryId::DockerImages => {
                    "悬空镜像、未被任何容器使用的镜像与同仓库旧版本标签，经 docker image rm 释放"
                }
            },
            Language::En => match self {
                CategoryId::SystemTemp => "System temporary files and update leftovers",
                CategoryId::UserTemp => "Application temporary files under user profile",
                CategoryId::UserCache => "Rebuildable data stored in application cache directories",
                CategoryId::BrowserCache => "Cache files from Chrome, Edge, Firefox, Safari",
                CategoryId::PackageCache => "Caches from npm, pnpm, Cargo, Go, pip, etc.",
                CategoryId::UpdaterPackages => {
                    "Downloaded update packages and updater staging; costs a re-download, loses no data"
                }
                CategoryId::Logs => "System and application event logs and crash dumps",
                CategoryId::RecycleBin => "Deleted files in Recycle Bin or Trash",
                CategoryId::Thumbnails => "System thumbnail cache, safe to rebuild",
                CategoryId::BrokenLoginItems => {
                    "Startup entries whose executable is missing or configuration is invalid"
                }
                CategoryId::AiAgents => {
                    "Session records and caches from Claude, Cursor, Trae, etc."
                }
                CategoryId::DevBuild => {
                    "node_modules, target, .venv, bin/obj in projects, rebuildable"
                }
                CategoryId::DevWorktrees => {
                    "Temporary worktrees created by AI agents, may contain uncommitted edits"
                }
                CategoryId::LocalSnapshots => {
                    "APFS local snapshots, the #1 cause of mysterious disk-full on macOS"
                }
                CategoryId::IosBackup => {
                    "Full iOS device backups created by iTunes / Finder, can be 100 GB+ each"
                }
                CategoryId::OldIdeData => {
                    "Per-version JetBrains data dirs under Application Support, excluding the newest, moved to Trash"
                }
                CategoryId::DockerImages => {
                    "Dangling images, tags unused by any container and old versions, freed via docker image rm"
                }
            },
        }
    }

    pub fn safety(&self) -> Safety {
        match self {
            // Windows/macOS 临时目录都可能包含正在运行的安装事务、socket 或锁。
            // 当前实现未按年龄和占用状态逐文件筛选，不能默认清理。
            CategoryId::SystemTemp => Safety::Caution,
            // 此类包含第三方应用缓存、窗口恢复状态和容器临时目录。
            // 应用可能错误地把状态放进名为 Caches/tmp 的目录，不能承诺无损。
            CategoryId::UserTemp => Safety::Caution,
            CategoryId::UserCache => Safety::Safe,
            // Service Worker、IndexedDB、Cookie 等状态数据已明确排除，剩余项
            // 只有 HTTP/代码/着色器缓存和已完成的崩溃报告。
            CategoryId::BrowserCache => Safety::Safe,
            // 类目级 Safe 是缺省值，不是保证。这一类里既有公共 registry 的本机
            // 镜像（npm、pip、uv、cargo…），也有 `go/pkg/mod`、`~/.gradle/caches`、
            // `~/.nuget/packages` 这种可能握着一份私有构件的唯一副本——后者按
            // 规范第 2 条逐个降级，判据写在 `cache.rs`。
            CategoryId::PackageCache => Safety::Safe,
            // 内容按签名判定，只可能是更新器的下载产物：唯一代价是重新下载。
            // 「刚下完、马上要装」的窗口由目标级年龄门挡住（updater.rs），
            // 不达标的叶子照样列出但不预选，所以类目级默认勾选不会撞上
            // 正在进行换版。
            CategoryId::UpdaterPackages => Safety::Safe,
            // Safe 的前提是每个目标都还像日志。整目录一个目标做不到这一点，
            // 所以 `~/Library/Logs` 按顶层子目录展开，非日志的条目各自降级
            // （`system::push_log_dir_targets`）。
            CategoryId::Logs => Safety::Safe,
            CategoryId::RecycleBin => Safety::Caution,
            CategoryId::Thumbnails => Safety::Safe,
            CategoryId::BrokenLoginItems => Safety::Safe,
            CategoryId::AiAgents => Safety::Caution,
            CategoryId::DevBuild => Safety::Caution,
            CategoryId::DevWorktrees => Safety::Danger,
            CategoryId::LocalSnapshots => Safety::Caution,
            CategoryId::IosBackup => Safety::Danger,
            // 内容是已卸载旧版本的配置/插件/缓存（当前版本的数据目录保留），
            // 但毕竟按版本永久删除，仍需用户确认。
            CategoryId::OldIdeData => Safety::Caution,
            // 「未被使用」不等于「不再需要」：用户可能特意拉了基础镜像备
            // 用，且删除按镜像永久执行，必须由用户逐项勾选。
            CategoryId::DockerImages => Safety::Caution,
        }
    }
}

/// 一个清理目标：一个具体目录路径 + 描述
///
/// `label` 是双语的：扫描在后台线程上跑，那时还不知道用户之后会切到哪种
/// 语言，而语言开关必须立刻生效、不能触发重扫。
#[derive(Clone, Debug)]
pub struct ScanTarget {
    pub operation: crate::core::rules::Operation,
    pub disposal: crate::core::cleaner::Disposal,
    pub rule: crate::core::rules::RuleRef,
    pub path: PathBuf,
    pub label: Text,
    pub category: CategoryId,
    /// 是否属于"推荐清理"。同一分类里可以同时包含可无损重建的缓存和
    /// 需要用户确认的历史/工作区数据，不能再只由分类推断。
    pub recommended: bool,
    /// 虚拟路径目标的真实体积（如 Docker 镜像）。真实路径走文件系统
    /// 称重，用不到这个字段；快照这类取不到体积的虚拟目标保持 `None`
    /// （扫描记 0）。
    pub size_hint: Option<u64>,
}

/// 返回所有类别对应的扫描目标（支持跨平台）。
///
/// `brew_cleanup_at` 来自调用方已经加载的设置，避免目标构造过程中再次读取
/// 配置文件并刷新全局白名单。
pub fn all_targets(brew_cleanup_at: Option<i64>) -> Vec<ScanTarget> {
    let snapshot = crate::core::rules::current();
    crate::core::rules::with_snapshot(snapshot, || {
        collect_targets(crate::platform::user_home(), brew_cleanup_at)
    })
}

/// 用户主目录拿不到时仍要产出与 home 无关的系统目标（Windows\\Temp、
/// APFS 快照、外接卷废纸篓、Docker）。不能整表直接 return，否则跨账户
/// 提权只丢了一个 `--orig-user-home`，系统垃圾也不扫了。
fn collect_targets(home: Option<PathBuf>, brew_cleanup_at: Option<i64>) -> Vec<ScanTarget> {
    let mut t: Vec<ScanTarget> = Vec::new();
    let home = home.as_deref();
    for provider in crate::core::rules::list("engine", "providers") {
        match provider.as_str() {
            "system" => system::push_system_targets(&mut t, home),
            "cache" => cache::push_cache_targets(&mut t, home, brew_cleanup_at),
            "browser" => {
                if let Some(home) = home {
                    browser::push_browser_targets(&mut t, home)
                }
            }
            "development" => {
                if let Some(home) = home {
                    dev::push_dev_targets(&mut t, home)
                }
            }
            "docker" => docker::push_docker_targets(&mut t),
            #[cfg(target_os = "macos")]
            "macos" => macos::push_macos_targets(&mut t, home),
            _ => {}
        }
    }
    crate::core::rules::append_path_targets(&mut t, home);
    dedupe_paths(&mut t);
    split_covered_preserves(&mut t);
    t
}

/// 展示、称重与执行只保留一条。
///
/// 两级归一化，都只看路径与操作，不看规则加载顺序：
/// 1. 同一路径合并全部约束，删除方式冲突则拒绝；
/// 2. 父子重叠把子树目标的约束并入最外层的覆盖父目标，只留父目标一条——
///    `scanner` 逐目标独立称重后直接相加，父子同时入表会让体积凭空双算。
///
/// 覆盖父目标的操作必须会删除整个子树（树、清空内容、生态缓存、worktree
/// 与废纸篓）；精确文件、原生资源和登记都不覆盖任何东西。子目标的推荐状态、
/// 保留项、冲突原因与规则引用都并入父目标，处置不同则父目标被阻止。
fn dedupe_paths(t: &mut Vec<ScanTarget>) {
    let mut positions: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut merged: Vec<ScanTarget> = Vec::new();
    for target in t.drain(..) {
        let key = crate::core::safety::norm(&target.path);
        if let Some(&index) = positions.get(&key) {
            let first: &mut ScanTarget = &mut merged[index];
            if first.operation != target.operation || first.disposal != target.disposal {
                first.rule.blocked =
                    Some("Conflicting cleanup policies for the same target".into());
                first.recommended = false;
            }
            first.rule.merge(&target.rule);
            first.recommended &= target.recommended;
        } else {
            positions.insert(key, merged.len());
            merged.push(target);
        }
    }
    let paths: Vec<String> = merged
        .iter()
        .map(|target| crate::core::safety::norm(&target.path))
        .collect();
    let operations: Vec<crate::core::rules::Operation> = merged
        .iter()
        .map(|target| target.operation.clone())
        .collect();
    let mut covered = vec![false; merged.len()];
    for index in 0..merged.len() {
        let Some(parent) = covering_ancestor(&paths, &operations, index) else {
            continue;
        };
        covered[index] = true;
        let child = merged[index].clone();
        let parent: &mut ScanTarget = &mut merged[parent];
        if parent.disposal != child.disposal {
            parent.rule.blocked = Some("Conflicting cleanup policies for the same subtree".into());
            parent.recommended = false;
        }
        parent.rule.merge(&child.rule);
        parent.recommended &= child.recommended;
    }
    *t = merged
        .into_iter()
        .enumerate()
        .filter_map(|(index, target)| (!covered[index]).then_some(target))
        .collect();
}

/// 操作是否会删除整个子树；只有这样的父目标才覆盖子目标。
fn covers_subtree(operation: &crate::core::rules::Operation) -> bool {
    use crate::core::rules::Operation;
    matches!(
        operation,
        Operation::Tree
            | Operation::Contents
            | Operation::Go
            | Operation::Pnpm
            | Operation::GitWorktree { .. }
            | Operation::Trash
    )
}

/// 最外层仍在覆盖这条路径的祖先（路径集合唯一决定，与输入顺序无关）。
fn covering_ancestor(
    paths: &[String],
    operations: &[crate::core::rules::Operation],
    index: usize,
) -> Option<usize> {
    let path = &paths[index];
    let mut found: Option<usize> = None;
    for (other, candidate) in paths.iter().enumerate() {
        if other == index || candidate == path || !covers_subtree(&operations[other]) {
            continue;
        }
        if !crate::core::safety::at_or_under(path, candidate) {
            continue;
        }
        if found.is_none_or(|current| paths[current].len() > candidate.len()) {
            found = Some(other);
        }
    }
    found
}

/// 拆分深度的上限与替换目标总量上限，避免病态布局把一次发现变成无界枚举。
const SPLIT_MAX_DEPTH: usize = 8;
const SPLIT_MAX_TARGETS: usize = 4096;

/// 覆盖父目标里含保留项时，拆成互不重叠的独立子目标。
///
/// `dedupe_paths` 会把子树目标的约束并进最外层的覆盖父目标，父目标因此可能包
/// 住某条规则声明的保留项（例如 Hermes 的 `sessions`）。整目录删除会把保留项
/// 一起带走；一律拒绝又会让兄弟目录跟着陪葬。这里按真实子项拆分：保留项原样
/// 留下，其余子项各自成为目标，发现与执行仍是一对一（每个子目标走自己的计划）。
/// 拆不动（目录读不出、超深、超预算）就保留原父目标，由计划层 `validate`
/// 拒绝并解释，绝不静默整删。
///
/// 只对 `Tree` / `Contents` 这类会删除整个子树的操作生效；精确文件、原生资源
/// 和登记不覆盖任何东西。
fn split_covered_preserves(t: &mut Vec<ScanTarget>) {
    let mut out = Vec::with_capacity(t.len());
    for target in t.drain(..) {
        if !target.rule.declares_preserve() {
            out.push(target);
            continue;
        }
        let preserve = target.rule.preserved();
        if !covers_preserve(&target, &preserve) {
            out.push(target);
            continue;
        }
        let normalized: Vec<String> = preserve
            .iter()
            .map(|path| crate::core::safety::norm(path))
            .collect();
        let mut budget = SPLIT_MAX_TARGETS;
        let mut children = Vec::new();
        if split_target(
            &target,
            &normalized,
            SPLIT_MAX_DEPTH,
            &mut budget,
            &mut children,
        ) {
            out.extend(children);
        } else {
            out.push(target);
        }
    }
    *t = out;
}

/// 目标是否会删除一个严格包含保留路径的子树。
fn covers_preserve(target: &ScanTarget, preserve: &[PathBuf]) -> bool {
    if !matches!(
        target.operation,
        crate::core::rules::Operation::Tree | crate::core::rules::Operation::Contents
    ) {
        return false;
    }
    let path = crate::core::safety::norm(&target.path);
    preserve.iter().any(|keep| {
        let keep = crate::core::safety::norm(keep);
        keep != path && crate::core::safety::at_or_under(&keep, &path)
    })
}

/// 把 `target` 的顶层子项收进 `out`，跳过保留项、保留项更深时递归。返回 false
/// ——此时 `out` 不被改动——表示目录读不出或触到上限，调用方据此保留原目标。
fn split_target(
    target: &ScanTarget,
    normalized: &[String],
    depth: usize,
    budget: &mut usize,
    out: &mut Vec<ScanTarget>,
) -> bool {
    let Ok(entries) = std::fs::read_dir(&target.path) else {
        return false;
    };
    let mut local = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            return false;
        };
        if *budget == 0 {
            return false;
        }
        let child = entry.path();
        let child_key = crate::core::safety::norm(&child);
        if let Some(keep) = normalized
            .iter()
            .find(|keep| crate::core::safety::at_or_under(keep, &child_key))
        {
            // 保留路径落在这个子项上：完全相等就整项留下，更深则递归进去，让
            // 保留项的兄弟仍然可以被清理。
            if keep == &child_key {
                continue;
            }
            if depth == 0 {
                return false;
            }
            let Some(sub) = child_target(target, &child) else {
                return false;
            };
            *budget -= 1;
            if !split_target(&sub, normalized, depth - 1, budget, &mut local) {
                return false;
            }
            continue;
        }
        let Some(child) = child_target(target, &child) else {
            return false;
        };
        *budget -= 1;
        local.push(child);
    }
    out.extend(local);
    true
}

/// 覆盖父目标拆出来的一条子目标。清空父目录会连子项一起删掉，所以 `Contents`
/// 父目标的子项要变成子树删除（或精确文件），而不是再来一次 `Contents` 留下空壳。
fn child_target(parent: &ScanTarget, child: &std::path::Path) -> Option<ScanTarget> {
    use crate::core::rules::Operation;
    let base = match &parent.operation {
        Operation::Contents => Operation::Tree,
        other => other.clone(),
    };
    let operation = if std::fs::symlink_metadata(child).is_ok_and(|md| md.is_file()) {
        Operation::File
    } else {
        base
    };
    let name = child
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())?;
    Some(ScanTarget {
        operation,
        disposal: parent.disposal,
        rule: parent.rule.clone(),
        path: child.to_path_buf(),
        label: Text::new(
            format!("{} · {}", parent.label.get(Language::Zh), name),
            format!("{} · {}", parent.label.get(Language::En), name),
        ),
        category: parent.category,
        recommended: parent.recommended,
        size_hint: None,
    })
}

pub(super) fn target(
    path: PathBuf,
    label: impl Into<Text>,
    category: CategoryId,
    operation: crate::core::rules::Operation,
    policy: (&str, &str),
) -> ScanTarget {
    target_with_recommendation(path, label, category, true, operation, policy)
}

pub(super) fn target_with_recommendation(
    path: PathBuf,
    label: impl Into<Text>,
    category: CategoryId,
    recommended: bool,
    operation: crate::core::rules::Operation,
    policy: (&str, &str),
) -> ScanTarget {
    use crate::core::cleaner::Disposal;
    use crate::core::rules::{Operation, RuleRef};
    let mut rule = RuleRef::provider(policy.0, policy.1);
    let configured = rule
        .snapshot
        .definition(policy.0)
        .provider_policies
        .get(policy.1);
    let disposal = configured.map_or(Disposal::Permanent, |policy| policy.disposal);
    let recommended = recommended && configured.is_some_and(|policy| policy.recommended);
    if disposal == Disposal::RecycleBin && !matches!(operation, Operation::File | Operation::Tree) {
        rule.blocked = Some("Provider disposal conflicts with its typed operation".into());
    }
    ScanTarget {
        operation,
        disposal,
        rule,
        path,
        label: label.into(),
        category,
        recommended,
        size_hint: None,
    }
}

/// Resource extent and policy are retained independently of the display path.
pub(super) fn target_with_size(
    path: PathBuf,
    label: impl Into<Text>,
    category: CategoryId,
    size_hint: u64,
    operation: crate::core::rules::Operation,
    policy: (&str, &str),
) -> ScanTarget {
    let mut target = target(path, label, category, operation, policy);
    target.size_hint = Some(size_hint);
    target
}

#[cfg(test)]
mod tests {
    // 归属界线：留在这里的测试都是对 `all_targets()` 产出的**整体表级不变量**做
    // 断言（不嵌套、不含敏感项、路径绝对、清理粒度），跨多个规则文件、没有单一
    // 归属；只测某一个规则文件的，写进那个文件自己的 `mod tests`。
    // `cache::push_user_cache_dirs`（`~/Library/Caches` 逐所有者展开）已 test 可见，
    // 其用例在 Windows 上也跑。Group Container 的缓存选择已迁入 macos 规则的
    // `container_directories`（由 `rules::directories` 的能力测试覆盖），
    // `macos::push_group_container_caches` 已删除——原用例引用它会让 macOS 构建
    // 编译失败（Windows 因 `cfg` 门控看不到），本批修正。
    use super::cache::push_user_cache_dirs;
    use super::*;

    #[test]
    fn runtime_provider_policy_changes_discovery_and_keeps_frozen_extent() {
        use crate::core::cleaner::Disposal;
        use crate::core::rules::{self, CleanupPlan, Operation, PlannedTarget, RuleSnapshot};
        use std::sync::Arc;
        let root = std::env::temp_dir().join(format!("qc-provider-policy-{}", std::process::id()));
        let package = root.join(".cache/uv");
        let unknown = root.join(".cache/unknown-fixture");
        let extension = root.join(".vscode/extensions/fixture.old-1.0");
        let updater = root.join("AppData/Fixture-updater");
        for path in [&package, &unknown, &extension, &updater] {
            std::fs::create_dir_all(path).unwrap();
            std::fs::write(path.join("sentinel"), b"fixture").unwrap();
        }
        let artifact = updater.join("update.zip");
        std::fs::write(&artifact, b"installer").unwrap();
        std::fs::write(
            root.join(".vscode/extensions/.obsolete"),
            br#"{"fixture.old-1.0":true}"#,
        )
        .unwrap();
        let discover = || {
            let mut targets = Vec::new();
            // `~/.cache` 布局走 cache 规则的声明式目录条目，不再受提供者策略影响。
            let snapshot = rules::current();
            let roots = rules::directories::fixture_roots(&root, &root.join("roaming"));
            targets.extend(rules::directories::scan_at(&snapshot, "cache", &roots).0);
            // 更新包产物仍由提供者的具名策略驱动（Tree 操作，可被处置策略改变）。
            super::updater::push_updater_artifacts(&mut targets, &updater, "Fixture");
            // 已迁移的编辑器清单布局走规则，不再受提供者策略影响。
            targets.extend(rules::directories::scan_at(&snapshot, "development", &roots).0);
            targets
        };
        let original = rules::snapshot();
        let before = rules::with_snapshot(original.clone(), discover);
        let old_extension = before
            .iter()
            .find(|target| target.path == extension)
            .unwrap();
        assert!(old_extension.recommended);
        assert_eq!(old_extension.operation, Operation::Tree);
        let old_artifact = before
            .iter()
            .find(|target| target.path == artifact)
            .unwrap();
        assert_eq!(old_artifact.operation, Operation::Tree);
        let plan = CleanupPlan::new(
            old_artifact.rule.clone(),
            vec![PlannedTarget {
                path: artifact.clone(),
                operation: old_artifact.operation.clone(),
                disposal: old_artifact.disposal,
                identity: crate::core::model::capture_identity(&artifact),
            }],
        );
        let mut bundle = original.bundle.clone();
        let engine = bundle
            .rules
            .iter_mut()
            .find(|rule| rule.id == "engine")
            .unwrap();
        engine.version += 1;
        let policy = engine
            .provider_policies
            .get_mut("updater_artifact")
            .unwrap();
        policy.recommended = false;
        policy.disposal = Disposal::RecycleBin;
        // 迁移后的清单布局不再引用提供者策略，改动它们也不该影响规则目标。
        let migrated = engine
            .provider_policies
            .get_mut("development_candidate")
            .unwrap();
        migrated.recommended = false;
        migrated.disposal = Disposal::RecycleBin;
        bundle.validate().unwrap();
        let next = Arc::new(RuleSnapshot { bundle });
        let after = rules::with_snapshot(next.clone(), discover);
        for target in &before {
            let changed = after.iter().find(|item| item.path == target.path).unwrap();
            assert_eq!(
                changed.operation, target.operation,
                "configuration cannot widen discovery"
            );
        }
        assert_eq!(before.len(), after.len());
        assert!(
            before
                .iter()
                .find(|target| target.path == package)
                .unwrap()
                .recommended
        );
        // 迁移后的 `~/.cache` 布局随 cache 规则的声明策略走，不再受引擎策略影响。
        let package_target = after.iter().find(|target| target.path == package).unwrap();
        assert!(package_target.recommended);
        assert_eq!(
            package_target.disposal,
            before
                .iter()
                .find(|target| target.path == package)
                .unwrap()
                .disposal
        );
        assert!(
            !after
                .iter()
                .find(|target| target.path == unknown)
                .unwrap()
                .recommended
        );
        let changed = after.iter().find(|target| target.path == artifact).unwrap();
        assert!(!changed.recommended);
        assert_eq!(changed.disposal, Disposal::RecycleBin);
        let migrated = after
            .iter()
            .find(|target| target.path == extension)
            .unwrap();
        assert!(
            migrated.recommended,
            "规则声明的清单布局不再随提供者策略失去预选"
        );
        assert_eq!(migrated.disposal, Disposal::Permanent);
        assert_eq!(plan.targets[0].disposal, Disposal::Permanent);
        assert!(Arc::ptr_eq(&plan.rule.snapshot, &original));
        let explanation = plan.explanation();
        assert_eq!(
            explanation["observations"][0]["provider_policy"][0],
            "updater_artifact"
        );
        assert_eq!(
            explanation["observations"][0]["provider_policy"][1]["recommended"],
            true
        );
        assert!(plan.validate().is_ok());
        for path in [&package, &unknown, &extension, &updater] {
            assert!(path.join("sentinel").is_file());
        }
        assert!(artifact.is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// 父子重叠只留最外层一条：约束、推荐状态与规则引用都并入父目标，
    /// 处置不同则父目标被阻止；同盘邻居与精确文件父目标不受影响。
    #[test]
    fn nested_targets_merge_into_the_covering_parent_in_any_order() {
        use crate::core::cleaner::Disposal;
        use crate::core::rules::{Operation, RuleRef};

        let root = PathBuf::from("C:/fixture/nested");
        let build = |relative: &str, operation: Operation, recommended: bool| ScanTarget {
            operation,
            disposal: Disposal::Permanent,
            rule: RuleRef::provider("engine", "cache_candidate"),
            path: root.join(relative),
            label: Text::same(relative),
            category: CategoryId::UserCache,
            recommended,
            size_hint: None,
        };
        let parent = build("parent", Operation::Contents, false);
        let child = build("parent/child", Operation::Contents, true);
        let grandchild = build("parent/child/deep", Operation::Contents, true);
        let sibling = build("neighbour", Operation::Tree, true);
        let exact = build("parent/exact", Operation::File, true);

        let normalize = |targets: Vec<ScanTarget>| {
            let mut targets = targets;
            dedupe_paths(&mut targets);
            let mut rows: Vec<String> = targets
                .iter()
                .map(|target| {
                    format!(
                        "{}|{:?}|{:?}|{}|contributors={}|blocked={:?}",
                        target.path.display(),
                        target.operation,
                        target.disposal,
                        target.recommended,
                        target.rule.contributors.len(),
                        target.rule.blocked
                    )
                })
                .collect();
            rows.sort();
            rows
        };
        let expected = normalize(vec![
            parent.clone(),
            child.clone(),
            grandchild.clone(),
            sibling.clone(),
            exact.clone(),
        ]);
        assert_eq!(expected.len(), 2, "{expected:?}");
        assert!(expected[0].contains("neighbour|Tree"), "{expected:?}");
        assert!(
            !expected.iter().any(|row| row.contains("parent/child")),
            "被父目标覆盖的子目标只统计一次：{expected:?}"
        );
        assert!(
            !expected.iter().any(|row| row.contains("parent/exact")),
            "精确文件在父目标的内容范围里，也只统计一次：{expected:?}"
        );
        assert!(
            expected[1].contains("contributors=3"),
            "父子各条贡献的规则引用都要保留：{expected:?}"
        );
        assert!(
            expected[1].starts_with("C:/fixture/nested\\parent|Contents|Permanent|false"),
            "推荐取交集：父目标本身不预选就不因合并变成预选：{expected:?}"
        );
        for order in [
            vec![
                grandchild.clone(),
                child.clone(),
                parent.clone(),
                exact.clone(),
                sibling.clone(),
            ],
            vec![
                sibling.clone(),
                exact.clone(),
                grandchild.clone(),
                parent.clone(),
                child.clone(),
            ],
        ] {
            assert_eq!(normalize(order), expected, "顺序不能改变计划与统计");
        }
        // 精确文件父目标不覆盖任何东西：它和它下面的目标都得留下。
        let file_parent = build("exact-file", Operation::File, true);
        let file_child = build("exact-file/inner", Operation::Contents, true);
        let kept = normalize(vec![file_child, file_parent]);
        assert_eq!(kept.len(), 2, "{kept:?}");
    }

    /// 处置不同不能悄悄二选一：父目标被阻止，子目标不再单独出现。
    #[test]
    fn nested_conflicting_disposal_blocks_the_parent() {
        use crate::core::cleaner::Disposal;
        use crate::core::rules::{Operation, RuleRef};

        let root = PathBuf::from("C:/fixture/nested-conflict");
        let mut parent = ScanTarget {
            operation: Operation::Contents,
            disposal: Disposal::Permanent,
            rule: RuleRef::provider("engine", "cache_candidate"),
            path: root.clone(),
            label: Text::same("parent"),
            category: CategoryId::UserCache,
            recommended: true,
            size_hint: None,
        };
        let child = ScanTarget {
            operation: Operation::Tree,
            disposal: Disposal::RecycleBin,
            rule: RuleRef::provider("engine", "old_ide_data"),
            path: root.join("child"),
            label: Text::same("child"),
            category: CategoryId::UserCache,
            recommended: true,
            size_hint: None,
        };
        let mut targets = vec![child, parent.clone()];
        dedupe_paths(&mut targets);
        assert_eq!(targets.len(), 1, "{targets:?}");
        assert!(targets[0].rule.blocked.is_some());
        assert!(!targets[0].recommended);
        parent.recommended = false;
        assert_eq!(targets[0].path, parent.path);
    }

    /// 生产目标表里不能存在父子重叠——它是「唯一统计」的长期不变量。
    #[test]
    fn production_targets_have_no_nested_pairs() {
        let targets = all_targets(None);
        assert!(!targets.is_empty());
        let mut nested = Vec::new();
        for (index, parent) in targets.iter().enumerate() {
            for (other, child) in targets.iter().enumerate() {
                if index == other {
                    continue;
                }
                let (outer, inner) = (
                    crate::core::safety::norm(&parent.path),
                    crate::core::safety::norm(&child.path),
                );
                if inner != outer && crate::core::safety::at_or_under(&inner, &outer) {
                    nested.push(format!(
                        "{} {:?} > {} {:?}",
                        parent.path.display(),
                        parent.operation,
                        child.path.display(),
                        child.operation
                    ));
                }
            }
        }
        assert_eq!(nested.len(), 0, "{nested:?}");
    }

    /// 固定目标表逐次构造一致、且没有重复物理目标——「相同物理目标只统计一次」
    /// 的表级不变量，也是「无重复全盘扫描」的前提。
    #[test]
    fn production_target_table_is_deterministic_and_duplicate_free() {
        let paths = |targets: &[ScanTarget]| {
            let mut paths: Vec<String> = targets
                .iter()
                .map(|target| crate::core::safety::norm(&target.path))
                .collect();
            paths.sort();
            paths
        };
        let first = paths(&all_targets(None));
        assert!(!first.is_empty());
        let second = paths(&all_targets(None));
        assert_eq!(first, second, "固定目标表逐次一致");
        let mut unique = first.clone();
        unique.dedup();
        assert_eq!(unique.len(), first.len(), "固定目标表无重复路径：{first:?}");
    }

    /// 被覆盖的子目标仍把它的保留项带进父目标：父目标的计划必须因此被阻止，
    /// 不能借着「子目标自己有保护」把保留路径随父目标一起删掉。
    #[test]
    fn covered_child_still_contributes_its_preserve_entries() {
        use crate::core::rules::{CleanupPlan, Operation, PlannedTarget, RuleRef};

        let root = std::env::temp_dir().join(format!("qc-nested-preserve-{}", std::process::id()));
        let parent = root.clone();
        let child = root.join("sessions");
        std::fs::create_dir_all(&child).unwrap();
        let build = |path: PathBuf, rule: RuleRef| ScanTarget {
            operation: Operation::Contents,
            disposal: crate::core::cleaner::Disposal::Permanent,
            rule,
            path,
            label: Text::same("fixture"),
            category: CategoryId::UserCache,
            recommended: true,
            size_hint: None,
        };
        // 父目标自己那条规则没有保留项；保护只可能来自被并进来的子目标。
        let mut targets = vec![
            build(
                parent.clone(),
                RuleRef::provider("engine", "cache_candidate"),
            ),
            build(child.clone(), RuleRef::new("hermes", Some(root.clone()))),
        ];
        dedupe_paths(&mut targets);
        assert_eq!(targets.len(), 1, "{targets:?}");
        let plan = CleanupPlan::new(
            targets[0].rule.clone(),
            vec![PlannedTarget {
                path: parent.clone(),
                operation: Operation::Contents,
                identity: crate::core::model::capture_identity(&parent),
                disposal: crate::core::cleaner::Disposal::Permanent,
            }],
        );
        let reason = plan.validate().unwrap_err();
        assert!(
            reason.contains("covers a preserved path")
                && ["config.yaml", "sessions", ".env"]
                    .iter()
                    .any(|name| reason.contains(name)),
            "并进来的保留项必须让父目标被阻止：{reason}"
        );
        assert!(child.is_dir());
        let _ = std::fs::remove_dir_all(root);
    }

    /// 覆盖父目标含保留项时按真实子项拆分：保留项原样留下，兄弟子项各自成为
    /// 独立目标（发现与执行仍是一对一），与输入顺序无关。
    #[test]
    fn covering_targets_split_into_independent_children_around_preserves() {
        use crate::core::rules::{Operation, RuleRef};

        let root = std::env::temp_dir().join(format!("qc-split-preserve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sessions")).unwrap();
        std::fs::create_dir_all(root.join("cache")).unwrap();
        std::fs::write(root.join("config.yaml"), b"keep").unwrap();
        std::fs::write(root.join("cache/data.bin"), b"x").unwrap();
        let build = |path: PathBuf, rule: RuleRef| ScanTarget {
            operation: Operation::Contents,
            disposal: crate::core::cleaner::Disposal::Permanent,
            rule,
            path,
            label: Text::same("fixture"),
            category: CategoryId::UserCache,
            recommended: true,
            size_hint: None,
        };
        let parent = build(root.clone(), RuleRef::provider("engine", "cache_candidate"));
        let child = build(
            root.join("sessions"),
            RuleRef::new("hermes", Some(root.clone())),
        );

        for mut targets in [
            vec![parent.clone(), child.clone()],
            vec![child.clone(), parent.clone()],
        ] {
            dedupe_paths(&mut targets);
            assert_eq!(
                targets.len(),
                1,
                "the child merges into the covering parent"
            );
            split_covered_preserves(&mut targets);
            let paths: Vec<_> = targets.iter().map(|t| t.path.clone()).collect();
            assert_eq!(
                paths,
                vec![root.join("cache")],
                "preserved sessions/config.yaml stay, the sibling cache splits out"
            );
            assert_eq!(
                targets[0].operation,
                Operation::Tree,
                "a Contents parent's children become subtree removals, not empty shells"
            );
            assert!(targets[0]
                .rule
                .preserved()
                .iter()
                .any(|p| p == &root.join("sessions")));
        }
        assert!(root.join("sessions").is_dir());
        assert!(root.join("config.yaml").is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    /// 拆不动（目录读不出）时保留原覆盖目标，交给计划层 `validate` 拒绝并解释，
    /// 绝不因为拆不了就整删。
    #[test]
    fn unsplittable_covering_target_stays_whole_for_validate_to_refuse() {
        use crate::core::rules::{Operation, RuleRef};

        let root = std::env::temp_dir().join(format!("qc-split-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut targets = vec![ScanTarget {
            operation: Operation::Contents,
            disposal: crate::core::cleaner::Disposal::Permanent,
            rule: RuleRef::new("hermes", Some(root.clone())),
            path: root.clone(),
            label: Text::same("fixture"),
            category: CategoryId::UserCache,
            recommended: true,
            size_hint: None,
        }];
        split_covered_preserves(&mut targets);
        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0].path, root,
            "an unreadable covering target stays whole so the plan layer refuses it"
        );
    }

    #[test]
    fn provider_extent_and_parameters_ignore_category_and_display_uri() {
        use crate::core::rules::Operation;
        let item = target_with_size(
            PathBuf::from("tmutil://snapshot/display-only"),
            "fixture",
            CategoryId::UserCache,
            42,
            Operation::Docker {
                reference: "sha256:fixture".into(),
            },
            ("engine", "docker_image"),
        );
        assert_eq!(
            item.operation,
            Operation::Docker {
                reference: "sha256:fixture".into()
            }
        );
        assert!(!item.recommended);
        assert_eq!(item.size_hint, Some(42));
        let missing = target(
            PathBuf::from("C:/fixture"),
            "fixture",
            CategoryId::UserCache,
            Operation::Contents,
            ("engine", "missing_fixture_policy"),
        );
        assert!(!missing.recommended);
        assert!(missing.rule.blocked.is_some());
        let conflicting = target(
            PathBuf::from("C:/fixture"),
            "fixture",
            CategoryId::OldIdeData,
            Operation::Contents,
            ("engine", "old_ide_data"),
        );
        assert!(conflicting.rule.blocked.is_some());
    }

    /// 同一条路径只能入表一次。
    ///
    /// 形状识别是**兜底规则**，和显式条目撞车是迟早的事（Chromium 叶子 vs
    /// 某条写死的路径）。撞上不是靠人盯 review，而是靠这条：先入表的那条
    /// （更具体的规则）留下，重复的那条丢掉。
    #[test]
    fn duplicate_paths_are_kept_once() {
        let mut targets = vec![
            target(
                PathBuf::from("/tmp/qc-dup/a"),
                Text::same("具体规则"),
                CategoryId::AiAgents,
                crate::core::rules::Operation::Contents,
                ("engine", "agent_history"),
            ),
            target(
                PathBuf::from("/tmp/qc-dup/a"),
                Text::same("兜底规则"),
                CategoryId::UserCache,
                crate::core::rules::Operation::Contents,
                ("engine", "browser_cache"),
            ),
            target(
                PathBuf::from("/tmp/qc-dup/b"),
                Text::same("b"),
                CategoryId::UserCache,
                crate::core::rules::Operation::Contents,
                ("engine", "browser_cache"),
            ),
        ];
        dedupe_paths(&mut targets);
        assert_eq!(targets.len(), 2);
        assert_eq!(
            targets[0].category,
            CategoryId::AiAgents,
            "应保留先入表的那条"
        );
    }

    /// 形状识别出来的目标绝不能是登录态/会话数据。
    ///
    /// 这条比 `chromium` 自己的单测更靠外一层：它扫的是**整张表**，
    /// 以后不论哪条规则把 `Cookies` / `Local Storage` / Profile 本体默认勾上，
    /// 都会在这里红。
    #[test]
    #[cfg(target_os = "macos")]
    fn default_selected_never_contains_session_state() {
        for target in all_targets(None).iter().filter(|t| t.recommended) {
            let path = target.path.to_string_lossy();
            for forbidden in [
                "/Cookies",
                "/Login Data",
                "/Local Storage",
                "/Session Storage",
                "/IndexedDB",
                "/Service Worker",
                "/WebStorage",
                "/chrome-profile",
            ] {
                assert!(
                    !path.ends_with(forbidden),
                    "{} 存的是登录态/Profile 本体，不能预选",
                    path
                );
            }
        }
    }

    /// 扫描目标之间不能有父子嵌套。
    ///
    /// `scanner::scan_fixed_inner` 逐目标独立称重后直接相加，不做嵌套去重，
    /// 所以父子同时入表会让展示给用户的可释放体积凭空翻倍。macOS 分支原先
    /// 就踩了这个：`~/Library/Caches` 整体和它下面的 Chrome / Safari / Edge /
    /// Homebrew 缓存同时是目标。
    ///
    /// 只在 macOS 上跑：Windows 的目标路径依赖 `%LOCALAPPDATA%` 等环境变量，
    /// 这里没有验证过，不能盲目让它在 Windows CI 上生效。
    #[test]
    #[cfg(target_os = "macos")]
    fn targets_do_not_nest() {
        let targets = all_targets(None);
        for (a_idx, a) in targets.iter().enumerate() {
            for (b_idx, b) in targets.iter().enumerate() {
                if a_idx == b_idx {
                    continue;
                }
                assert_ne!(
                    a.path,
                    b.path,
                    "{} 被多个规则重复归类，体积会被重复计算",
                    a.path.display()
                );
                assert!(
                    !b.path.starts_with(&a.path),
                    "{} 嵌套在 {} 里，体积会被重复计算",
                    b.path.display(),
                    a.path.display(),
                );
            }
        }
    }

    /// 默认勾选的分类中不能出现敏感的 Apple 系统服务缓存或登录会话。
    ///
    /// `HTTPStorages` 含 `.binarycookies` 登录会话；`CloudKit`、
    /// `AuthenticationServices`、`securityd` 等涉及认证、iCloud、安全服务，
    /// 清理后会导致重新登录、iCloud 同步异常等问题。用测试钉死，防止
    /// 后续修改不慎把它们加回来。
    #[test]
    #[cfg(target_os = "macos")]
    fn no_sensitive_targets_in_default_selected() {
        let targets = all_targets(None);

        // HTTPStorages 不应出现在任何清理目标中
        for t in &targets {
            let path = t.path.to_string_lossy();
            assert!(
                !path.contains("HTTPStorages"),
                "HTTPStorages 不应出现在清理目标中: {}",
                path
            );
        }

        // 敏感 Apple 缓存不应在默认勾选的分类中（只检查 ~/Library/Caches 下的）
        let sensitive_patterns = [
            "CloudKit",
            "AuthenticationServices",
            "amsaccountsd",
            "appleaccountd",
            "securityd",
            "identityservicesd",
            "protectedcloudstorage",
            "findmy",
            "ScreenTime",
            "passd",
            "HomeKit",
            "iCloud",
        ];

        for t in &targets {
            if !t.recommended {
                continue;
            }
            let path = t.path.to_string_lossy();
            if !path.contains("/Caches/") {
                continue;
            }
            for pat in &sensitive_patterns {
                assert!(
                    !path.contains(pat),
                    "敏感 Apple 缓存在默认勾选分类中: {} ({:?})",
                    path,
                    t.category
                );
            }
        }
    }

    #[test]
    fn unknown_user_cache_dirs_require_manual_selection() {
        let root = crate::core::testing::fixture("qc_ambiguous_cache");
        let cache = root.join("Library/Caches");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cache.join("JetBrains")).unwrap();
        std::fs::create_dir_all(cache.join("ms-playwright")).unwrap();
        let mut targets = Vec::new();

        push_user_cache_dirs(&mut targets, &cache);

        for path in [cache.join("JetBrains"), cache.join("ms-playwright")] {
            let target = targets
                .iter()
                .find(|target| target.path == path)
                .expect("含糊缓存仍应展示给用户");
            assert!(!target.recommended);
            assert_eq!(target.category, CategoryId::UserTemp);
        }
        let _ = std::fs::remove_dir_all(root);
    }

    /// 包缓存目标是**整个目录**，不切到「下载缓存」子层；单机可能只有一份的
    /// 那些不预选。
    ///
    /// 判据写在 `cache.rs`。切子层试过并被实测否掉：删 `~/.cargo/registry/cache`
    /// 之后带 `.cargo-ok` 的 `registry/src` 还在，`cargo build --offline` 照样
    /// 报 `failed to download`；删 `go/pkg/mod/cache` 之后按域名解包的目录还在，
    /// `GOPROXY=off go build` 照样报 `module lookup disabled`。
    ///
    /// `go/pkg/mod` 还有第二条约束：`cleaner` 靠路径后缀把它路由到
    /// `go clean -modcache`（见 `core::owner`），目标一旦指到子目录，路由就会
    /// 静默失效退回裸删。所以这里连带钉死「子层不能是目标」。
    #[test]
    #[cfg(target_os = "macos")]
    fn package_cache_targets_cover_whole_dirs_and_keep_owner_routing() {
        let home = dirs::home_dir().expect("测试需要真实 HOME");
        let targets = all_targets(None);
        let entry = |rel: &str| targets.iter().find(|t| t.path == home.join(rel));

        for rel in [
            ".npm/_cacache",
            ".pnpm-store",
            "Library/Caches/Homebrew",
            "Library/Caches/go-build",
        ] {
            let target = entry(rel).unwrap_or_else(|| panic!("{rel} 应作为公共镜像入表"));
            assert_eq!(target.category, CategoryId::PackageCache, "{rel} 归类错了");
            assert!(target.recommended, "{rel} 删了最坏只是重下，该预选");
        }

        // 可能只有本机一份：展示，但不预选
        for rel in [".cargo/registry", "go/pkg/mod", ".gradle/caches"] {
            let target = entry(rel).unwrap_or_else(|| panic!("{rel} 仍然要展示"));
            assert_eq!(target.category, CategoryId::PackageCache, "{rel} 归类错了");
            assert!(!target.recommended, "{rel} 可能只有本机一份，不能预选");
        }

        // go 目标必须正好落在 owner 路由认得的那个路径上
        let modcache = entry("go/pkg/mod").expect("go module 缓存要入表");
        assert!(
            crate::core::owner::is_go_modcache(&modcache.path),
            "go 目标不再被 `is_go_modcache` 认出：`go clean -modcache` 路由已失效"
        );

        for rel in [
            ".cargo/registry/cache",
            ".cargo/registry/src",
            ".cargo/registry/index",
            "go/pkg/mod/cache",
        ] {
            assert!(
                entry(rel).is_none(),
                "{rel} 不该单独入表——切子层既救不了离线构建，还会打断 owner 路由"
            );
        }
    }

    #[test]
    fn missing_home_keeps_system_scoped_targets() {
        let targets = collect_targets(None, None);
        #[cfg(windows)]
        {
            assert!(
                targets.iter().any(|t| t
                    .path
                    .components()
                    .any(|c| c.as_os_str() == "SoftwareDistribution")),
                "主目录未知时仍应列出 Windows 更新缓存: {targets:?}"
            );
            assert!(
                targets.iter().any(|t| t.category == CategoryId::SystemTemp),
                "主目录未知时系统临时目标不能整表消失"
            );
        }
        #[cfg(target_os = "macos")]
        {
            for t in &targets {
                let path = t.path.to_string_lossy();
                assert!(
                    !path.contains("Library/Application Support"),
                    "没有 home 不该扫用户 Application Support: {path}"
                );
                assert!(
                    !path.contains("Library/Caches"),
                    "没有 home 不该扫用户 Caches: {path}"
                );
            }
        }
    }

    #[test]
    fn all_targets_are_absolute_and_categorised() {
        for t in all_targets(None) {
            // 虚拟路径（APFS 本地快照）不是文件系统路径，跳过绝对路径检查
            if crate::core::model::is_virtual_path(&t.path) {
                // 仍然检查标签
                for lang in Language::ALL {
                    assert!(
                        !t.label.get(lang).is_empty(),
                        "{:?} 缺 {lang:?} 标签",
                        t.path
                    );
                }
                continue;
            }
            assert!(t.path.is_absolute(), "{:?} 不是绝对路径", t.path);
            // 两种语言都得有文案，别只填一半
            for lang in Language::ALL {
                assert!(
                    !t.label.get(lang).is_empty(),
                    "{:?} 缺 {lang:?} 标签",
                    t.path
                );
            }
        }
    }

    /// 每个扫描目标的**内容**都必须是可清理的。
    ///
    /// 清理走的是「清空目录内容、保留目录本身」，所以目标自身被列为
    /// 「不可删除」（如 `%TEMP%`）没问题；但如果目标落在某个**整棵子树**
    /// 受保护的路径下（如 `System32`），它的每个子项都会被判定为受保护，
    /// 结果就是界面上显示「可清理 N MB」，一点也清不掉。
    ///
    /// 用一个虚拟子项探测这件事：子项受保护 ⇔ 该目标整体不可清理。
    #[test]
    fn every_target_has_cleanable_contents() {
        for t in all_targets(None) {
            if matches!(
                t.category,
                CategoryId::RecycleBin | CategoryId::BrokenLoginItems
            ) {
                // 废纸篓和损坏登录项都走平台专用通道；后者对 /Library 下的
                // 系统级 plist 使用 Finder 授权移入废纸篓。
                continue;
            }
            if crate::core::model::is_virtual_path(&t.path) {
                // 虚拟目标（快照/Docker 镜像）不在文件系统上，没有子项可探测
                continue;
            }
            let probe = t.path.join("__probe__");
            assert!(
                !crate::core::safety::is_protected(&probe),
                "{:?} 位于受保护子树内，扫得出体积却永远清不掉",
                t.path
            );
        }
    }

    /// 打印本机实际命中的 AI agent 目录，用 `--nocapture` 查看。
    #[test]
    fn report_existing_ai_agent_targets() {
        let all = all_targets(None);
        let agent: Vec<_> = all
            .iter()
            .filter(|t| t.category.is_developer() && t.path.exists())
            .collect();
        println!("\n本机命中 {} 个开发类固定路径目标：", agent.len());
        for t in &agent {
            println!(
                "  [{:?}] {} -> {}",
                t.category,
                t.label.get(Language::Zh),
                t.path.display()
            );
        }
    }

    /// 处置方式是「删错了代价对称不对称」的表达，不是按类别大小拍脑袋。
    /// 这个测试把两条判据都钉住，防止以后有人顺手把某个大类改成废纸篓。
    #[test]
    fn disposal_routes_only_asymmetric_cost_categories_to_trash() {
        use crate::core::cleaner::Disposal;

        // 旧版 IDE 数据：误删掉的是多年配置，体积却只有几百 MB 到几 GB。
        assert_eq!(CategoryId::OldIdeData.disposal(), Disposal::RecycleBin);

        // 缓存与构建产物：本来就该重建，进废纸篓只是把占用挪个地方。
        for cat in [
            CategoryId::UserCache,
            CategoryId::BrowserCache,
            CategoryId::PackageCache,
            CategoryId::DevBuild,
            CategoryId::SystemTemp,
        ] {
            assert_eq!(
                cat.disposal(),
                Disposal::Permanent,
                "{cat:?} 是可重建产物，不该走废纸篓"
            );
        }

        // iOS 备份是刻意的例外：单个备份动辄几十 GB，而 `recycle_path`
        // 刻意不往 bytes 上记账，走废纸篓的结果是用户看到「已释放 0 B」、
        // 磁盘一点没空出来，与他勾选这一项的目的直接相反。
        assert_eq!(
            CategoryId::IosBackup.disposal(),
            Disposal::Permanent,
            "iOS 备份体积过大，进废纸篓不释放空间，等于没清"
        );
    }

    /// 嵌套保留深到两层：拆分递归进入含保留项的子树，每一层的兄弟子项各自
    /// 成为目标，保留项所在的最深处整棵留下——不是只处理第一层。
    #[test]
    fn deep_preserved_paths_recurse_and_keep_siblings() {
        use crate::core::rules::{Operation, RuleRef, RuleSnapshot};
        use std::sync::Arc;

        let root =
            std::env::temp_dir().join(format!("qc-split-deep-preserve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("level1/level2")).unwrap();
        std::fs::create_dir_all(root.join("other")).unwrap();
        std::fs::write(root.join("level1/level2/keep.txt"), b"keep").unwrap();
        std::fs::write(root.join("level1/sibling.bin"), b"x").unwrap();
        std::fs::write(root.join("other/data.bin"), b"x").unwrap();

        let original = crate::core::rules::snapshot();
        let mut bundle = original.bundle.clone();
        bundle
            .rules
            .iter_mut()
            .find(|rule| rule.id == "hermes")
            .unwrap()
            .preserve = vec!["level1/level2/keep.txt".into()];
        bundle.validate().unwrap();
        let changed = Arc::new(RuleSnapshot { bundle });

        crate::core::rules::with_snapshot(changed, || {
            let target = ScanTarget {
                operation: Operation::Contents,
                disposal: crate::core::cleaner::Disposal::Permanent,
                rule: RuleRef::new("hermes", Some(root.clone())),
                path: root.clone(),
                label: Text::same("fixture"),
                category: CategoryId::UserCache,
                recommended: true,
                size_hint: None,
            };
            let mut targets = vec![target];
            dedupe_paths(&mut targets);
            split_covered_preserves(&mut targets);

            let mut paths: Vec<_> = targets.iter().map(|t| t.path.clone()).collect();
            paths.sort();
            assert_eq!(
                paths,
                vec![root.join("level1/sibling.bin"), root.join("other")],
                "两层深的保留：兄弟在每一层都拆出来，最深处整棵留下"
            );
            // Contents 父目标的目录子项变成子树删除；文件子项保持精确文件操作。
            let dir_child = targets
                .iter()
                .find(|t| t.path == root.join("other"))
                .unwrap();
            assert_eq!(dir_child.operation, Operation::Tree);
            let file_child = targets
                .iter()
                .find(|t| t.path == root.join("level1/sibling.bin"))
                .unwrap();
            assert_eq!(file_child.operation, Operation::File);
            assert!(
                root.join("level1/level2/keep.txt").is_file(),
                "保留项必须原地不动"
            );
            assert!(
                !paths.contains(&root.join("level1/level2")),
                "保留项的父目录不能被拆成目标"
            );
        });
        let _ = std::fs::remove_dir_all(root);
    }
}
