//! 垃圾清理类别与扫描目标规则

mod browser;
mod cache;
mod dev;
mod helpers;
#[cfg(target_os = "macos")]
mod macos;
mod system;

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
}

impl CategoryId {
    pub const ALL: [CategoryId; 14] = [
        CategoryId::SystemTemp,
        CategoryId::UserTemp,
        CategoryId::UserCache,
        CategoryId::BrowserCache,
        CategoryId::PackageCache,
        CategoryId::Logs,
        CategoryId::RecycleBin,
        CategoryId::Thumbnails,
        CategoryId::BrokenLoginItems,
        CategoryId::AiAgents,
        CategoryId::DevBuild,
        CategoryId::DevWorktrees,
        CategoryId::LocalSnapshots,
        CategoryId::IosBackup,
    ];

    /// 扫描完成后是否默认勾选。
    ///
    /// "推荐清理"只能包含明确标为 Safe 的类别。Caution 即使通常可以
    /// 重建，也可能让用户丢失离线缓存、下载成本或废纸篓中的恢复机会，
    /// 必须由用户主动勾选。
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
                CategoryId::Logs => "日志与崩溃转储",
                CategoryId::RecycleBin => "回收站 / 废纸篓",
                CategoryId::Thumbnails => "缩略图缓存",
                CategoryId::BrokenLoginItems => "损坏的登录项",
                CategoryId::AiAgents => "AI 编程助手缓存",
                CategoryId::DevBuild => "项目构建产物与依赖",
                CategoryId::DevWorktrees => "AI agent 临时 worktree",
                CategoryId::LocalSnapshots => "APFS 本地快照",
                CategoryId::IosBackup => "iOS 设备备份",
            },
            Language::En => match self {
                CategoryId::SystemTemp => "System Temp Files",
                CategoryId::UserTemp => "User Temp Files",
                CategoryId::UserCache => "Application Cache",
                CategoryId::BrowserCache => "Browser Cache",
                CategoryId::PackageCache => "Package Manager Cache",
                CategoryId::Logs => "Logs & Crash Dumps",
                CategoryId::RecycleBin => "Recycle Bin / Trash",
                CategoryId::Thumbnails => "Thumbnail Cache",
                CategoryId::BrokenLoginItems => "Broken Login Items",
                CategoryId::AiAgents => "AI Assistant Cache",
                CategoryId::DevBuild => "Build Artifacts & Deps",
                CategoryId::DevWorktrees => "AI Agent Git Worktrees",
                CategoryId::LocalSnapshots => "APFS Local Snapshots",
                CategoryId::IosBackup => "iOS Device Backup",
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
            CategoryId::Logs => "📝",
            CategoryId::RecycleBin => "♻️",
            CategoryId::Thumbnails => "🖼",
            CategoryId::BrokenLoginItems => "🚫",
            CategoryId::AiAgents => "🤖",
            CategoryId::DevBuild => "🛠",
            CategoryId::DevWorktrees => "🌿",
            CategoryId::LocalSnapshots => "📸",
            CategoryId::IosBackup => "📱",
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
            },
            Language::En => match self {
                CategoryId::SystemTemp => "System temporary files and update leftovers",
                CategoryId::UserTemp => "Application temporary files under user profile",
                CategoryId::UserCache => "Rebuildable data stored in application cache directories",
                CategoryId::BrowserCache => "Cache files from Chrome, Edge, Firefox, Safari",
                CategoryId::PackageCache => "Caches from npm, pnpm, Cargo, Go, pip, etc.",
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
            // 只收可重新下载或生成的包缓存；本地 Maven 仓库和泛 ~/.cache
            // 不再归入此类。
            CategoryId::PackageCache => Safety::Safe,
            CategoryId::Logs => Safety::Safe,
            CategoryId::RecycleBin => Safety::Caution,
            CategoryId::Thumbnails => Safety::Safe,
            CategoryId::BrokenLoginItems => Safety::Safe,
            CategoryId::AiAgents => Safety::Caution,
            CategoryId::DevBuild => Safety::Caution,
            CategoryId::DevWorktrees => Safety::Danger,
            CategoryId::LocalSnapshots => Safety::Caution,
            CategoryId::IosBackup => Safety::Danger,
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
    }
}

#[cfg(test)]
mod tests {
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

        assert!(!is_broken_launch_agent(&valid));
        assert!(is_broken_launch_agent(&missing));
        assert!(!is_broken_launch_agent(&relative));
        assert!(is_broken_launch_agent(&empty));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn all_targets_are_absolute_and_categorised() {
        for t in all_targets() {
            // tmutil:// 虚拟路径（APFS 本地快照）不是文件系统路径，跳过绝对路径检查
            let path_str = t.path.to_string_lossy();
            if path_str.starts_with("tmutil://") {
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
