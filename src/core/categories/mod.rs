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
                    "Application Support 下 JetBrains 的按版本数据目录，当前版本之外的旧目录，永久删除"
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
                    "Per-version JetBrains data dirs under Application Support, excluding the newest, permanently deleted"
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
pub fn all_targets() -> Vec<ScanTarget> {
    #[cfg(windows)]
    let home = crate::platform::windows::real_user_home().to_path_buf();
    #[cfg(not(windows))]
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

    let mut t: Vec<ScanTarget> = Vec::new();

    system::push_system_targets(&mut t, &home);
    cache::push_cache_targets(&mut t, &home);
    browser::push_browser_targets(&mut t, &home);
    dev::push_dev_targets(&mut t, &home);
    docker::push_docker_targets(&mut t);
    #[cfg(target_os = "macos")]
    macos::push_macos_targets(&mut t, &home);

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
    use super::cache::push_home_cache_targets;
    #[cfg(target_os = "macos")]
    use super::cache::push_user_cache_dirs;
    use super::dev::{
        push_ai_agent_targets, push_obsolete_vscode_extensions, CLI_AGENTS, ELECTRON_CACHE_DIRS,
        LOCAL_AGENT_DIRS,
    };
    #[cfg(target_os = "macos")]
    use super::helpers::is_broken_launch_agent;
    #[cfg(target_os = "macos")]
    use super::macos::push_group_container_caches;
    #[cfg(target_os = "macos")]
    use super::system::push_log_dir_targets;
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
        let targets = all_targets();
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
        let targets = all_targets();

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

    /// 绝不能出现在清理目标里的东西：配置、凭据、用户自己装的插件与技能。
    ///
    /// 这些目录名一旦被误加进 `CLI_AGENTS`，用户清一次就得重新登录、
    /// 重装插件。用测试钉死比靠 review 可靠。
    const NEVER_CLEAN: &[&str] = &[
        "settings.json",
        "config.toml",
        "auth.json",
        "oauth_creds.json",
        ".credentials.json",
        "memories",
        "prompts",
        "rules",
        "skills",
        "plugins",
        "extensions",
        "plans",
        "brain",
        "connectors",
        "CLAUDE.md",
        "AGENTS.md",
        "GEMINI.md",
    ];

    #[test]
    fn ai_agent_targets_never_touch_config_or_credentials() {
        for (dir, label, subs) in CLI_AGENTS {
            for sub in *subs {
                assert!(
                    !NEVER_CLEAN.contains(sub),
                    "{label}（{dir}）把 {sub} 列成了可清理项，这会破坏用户配置"
                );
            }
        }
        for (dir, subs, label, _) in LOCAL_AGENT_DIRS {
            for sub in *subs {
                assert!(
                    !NEVER_CLEAN.contains(sub),
                    "{label}（{dir}）把 {sub} 列成了可清理项"
                );
            }
        }
    }

    /// Electron 的会话态目录不能进清理表，否则用户会被踢下线。
    #[test]
    fn electron_cache_list_excludes_session_state() {
        for stateful in [
            "Service Worker",
            "IndexedDB",
            "Local Storage",
            "Session Storage",
        ] {
            assert!(
                !ELECTRON_CACHE_DIRS.contains(&stateful),
                "{stateful} 存的是登录态/应用状态，不能当缓存清"
            );
        }
    }

    #[test]
    fn ai_agent_recommendations_are_decided_per_target() {
        let root = std::env::temp_dir().join(format!("qc_agent_rules_{}", std::process::id()));
        let home = root.join("home");
        let local = root.join("local");
        let roaming = root.join("roaming");
        let mut targets = Vec::new();

        push_ai_agent_targets(&mut targets, &home, &local, &roaming);

        let claude_cache = home.join(".claude/cache");
        let claude_projects = home.join(".claude/projects");
        let cursor_cache = roaming.join("Cursor/Cache");
        let cursor_profiles = roaming.join("Cursor/CachedProfilesData");
        let cursor_blobs = roaming.join("Cursor/blob_storage");
        assert!(targets
            .iter()
            .any(|target| target.path == claude_cache && target.recommended));
        assert!(targets
            .iter()
            .any(|target| target.path == cursor_cache && target.recommended));
        assert!(targets
            .iter()
            .any(|target| target.path == claude_projects && !target.recommended));
        for path in [cursor_profiles, cursor_blobs] {
            assert!(targets
                .iter()
                .any(|target| target.path == path && !target.recommended));
        }
    }

    #[test]
    fn only_vscode_declared_obsolete_extensions_are_recommended() {
        let root = std::env::temp_dir().join(format!("qc_obsolete_ext_{}", std::process::id()));
        let extensions = root.join(".vscode/extensions");
        let old = extensions.join("example.tool-1.0.0");
        let current = extensions.join("example.tool-2.0.0");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(
            extensions.join(".obsolete"),
            r#"{"example.tool-1.0.0":true,"example.tool-2.0.0":false,"../escape":true}"#,
        )
        .unwrap();
        let mut targets = Vec::new();

        push_obsolete_vscode_extensions(&mut targets, &root);

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, old);
        assert!(targets[0].recommended);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unknown_and_group_container_caches_require_manual_selection() {
        let root = std::env::temp_dir().join(format!("qc_ambiguous_cache_{}", std::process::id()));
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

    /// 混装目录按内容拆开：更新包叶子进「应用更新包」，形态不明的子项各自
    /// 作为展示项入表，父目录不得再次入表。
    ///
    /// 本机对应物是 `~/Library/Caches/com.google.antigravity`——同一个目录里
    /// 既有 URLCache 的 `Cache.db`，又有 electron-updater 的 `pending/` 和
    /// `update.zip`。整目录只能取一个默认值，注定错判。
    #[test]
    #[cfg(target_os = "macos")]
    fn mixed_cache_dir_is_split_by_content() {
        let root = std::env::temp_dir().join(format!("qc_mixed_cache_{}", std::process::id()));
        let caches = root.join("Library/Caches");
        let mixed = caches.join("com.example.mixedapp");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(mixed.join("pending")).unwrap();
        std::fs::write(mixed.join("pending/app.zip"), b"pkg").unwrap();
        std::fs::write(mixed.join("update.zip"), b"pkg").unwrap();
        std::fs::write(mixed.join("current.blockmap"), b"bm").unwrap();
        std::fs::write(mixed.join("Cache.db"), b"db").unwrap();
        std::fs::create_dir_all(mixed.join("fsCachedData")).unwrap();
        // 没命中签名的目录：仍然整目录一项，不下钻
        std::fs::create_dir_all(caches.join("example.plainapp/state")).unwrap();

        let mut targets = Vec::new();
        push_user_cache_dirs(&mut targets, &caches);
        let paths: Vec<&PathBuf> = targets.iter().map(|t| &t.path).collect();

        for leaf in ["pending", "update.zip", "current.blockmap"] {
            let target = targets
                .iter()
                .find(|t| t.path == mixed.join(leaf))
                .unwrap_or_else(|| panic!("更新包叶子 {leaf} 没有入表"));
            assert_eq!(
                target.category,
                CategoryId::UpdaterPackages,
                "{leaf} 归类错了"
            );
        }
        for residual in ["Cache.db", "fsCachedData"] {
            let target = targets
                .iter()
                .find(|t| t.path == mixed.join(residual))
                .unwrap_or_else(|| panic!("拆开后 {residual} 不该从界面上消失"));
            assert_eq!(target.category, CategoryId::UserTemp);
            assert!(!target.recommended, "{residual} 形态不明，不能默认勾选");
        }
        assert!(!paths.contains(&&mixed), "父目录入了表，会和子项双算体积");

        let plain = caches.join("example.plainapp");
        assert!(paths.contains(&&plain), "未命中签名的目录仍应整目录展示");
        assert!(
            !paths.contains(&&plain.join("state")),
            "没拆开的目录不该下钻"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// 年龄门：刚下完的更新包只展示、不预选，滞留够久的才预选。
    ///
    /// Squirrel.Mac 换版时把暂存内容拷去 `/Applications`，此刻删掉它等于让
    /// 一次正在进行的更新倒退；而 mtime 早就停住的目录说明那次事务要么完成
    /// 要么被放弃，留在盘上的纯粹是垃圾。
    #[test]
    #[cfg(target_os = "macos")]
    fn fresh_update_package_is_not_preselected() {
        let root = std::env::temp_dir().join(format!("qc_updater_age_{}", std::process::id()));
        let caches = root.join("Library/Caches");
        let _ = std::fs::remove_dir_all(&root);
        for (app, days) in [("example.staleapp", 30u64), ("example.freshapp", 0)] {
            let dir = caches.join(app);
            std::fs::create_dir_all(&dir).unwrap();
            let pkg = dir.join("update.zip");
            std::fs::write(&pkg, b"pkg").unwrap();
            if days > 0 {
                backdate(&pkg, days);
            }
        }

        let mut targets = Vec::new();
        push_user_cache_dirs(&mut targets, &caches);
        let recommended = |app: &str| {
            targets
                .iter()
                .find(|t| t.path == caches.join(app).join("update.zip"))
                .map(|t| t.recommended)
        };
        assert_eq!(
            recommended("example.staleapp"),
            Some(true),
            "滞留 30 天的更新包该预选"
        );
        assert_eq!(
            recommended("example.freshapp"),
            Some(false),
            "刚下完的更新包必须仍然展示，但不能预选"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// 把文件的 mtime 往前挪 `days` 天。
    ///
    /// std 只能这样改已打开文件的修改时间，改不了目录，所以年龄门只在文件
    /// 叶子上验；目录叶子走同一把 `helpers::is_older_than`，逻辑没有分叉。
    #[cfg(target_os = "macos")]
    fn backdate(path: &std::path::Path, days: u64) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(
            std::time::SystemTime::now() - std::time::Duration::from_secs(days * 86_400),
        )
        .unwrap();
    }

    /// 只被部分认领的父目录：其余孩子必须入表。
    ///
    /// `browser.rs` 只认领 `Google/Chrome`，而旧写法把 `Google` 整个跳过，于是
    /// 兄弟子项（GoogleUpdater 的下载目录那一类）在界面上彻底隐身——看不见也
    /// 清不掉，比「不默认勾选」更糟。规范说得很清楚：展示不是成本，隐藏才是。
    #[test]
    #[cfg(target_os = "macos")]
    fn partially_claimed_parent_still_shows_its_other_children() {
        let root = std::env::temp_dir().join(format!("qc_partial_claim_{}", std::process::id()));
        let caches = root.join("Library/Caches");
        let google = caches.join("Google");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(google.join("Chrome/Default")).unwrap();
        std::fs::create_dir_all(google.join("Software Update")).unwrap();
        std::fs::create_dir_all(caches.join("Zed/logs")).unwrap();
        std::fs::write(caches.join("Zed/ranges.txt"), b"x").unwrap();
        std::fs::write(caches.join("Zed/update.zip"), b"pkg").unwrap();

        let mut targets = Vec::new();
        push_user_cache_dirs(&mut targets, &caches);
        let paths: Vec<&PathBuf> = targets.iter().map(|t| &t.path).collect();

        let sibling = google.join("Software Update");
        let target = targets
            .iter()
            .find(|t| t.path == sibling)
            .expect("未被认领的兄弟子项隐身了");
        assert_eq!(target.category, CategoryId::UserTemp);
        assert!(!target.recommended, "认不出它是什么，只能展示不能预选");
        assert!(paths.contains(&&caches.join("Zed/ranges.txt")));
        // 部分认领的目录也吃更新包探测：叶子进「应用更新包」，不会因为父目录
        // 被特殊对待就降级成展示项
        assert_eq!(
            targets
                .iter()
                .find(|t| t.path == caches.join("Zed/update.zip"))
                .map(|t| t.category),
            Some(CategoryId::UpdaterPackages)
        );

        // 已入表的孩子和父目录本身都不能再进来：父子/同名都会双算体积
        for forbidden in [
            google.clone(),
            google.join("Chrome"),
            caches.join("Zed"),
            caches.join("Zed/logs"),
        ] {
            assert!(
                !paths.contains(&&forbidden),
                "{:?} 已经由更具体的规则认领，重复入表会双算",
                forbidden
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }

    /// Apple 自己的目录不做探测。
    ///
    /// 签名表是按第三方更新器的产物形态做的，对系统守护进程没有意义；而
    /// `is_sensitive_apple_cache` 只列了确认危险的那些，其余 `com.apple.*`
    /// 并不因此就算安全。
    #[test]
    #[cfg(target_os = "macos")]
    fn apple_owned_cache_dirs_are_never_probed() {
        let root = std::env::temp_dir().join(format!("qc_apple_cache_{}", std::process::id()));
        let caches = root.join("Library/Caches");
        let daemon = caches.join("com.apple.ExampleDaemon");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(daemon.join("pending")).unwrap();
        std::fs::write(daemon.join("pending/payload.zip"), b"pkg").unwrap();
        std::fs::write(daemon.join("update.zip"), b"pkg").unwrap();

        let mut targets = Vec::new();
        push_user_cache_dirs(&mut targets, &caches);

        assert!(
            !targets
                .iter()
                .any(|t| t.category == CategoryId::UpdaterPackages),
            "com.apple.* 目录被探测了"
        );
        let target = targets
            .iter()
            .find(|t| t.path == daemon)
            .expect("com.apple.* 目录仍应整项展示");
        assert_eq!(target.category, CategoryId::UserTemp);
        assert!(!target.recommended);
        let _ = std::fs::remove_dir_all(root);
    }

    /// `~/.cache` 里确认能重建的包缓存不能和认不出的目录混在一个桶里。
    ///
    /// uv 的缓存按 XDG 落在 `~/.cache/uv`，官方就有 `uv cache clean`，删了
    /// 只是重下一遍——原来只有 `opencode` 被特判，其余一律 UserTemp 不勾，
    /// 机器上 1 GB 出头的 uv 缓存就这么躺在需要手动勾选的那一堆里。
    #[test]
    fn rebuildable_home_cache_dirs_are_package_cache() {
        let root = std::env::temp_dir().join(format!("qc_home_cache_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for name in ["uv", "pip", "some-tool-nobody-heard-of"] {
            std::fs::create_dir_all(root.join(".cache").join(name)).unwrap();
        }

        let mut targets = Vec::new();
        push_home_cache_targets(&mut targets, &root);

        for name in ["uv", "pip"] {
            let target = targets
                .iter()
                .find(|t| t.path == root.join(".cache").join(name))
                .unwrap_or_else(|| panic!("~/.cache/{name} 没有入表"));
            assert_eq!(
                target.category,
                CategoryId::PackageCache,
                "~/.cache/{name} 归类错了"
            );
            assert!(target.recommended);
        }
        let unknown = targets
            .iter()
            .find(|t| t.path == root.join(".cache").join("some-tool-nobody-heard-of"))
            .expect("表外的目录仍应展示");
        assert_eq!(unknown.category, CategoryId::UserTemp);
        assert!(!unknown.recommended);
        let _ = std::fs::remove_dir_all(root);
    }

    /// `~/Library/Logs` 按顶层子目录展开，黑名单里那几个不预选。
    ///
    /// 整目录一个目标等于把 N 个互不相干的所有者打包，用户只能全选或全不选；
    /// 实机那里躺着 `OneDrive/…/general.keystore` 和当天的崩溃报告。
    #[test]
    #[cfg(target_os = "macos")]
    fn logs_are_split_by_owner_and_hazards_stay_unpreselected() {
        let root = std::env::temp_dir().join(format!("qc_logs_{}", std::process::id()));
        let logs = root.join("Library/Logs");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(logs.join("Notion")).unwrap();
        std::fs::create_dir_all(logs.join("DiagnosticReports")).unwrap();
        std::fs::create_dir_all(logs.join("com.apple.CloudTelemetry")).unwrap();
        std::fs::create_dir_all(logs.join("OneDrive/Personal")).unwrap();
        std::fs::write(logs.join("OneDrive/Personal/general.keystore"), b"k").unwrap();
        std::fs::write(logs.join("warp.log"), b"log").unwrap();
        // 实机 `~/Library/Logs` 顶层真有这些：SQLite 的 telemetry 缓存与它的
        // 事务侧文件，名字在 Logs 里但不是日志。
        std::fs::write(logs.join("telemetryCache.otc"), b"sqlite").unwrap();
        std::fs::write(logs.join("telemetryCache.otc-wal"), b"w").unwrap();
        // 名字不在黑名单里，但内容说明它正被某个进程当数据库用
        std::fs::create_dir_all(logs.join("Telemetry")).unwrap();
        std::fs::write(logs.join("Telemetry/state.otc"), b"sqlite").unwrap();
        std::fs::write(logs.join("Telemetry/state.otc-wal"), b"w").unwrap();

        let mut targets = Vec::new();
        push_log_dir_targets(&mut targets, &logs);
        let entry = |rel: &str| targets.iter().find(|t| t.path == logs.join(rel));

        assert_eq!(entry("Notion").map(|t| t.recommended), Some(true));
        assert_eq!(
            entry("warp.log").map(|t| t.recommended),
            Some(true),
            "顶层散落的单个日志也是目标，不能因为拆分反而漏掉"
        );
        for hazard in ["DiagnosticReports", "OneDrive", "com.apple.CloudTelemetry"] {
            let target = entry(hazard).unwrap_or_else(|| panic!("{hazard} 仍然要展示"));
            assert_eq!(
                target.category,
                CategoryId::Logs,
                "{hazard} 该留在日志类目里"
            );
            assert!(!target.recommended, "{hazard} 不只有日志，不能预选");
        }
        for stray in ["telemetryCache.otc", "telemetryCache.otc-wal"] {
            let target = entry(stray).expect("散落的非日志文件仍要展示");
            assert!(
                !target.recommended,
                "{stray} 不是日志，不能因为住在 Logs 里就被默认删掉"
            );
        }
        // 名字表之外的第二道关口：按内容判定
        let telemetry = entry("Telemetry").expect("内容探测不该把目录从表里抹掉");
        assert!(
            !telemetry.recommended,
            "顶层有 SQLite 事务侧文件的目录正被进程使用，不能预选"
        );
        assert!(
            targets.iter().all(|t| t.path != logs),
            "整目录一个目标会让用户无法分别决定"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// 可能只在本机有一份的包缓存不默认勾选，公共 registry 的镜像照旧。
    ///
    /// 判据写在 `cache.rs`：这个目录是公共 registry 的本机镜像，还是本机某份
    /// 产物的唯一副本。`~/.m2/repository` 早就按这条排除了，`go/pkg/mod` 和
    /// `~/.gradle/caches` 没有理由例外。
    #[test]
    #[cfg(target_os = "macos")]
    fn single_copy_package_caches_are_not_preselected() {
        let home = dirs::home_dir().expect("测试需要真实 HOME");
        let targets = all_targets();
        let entry = |rel: &str| targets.iter().find(|t| t.path == home.join(rel));

        for rel in ["go/pkg/mod", ".gradle/caches"] {
            let target = entry(rel).unwrap_or_else(|| panic!("{rel} 仍然要展示"));
            assert_eq!(target.category, CategoryId::PackageCache);
            assert!(!target.recommended, "{rel} 可能只有本机一份，不能预选");
        }
        for rel in [
            ".npm/_cacache",
            ".cargo/registry",
            "Library/Caches/Homebrew",
            "Library/Caches/go-build",
        ] {
            let target = entry(rel).expect("公共 registry 镜像照旧入表");
            assert!(target.recommended, "{rel} 删了最坏只是重下，该预选");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn broken_launch_agent_requires_conclusive_evidence() {
        let root = std::env::temp_dir().join("qc_broken_launch_agent_tests");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let write = |name: &str, body: &str| {
            let path = root.join(name);
            std::fs::write(&path, body).unwrap();
            path
        };
        let plist = |entry: &str| {
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>{entry}</dict></plist>"#
            )
        };

        let valid = write(
            "valid.plist",
            &plist("<key>Program</key><string>/bin/launchctl</string>"),
        );
        let missing = write(
            "missing.plist",
            &plist("<key>Program</key><string>/definitely/missing/quick-cleaner</string>"),
        );
        let relative = write(
            "relative.plist",
            &plist("<key>ProgramArguments</key><array><string>tool-on-path</string></array>"),
        );
        let empty = write("empty.plist", &plist(""));
        // 语法根本不合法的 plist。原来靠一次 `plutil -lint` 预检拦下，
        // 现在预检去掉了，得确认它仍然被判为损坏（`-extract` 会失败）。
        let malformed = write("malformed.plist", "<plist><dict><key>Program");

        assert!(!is_broken_launch_agent(&valid));
        assert!(is_broken_launch_agent(&missing));
        assert!(!is_broken_launch_agent(&relative));
        assert!(is_broken_launch_agent(&empty));
        assert!(
            is_broken_launch_agent(&malformed),
            "语法非法的 plist 必须判为损坏"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn all_targets_are_absolute_and_categorised() {
        for t in all_targets() {
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
        for t in all_targets() {
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
        let all = all_targets();
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
}
