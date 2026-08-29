//! 垃圾清理类别与扫描目标规则

mod browser;
mod cache;
mod dev;
mod docker;
mod helpers;
#[cfg(target_os = "macos")]
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
    pub fn disposal(&self) -> crate::core::cleaner::Disposal {
        use crate::core::cleaner::Disposal;
        match self {
            CategoryId::OldIdeData => Disposal::RecycleBin,
            _ => Disposal::Permanent,
        }
    }

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
    collect_targets(crate::platform::user_home(), brew_cleanup_at)
}

/// 用户主目录拿不到时仍要产出与 home 无关的系统目标（Windows\\Temp、
/// APFS 快照、外接卷废纸篓、Docker）。不能整表直接 return，否则跨账户
/// 提权只丢了一个 `--orig-user-home`，系统垃圾也不扫了。
fn collect_targets(home: Option<PathBuf>, brew_cleanup_at: Option<i64>) -> Vec<ScanTarget> {
    let mut t: Vec<ScanTarget> = Vec::new();
    let home = home.as_deref();
    system::push_system_targets(&mut t, home);
    cache::push_cache_targets(&mut t, home, brew_cleanup_at);
    if let Some(home) = home {
        browser::push_browser_targets(&mut t, home);
        dev::push_dev_targets(&mut t, home);
    }
    docker::push_docker_targets(&mut t);
    #[cfg(target_os = "macos")]
    macos::push_macos_targets(&mut t, home);
    t
}

pub(super) fn target(path: PathBuf, label: impl Into<Text>, category: CategoryId) -> ScanTarget {
    let recommended = category.default_selected();
    target_with_recommendation(path, label, category, recommended)
}

pub(super) fn target_with_recommendation(
    path: PathBuf,
    label: impl Into<Text>,
    category: CategoryId,
    recommended: bool,
) -> ScanTarget {
    ScanTarget {
        path,
        label: label.into(),
        category,
        recommended,
        size_hint: None,
    }
}

/// 带真实体积的虚拟路径目标（如 Docker 镜像）。`recommended` 语义同
/// [`target`]：跟随分类默认。
pub(super) fn target_with_size(
    path: PathBuf,
    label: impl Into<Text>,
    category: CategoryId,
    size_hint: u64,
) -> ScanTarget {
    ScanTarget {
        path,
        label: label.into(),
        category,
        recommended: category.default_selected(),
        size_hint: Some(size_hint),
    }
}

#[cfg(test)]
mod tests {
    // 归属界线：留在这里的测试都是对 `all_targets()` 产出的**整体表级不变量**做
    // 断言（不嵌套、不含敏感项、路径绝对、清理粒度），跨多个规则文件、没有单一
    // 归属；只测某一个规则文件的，写进那个文件自己的 `mod tests`。
    // `unknown_and_group_container_caches_require_manual_selection` 同时驱动
    // `cache::push_user_cache_dirs` 和 `macos::push_group_container_caches`，同属跨模块这一类。
    #[cfg(target_os = "macos")]
    use super::cache::push_user_cache_dirs;
    #[cfg(target_os = "macos")]
    use super::macos::push_group_container_caches;
    use super::*;

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

    #[cfg(target_os = "macos")]
    #[test]
    fn unknown_and_group_container_caches_require_manual_selection() {
        let root = crate::core::testing::fixture("qc_ambiguous_cache");
        let cache = root.join("Library/Caches");
        let group_cache =
            root.join("Library/Group Containers/TEAM.password-manager/Library/Caches");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cache.join("JetBrains")).unwrap();
        std::fs::create_dir_all(cache.join("ms-playwright")).unwrap();
        std::fs::create_dir_all(&group_cache).unwrap();
        let mut targets = Vec::new();

        push_user_cache_dirs(&mut targets, &cache);
        push_group_container_caches(&mut targets, &root);

        for path in [
            cache.join("JetBrains"),
            cache.join("ms-playwright"),
            group_cache,
        ] {
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
}
