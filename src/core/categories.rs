//! 垃圾清理类别与扫描目标规则定义

use crate::core::i18n::{Language, Text};
use std::path::{Path, PathBuf};

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
    BrowserCache,
    PackageCache,
    Logs,
    RecycleBin,
    Thumbnails,
    // ---- 开发相关，默认不勾选 ----
    AiAgents,
    DevBuild,
    DevWorktrees,
    // ---- macOS 专用，默认不勾选 ----
    LocalSnapshots,
    IosBackup,
}

impl CategoryId {
    pub const ALL: [CategoryId; 12] = [
        CategoryId::SystemTemp,
        CategoryId::UserTemp,
        CategoryId::BrowserCache,
        CategoryId::PackageCache,
        CategoryId::Logs,
        CategoryId::RecycleBin,
        CategoryId::Thumbnails,
        CategoryId::AiAgents,
        CategoryId::DevBuild,
        CategoryId::DevWorktrees,
        CategoryId::LocalSnapshots,
        CategoryId::IosBackup,
    ];

    /// 扫描完成后是否默认勾选。
    ///
    /// 系统垃圾删掉只是重新生成，可以放心默认选中；开发类目不行——
    /// 删掉 `node_modules` / `target` 意味着下次构建要重来一遍，
    /// worktree 里甚至可能有没提交的改动。这些交给用户主动勾。
    pub fn default_selected(&self) -> bool {
        !self.is_developer()
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
                CategoryId::BrowserCache => "浏览器缓存",
                CategoryId::PackageCache => "包管理缓存",
                CategoryId::Logs => "日志与崩溃转储",
                CategoryId::RecycleBin => "回收站 / 废纸篓",
                CategoryId::Thumbnails => "缩略图缓存",
                CategoryId::AiAgents => "AI 编程助手缓存",
                CategoryId::DevBuild => "项目构建产物与依赖",
                CategoryId::DevWorktrees => "AI agent 临时 worktree",
                CategoryId::LocalSnapshots => "APFS 本地快照",
                CategoryId::IosBackup => "iOS 设备备份",
            },
            Language::En => match self {
                CategoryId::SystemTemp => "System Temp Files",
                CategoryId::UserTemp => "User Temp Files",
                CategoryId::BrowserCache => "Browser Cache",
                CategoryId::PackageCache => "Package Manager Cache",
                CategoryId::Logs => "Logs & Crash Dumps",
                CategoryId::RecycleBin => "Recycle Bin / Trash",
                CategoryId::Thumbnails => "Thumbnail Cache",
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
            CategoryId::BrowserCache => "🌐",
            CategoryId::PackageCache => "📦",
            CategoryId::Logs => "📝",
            CategoryId::RecycleBin => "♻️",
            CategoryId::Thumbnails => "🖼",
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
                CategoryId::BrowserCache => "Chrome / Edge / Safari 等浏览器的缓存数据",
                CategoryId::PackageCache => "npm / pnpm / cargo / go 等包管理器缓存",
                CategoryId::Logs => "系统与应用日志、崩溃转储",
                CategoryId::RecycleBin => "回收站/废纸篓中已删除的文件",
                CategoryId::Thumbnails => "系统缩略图缓存，可安全重建",
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
                CategoryId::BrowserCache => "Cache files from Chrome, Edge, Firefox, Safari",
                CategoryId::PackageCache => "Caches from npm, pnpm, Cargo, Go, pip, etc.",
                CategoryId::Logs => "System and application event logs and crash dumps",
                CategoryId::RecycleBin => "Deleted files in Recycle Bin or Trash",
                CategoryId::Thumbnails => "System thumbnail cache, safe to rebuild",
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
            CategoryId::SystemTemp => Safety::Safe,
            CategoryId::UserTemp => Safety::Safe,
            CategoryId::BrowserCache => Safety::Caution,
            CategoryId::PackageCache => Safety::Caution,
            CategoryId::Logs => Safety::Safe,
            CategoryId::RecycleBin => Safety::Caution,
            CategoryId::Thumbnails => Safety::Safe,
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
}

/// 返回所有类别对应的扫描目标（支持跨平台）。
pub fn all_targets() -> Vec<ScanTarget> {
    #[cfg(windows)]
    let home = crate::platform::windows::real_user_home().to_path_buf();
    #[cfg(not(windows))]
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

    let mut t: Vec<ScanTarget> = Vec::new();

    #[cfg(windows)]
    {
        let local = crate::platform::windows::real_user_local_appdata();
        let roaming = crate::platform::windows::real_user_roaming_appdata();
        let windows =
            PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()));

        // 系统临时
        t.push(target(
            windows.join("Temp"),
            "Windows\\Temp",
            CategoryId::SystemTemp,
        ));
        t.push(target(
            windows.join("SoftwareDistribution\\Download"),
            Text::new("Windows 更新缓存", "Windows Update cache"),
            CategoryId::SystemTemp,
        ));
        t.push(target(
            windows.join("SystemTemp"),
            "SystemTemp",
            CategoryId::SystemTemp,
        ));
        t.push(target(
            PathBuf::from("C:\\tmp"),
            "C:\\tmp",
            CategoryId::SystemTemp,
        ));

        // 用户临时（精确锚定真实前台用户）
        t.push(target(
            crate::platform::windows::real_user_temp(),
            "%TEMP%",
            CategoryId::UserTemp,
        ));
        t.push(target(
            local.join("CrashDumps"),
            Text::new("CrashDumps 崩溃转储", "CrashDumps"),
            CategoryId::UserTemp,
        ));

        // 浏览器缓存（全量覆盖 Default 及所有 Profile 1, Profile 2 ... 配置文件）
        push_chromium_browser_targets(&mut t, &local.join("Google\\Chrome\\User Data"), "Chrome");
        push_chromium_browser_targets(&mut t, &local.join("Microsoft\\Edge\\User Data"), "Edge");
        push_chromium_browser_targets(
            &mut t,
            &local.join("BraveSoftware\\Brave-Browser\\User Data"),
            "Brave",
        );

        // 包管理缓存
        t.push(target(
            local.join("npm-cache"),
            Text::new("npm 缓存", "npm cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join("npm-cache"),
            Text::new("npm 缓存 (home)", "npm cache (home)"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".pnpm-store"),
            "pnpm store",
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".pnpm-cache"),
            "pnpm cache",
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".cargo\\registry"),
            Text::new("cargo registry 缓存", "cargo registry cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".rustup\\downloads"),
            Text::new("rustup 下载缓存", "rustup downloads"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join("go\\pkg\\mod"),
            Text::new("go module 缓存", "go module cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            local.join("go-build"),
            Text::new("go build 缓存", "go build cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".bun"),
            Text::new("bun 缓存", "bun cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".gradle\\caches"),
            Text::new("gradle 缓存", "gradle cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".nuget\\packages"),
            Text::new("nuget 包缓存", "nuget package cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".m2\\repository"),
            Text::new("maven 本地仓库", "maven local repository"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".cache"),
            "~/.cache",
            CategoryId::PackageCache,
        ));
        t.push(target(
            local.join("uv\\cache"),
            Text::new("uv 缓存", "uv cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            local.join("pip\\cache"),
            Text::new("pip 缓存", "pip cache"),
            CategoryId::PackageCache,
        ));

        // 日志
        t.push(target(
            windows.join("Logs"),
            "Windows\\Logs",
            CategoryId::Logs,
        ));
        t.push(target(
            local.join("D3DSCache"),
            Text::new("D3D 着色器缓存", "D3D shader cache"),
            CategoryId::Logs,
        ));

        // 回收站（只统计真实前台用户自己的 SID 子目录）
        if let Some(sid) = crate::platform::windows::real_user_sid() {
            for letter in 'A'..='Z' {
                let rb = PathBuf::from(format!("{letter}:\\$Recycle.Bin")).join(&sid);
                if rb.exists() {
                    t.push(target(
                        rb,
                        Text::new(
                            format!("{letter}: 回收站"),
                            format!("{letter}: Recycle Bin"),
                        ),
                        CategoryId::RecycleBin,
                    ));
                }
            }
        }

        // 缩略图
        t.push(target(
            local.join("Microsoft\\Windows\\Explorer"),
            Text::new("缩略图/图标缓存", "Thumbnail / icon cache"),
            CategoryId::Thumbnails,
        ));

        // AI 编程助手的会话记录与缓存
        push_ai_agent_targets(&mut t, &home, &local, &roaming);
    }

    #[cfg(target_os = "macos")]
    {
        let cache = home.join("Library/Caches");
        let logs = home.join("Library/Logs");

        // 系统与用户临时/缓存
        t.push(target(
            PathBuf::from("/private/tmp"),
            "/private/tmp",
            CategoryId::SystemTemp,
        ));
        t.push(target(
            PathBuf::from("/private/var/tmp"),
            "/private/var/tmp",
            CategoryId::SystemTemp,
        ));

        // 浏览器缓存
        //
        // Chromium 系在 macOS 上把缓存放在 `~/Library/Caches/<产品名>` 而不是
        // bundle id 目录下：Edge 的实际缓存是 `Microsoft Edge`（GB 级），
        // `com.microsoft.edgemac` 只有 MB 级的零头。指错目录等于没清。
        // 指到产品目录而不是某个 profile，才能覆盖多 profile 的情况。
        t.push(target(
            cache.join("Google/Chrome"),
            Text::new("Chrome 缓存", "Chrome cache"),
            CategoryId::BrowserCache,
        ));
        t.push(target(
            cache.join("com.apple.Safari"),
            Text::new("Safari 缓存", "Safari cache"),
            CategoryId::BrowserCache,
        ));
        t.push(target(
            cache.join("Microsoft Edge"),
            Text::new("Edge 缓存", "Edge cache"),
            CategoryId::BrowserCache,
        ));

        // 包管理缓存
        t.push(target(
            home.join(".npm/_cacache"),
            Text::new("npm 缓存", "npm cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".cargo/registry"),
            Text::new("cargo 缓存", "cargo cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".rustup/downloads"),
            Text::new("rustup 缓存", "rustup cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join("go/pkg/mod"),
            Text::new("go 缓存", "go cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".cache"),
            "~/.cache",
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join("Library/Caches/Homebrew"),
            Text::new("Homebrew 缓存", "Homebrew cache"),
            CategoryId::PackageCache,
        ));

        // 日志
        t.push(target(logs, "~/Library/Logs", CategoryId::Logs));
        t.push(target(
            PathBuf::from("/Library/Logs"),
            "/Library/Logs",
            CategoryId::Logs,
        ));

        // 废纸篓
        t.push(target(
            home.join(".Trash"),
            Text::new("废纸篓", "Trash"),
            CategoryId::RecycleBin,
        ));

        // `~/Library/Caches` 剩下的部分。
        //
        // 这里逐个展开顶层子目录，而不是把整个 `~/Library/Caches` 作为一个目标：
        // 它和上面的浏览器 / Homebrew 缓存是父子关系，而 `scanner` 不做嵌套去重
        // （`scan_fixed_inner` 逐目标独立称重后直接相加），父子同时入表会让总量
        // 凭空翻倍。展开后顺带能按目录名给出标签，比一个不透明的大块更有用。
        push_user_cache_targets(&mut t, &cache);

        // AI 编程助手的缓存、会话残留与临时 worktree。
        //
        // macOS 上 Electron 型 agent 的缓存子目录结构与 Windows 完全一致
        // （Electron 自己保证的），只是根从 `%APPDATA%` 换成
        // `~/Library/Application Support`，`%LOCALAPPDATA%` 换成 `~/Library/Caches`。
        // CLI 型 agent（`.claude` / `.codex` 等）直接在 `~` 下，两边一样。
        let app_support = home.join("Library/Application Support");
        push_ai_agent_targets(&mut t, &home, &cache, &app_support);

        // QuickLook 缩略图缓存。
        //
        // macOS 15 上经典的 `com.apple.QuickLook.thumbnailcache` 已不存在，
        // 改为散落在 `$TMPDIR` 同级的 `C` 目录下的多个 `com.apple.quicklook.*`
        // 子目录。`$TMPDIR`（即 `/var/folders/<hash>/T`）下也有几个。
        // `<hash>` 是 per-user 的，编译期不知道，必须运行时从 `temp_dir()` 推。
        push_quicklook_thumbnail_targets(&mut t);

        // §6.2 补充清理目标

        // Xcode 开发产物（常达数十 GB）
        let developer = home.join("Library/Developer");
        t.push(target(
            developer.join("Xcode/DerivedData"),
            Text::new("Xcode DerivedData", "Xcode DerivedData"),
            CategoryId::DevBuild,
        ));
        t.push(target(
            developer.join("Xcode/iOS DeviceSupport"),
            Text::new("Xcode iOS DeviceSupport", "Xcode iOS DeviceSupport"),
            CategoryId::DevBuild,
        ));
        t.push(target(
            developer.join("Xcode/Archives"),
            Text::new("Xcode Archives", "Xcode Archives"),
            CategoryId::DevBuild,
        ));

        // 模拟器：不可用的设备镜像
        t.push(target(
            developer.join("CoreSimulator/Devices"),
            Text::new("iOS 模拟器设备", "iOS Simulator Devices"),
            CategoryId::DevBuild,
        ));

        // iOS 备份（Danger：单个可达 100 GB+，删了不可恢复）
        t.push(target(
            home.join("Library/Application Support/MobileSync/Backup"),
            Text::new("iOS 设备备份", "iOS Device Backup"),
            CategoryId::IosBackup,
        ));

        // 包管理器补充
        t.push(target(
            home.join(".gradle/caches"),
            Text::new("Gradle 缓存", "Gradle cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join("Library/pnpm/store"),
            Text::new("pnpm store", "pnpm store"),
            CategoryId::PackageCache,
        ));

        // 应用窗口状态：只存窗口位置/大小，删了无影响
        t.push(target(
            home.join("Library/Saved Application State"),
            Text::new("应用窗口状态", "Saved Application State"),
            CategoryId::UserTemp,
        ));
        // ~/Library/HTTPStorages 不加入默认清理：里面含 .binarycookies
        // 登录会话文件（Telegram、OneDrive、各种应用），删了等于把用户
        // 从一堆应用里登出。如果将来要加，必须放到默认不勾选的分类。

        // APFS 本地快照：通过 `tmutil listlocalsnapshots /` 发现，
        // 用 `tmutil deletelocalsnapshots <date>` 删除。
        // 这里只做发现，实际清理在 cleaner 模块用 `tmutil` 执行。
        push_local_snapshots(&mut t);

        // 其他卷的废纸篓：外接盘上的 `.Trashes/<uid>/`。
        // 本机废纸篓 `~/.Trash` 上面已加，但每个外接卷都有自己的 `.Trashes`，
        // 删到外接盘的文件不会出现在 `~/.Trash` 里。
        push_external_volume_trashes(&mut t);

        // §补充：参考 Mole 项目完善 macOS 清理目标

        // 更多浏览器缓存（~/Library/Caches 下的产品目录）
        // push_user_cache_targets 已展开 ~/Library/Caches 下的所有子目录，
        // 但它们被归到 UserTemp。这里把已知浏览器显式标为 BrowserCache，
        // 同时加入 CLAIMED_USER_CACHE_DIRS 避免重复。
        t.push(target(
            cache.join("Firefox"),
            Text::new("Firefox 缓存", "Firefox cache"),
            CategoryId::BrowserCache,
        ));
        t.push(target(
            cache.join("BraveSoftware"),
            Text::new("Brave 缓存", "Brave cache"),
            CategoryId::BrowserCache,
        ));
        t.push(target(
            cache.join("company.thebrowser.Browser"),
            Text::new("Arc 缓存", "Arc cache"),
            CategoryId::BrowserCache,
        ));
        t.push(target(
            cache.join("Chromium"),
            Text::new("Chromium 缓存", "Chromium cache"),
            CategoryId::BrowserCache,
        ));
        t.push(target(
            cache.join("com.operasoftware.Opera"),
            Text::new("Opera 缓存", "Opera cache"),
            CategoryId::BrowserCache,
        ));
        t.push(target(
            cache.join("com.vivaldi.Vivaldi"),
            Text::new("Vivaldi 缓存", "Vivaldi cache"),
            CategoryId::BrowserCache,
        ));

        // 浏览器 Application Support 下的缓存子目录
        // Chromium 系浏览器在 ~/Library/Application Support 下也存了大量缓存：
        // Code Cache、GPUCache、Service Worker/CacheStorage、Crashpad/completed 等。
        // 这些不在 ~/Library/Caches 下，上面的展开够不到。
        push_browser_app_support_caches(&mut t, &app_support);

        // Firefox Profile 缓存
        push_firefox_profile_caches(&mut t, &app_support);

        // Mail 附件下载（可能数 GB）
        t.push(target(
            home.join("Library/Mail Downloads"),
            Text::new("Mail 附件下载", "Mail Downloads"),
            CategoryId::UserTemp,
        ));
        t.push(target(
            home.join("Library/Containers/com.apple.mail/Data/Library/Mail Downloads"),
            Text::new("Mail 容器附件", "Mail Container Downloads"),
            CategoryId::UserTemp,
        ));

        // Group Containers 下的缓存、临时文件、日志
        // 沙盒应用共享的容器目录，很多应用在这里堆缓存。
        push_group_container_caches(&mut t, &home);

        // 不完整下载文件
        push_incomplete_downloads(&mut t, &home);

        // 系统崩溃报告：/Library/Logs 已覆盖 /Library/Logs/DiagnosticReports，
        // 不单独列出，避免嵌套重复计算。

        // DNS 缓存目录（可安全清理）
        push_dns_cache_targets(&mut t);

        // .DS_Store 文件清理（限定常见目录，不做全盘扫描）
        push_dsstore_targets(&mut t, &home);
    }

    t
}

/// `~/Library/Caches` 下已被更具体的目标认领的顶层目录。
///
/// 这些名字必须和上面 BrowserCache / PackageCache / AiAgents 目标里用的
/// 顶层段一致，否则会和 `push_user_cache_targets` 重复计数。
/// `Google` 覆盖 `Google/Chrome`；`claude-cli-nodejs` / `Zed` 等覆盖
/// `LOCAL_AGENT_DIRS` 里对应的条目。
#[cfg(target_os = "macos")]
const CLAIMED_USER_CACHE_DIRS: &[&str] = &[
    "Google",
    "com.apple.Safari",
    "Microsoft Edge",
    "Homebrew",
    // ---- 浏览器缓存，已显式标为 BrowserCache ----
    "Firefox",
    "BraveSoftware",
    "company.thebrowser.Browser",
    "Chromium",
    "com.operasoftware.Opera",
    "com.vivaldi.Vivaldi",
    // ---- LOCAL_AGENT_DIRS 里的目录名，避免与 AiAgents 双算 ----
    "claude-cli-nodejs",
    "amp",
    "Zed",
    "WorkBuddy",
    "cursor-updater",
    "antigravity-updater",
    "@genieworkbuddy-desktop-updater",
    "@makadesktop-updater",
    "@zcodedesktop-updater",
    "adspower_global-updater",
];

/// 把 `~/Library/Caches` 的顶层子目录逐个加为清理目标，跳过已被认领的。
#[cfg(target_os = "macos")]
fn push_user_cache_targets(t: &mut Vec<ScanTarget>, cache: &Path) {
    let Ok(rd) = std::fs::read_dir(cache) else {
        return;
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if CLAIMED_USER_CACHE_DIRS.contains(&name.as_str()) {
            continue;
        }
        // 跳过 Apple 系统服务缓存：这些涉及认证、iCloud、安全等关键服务，
        // 清理后可能导致重新登录、iCloud 同步异常、安全提示等问题。
        if is_sensitive_apple_cache(&name) {
            continue;
        }
        // 只要目录：Caches 顶层的散落文件通常是 App 自己的状态，不碰。
        if !entry.file_type().is_ok_and(|ft| ft.is_dir()) {
            continue;
        }
        t.push(target(
            entry.path(),
            format!("~/Library/Caches/{name}"),
            CategoryId::UserTemp,
        ));
    }
}

/// `~/Library/Caches` 下不应被默认清理的 Apple 系统服务缓存。
///
/// 这些目录涉及认证令牌、iCloud 数据、安全服务、账户信息等，
/// 盲目清理会导致用户被登出、iCloud 同步中断、安全提示弹窗等问题。
/// 它们虽然叫 "Caches"，但重建成本远高于普通应用缓存。
#[cfg(target_os = "macos")]
fn is_sensitive_apple_cache(name: &str) -> bool {
    // 精确匹配的敏感目录名
    const SENSITIVE_EXACT: &[&str] = &[
        "CloudKit",
        "com.apple.AuthenticationServicesCore.AuthenticationServicesAgent",
        "com.apple.amsaccountsd",
        "com.apple.amsengagementd",
        "com.apple.appleaccountd",
        "com.apple.securityd",
        "com.apple.identityservicesd",
        "com.apple.protectedcloudstorage.protectedcloudkeysyncing",
        "com.apple.ap.adprivacyd",
        "com.apple.findmy.fmipcore",
        "com.apple.passd",
        "com.apple.ScreenTimeAgent",
        "com.apple.ScreenTimeSettingsAgent",
        "com.apple.icloudwebd",
        "com.apple.iTunesCloud",
        "com.apple.itunescloudd",
        "com.apple.CloudTelemetry",
        "com.apple.iCloudNotificationAgent",
        "com.apple.HomeKit",
        "com.apple.gamed",
    ];

    if SENSITIVE_EXACT.contains(&name) {
        return true;
    }

    // 前缀匹配：以下前缀的目录都涉及敏感系统服务
    const SENSITIVE_PREFIXES: &[&str] = &[
        "com.apple.AuthenticationServices",
        "com.apple.ams",
        "com.apple.appleaccount",
        "com.apple.identity",
        "com.apple.protectedcloud",
        "com.apple.security",
        "com.apple.icloud",
        "com.apple.iCloud",
        "com.apple.Cloud",
        "com.apple.cloud",
        "com.apple.findmy",
        "com.apple.HomeKit",
        "com.apple.homekit",
        "com.apple.ScreenTime",
        "com.apple.screentime",
        "com.apple.passd",
        "com.apple.biome",
    ];

    SENSITIVE_PREFIXES.iter().any(|p| name.starts_with(p))
}

/// 发现 QuickLook 缩略图缓存目录并加为清理目标。
///
/// macOS 15 上缩略图缓存散落在 per-user 的 `$TMPDIR`（`/var/folders/<hash>/T`）
/// 及其同级 `C` 目录下的多个 `com.apple.quicklook.*` 子目录里。经典路径
/// `com.apple.QuickLook.thumbnailcache` 已不存在。`<hash>` 编译期不知道，
/// 用 `std::env::temp_dir()` 推：它返回 `/var/folders/<hash>/T`，
/// `parent()` 得到 `/var/folders/<hash>`，再拼 `C` 得到缓存目录。
#[cfg(target_os = "macos")]
fn push_quicklook_thumbnail_targets(t: &mut Vec<ScanTarget>) {
    let tmpdir = std::env::temp_dir();
    let Some(user_cache_root) = tmpdir.parent() else {
        return;
    };
    // 两个目录都扫：T（临时）和 C（缓存），QuickLook 在两边都写
    for root in [user_cache_root.join("C"), user_cache_root.join("T")] {
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // 只收 QuickLook 相关的目录，其余的 per-user 缓存不归这一类
            if !name.starts_with("com.apple.quicklook") {
                continue;
            }
            if !entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                continue;
            }
            t.push(target(
                entry.path(),
                Text::new(
                    format!("QuickLook 缓存 · {name}"),
                    format!("QuickLook cache · {name}"),
                ),
                CategoryId::Thumbnails,
            ));
        }
    }
}

/// 发现 APFS 本地快照。
///
/// `tmutil listlocalsnapshots /` 输出形如：
/// ```text
/// com.apple.TimeMachine.2024-01-15-123456
/// ```
/// 每个快照用一个虚拟路径 `tmutil://snapshot/<name>` 表示，
/// scanner 对这种路径走 `tmutil` 而不是文件系统枚举。
/// 实际大小无法直接获取（APFS 快照是 COW 的，共享数据块），
/// 这里用 0 占位，UI 展示时标注「快照」即可。
#[cfg(target_os = "macos")]
fn push_local_snapshots(t: &mut Vec<ScanTarget>) {
    let output = std::process::Command::new("tmutil")
        .arg("listlocalsnapshots")
        .arg("/")
        .output();
    let Ok(out) = output else {
        return;
    };
    if !out.status.success() {
        return;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        let name = line.trim();
        if name.is_empty() {
            continue;
        }
        // 虚拟路径：scanner 和 cleaner 识别 `tmutil://` 前缀
        let virtual_path = PathBuf::from(format!("tmutil://snapshot/{name}"));
        t.push(target(
            virtual_path,
            Text::new(
                format!("本地快照 · {name}"),
                format!("Local snapshot · {name}"),
            ),
            CategoryId::LocalSnapshots,
        ));
    }
}

/// 外接卷的废纸篓：`/Volumes/<volume>/.Trashes/<uid>/`。
///
/// macOS 上每个卷有自己的 `.Trashes` 目录，里面按 uid 分子目录。
/// 删到外接盘的文件不会出现在 `~/.Trash` 里，只在该卷的 `.Trashes/<uid>` 下。
/// 根卷 `/` 的废纸篓就是 `~/.Trash`，上面已加，这里只处理 `/Volumes` 下的外接盘。
#[cfg(target_os = "macos")]
fn push_external_volume_trashes(t: &mut Vec<ScanTarget>) {
    let uid = unsafe { libc::getuid() };
    let Ok(volumes) = std::fs::read_dir("/Volumes") else {
        return;
    };
    for entry in volumes.flatten() {
        let vol_path = entry.path();
        let trashes = vol_path.join(".Trashes");
        if !trashes.is_dir() {
            continue;
        }
        let user_trash = trashes.join(uid.to_string());
        if !user_trash.is_dir() {
            continue;
        }
        let vol_name = vol_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| vol_path.display().to_string());
        t.push(target(
            user_trash,
            Text::new(
                format!("废纸篓 · {vol_name}"),
                format!("Trash · {vol_name}"),
            ),
            CategoryId::RecycleBin,
        ));
    }
}

/// Chromium 系浏览器在 `~/Library/Application Support` 下的缓存子目录。
///
/// Chrome / Arc / Brave / Edge 等都基于 Chromium，缓存布局一致：
/// `<UserDataDir>/<Profile>/Code Cache`、`GPUCache`、`Service Worker/CacheStorage` 等。
/// 这些不在 `~/Library/Caches` 下，需要单独发现。
#[cfg(target_os = "macos")]
fn push_browser_app_support_caches(t: &mut Vec<ScanTarget>, app_support: &Path) {
    // Chromium 系浏览器的根目录映射：(目录名, 显示名)
    let chromium_browsers: &[(&str, &str)] = &[
        ("Google/Chrome", "Chrome"),
        ("Arc", "Arc"),
        ("BraveSoftware/Brave-Browser", "Brave"),
        ("Microsoft Edge", "Edge"),
        ("Vivaldi", "Vivaldi"),
        ("Opera", "Opera"),
    ];

    // 每个 profile 下的缓存子目录
    let cache_subdirs: &[&str] = &[
        "Code Cache",
        "GPUCache",
        "DawnCache",
        "GrShaderCache",
        "GraphiteDawnCache",
        "ShaderCache",
    ];

    for (dir, name) in chromium_browsers {
        let root = app_support.join(dir);
        if !root.is_dir() {
            continue;
        }
        // 枚举所有 profile 目录（Default, Profile 1, Profile 2 ...）
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let profile_name = entry.file_name().to_string_lossy().to_string();

            // profile 下的缓存子目录
            for sub in cache_subdirs {
                let cache_dir = path.join(sub);
                if cache_dir.is_dir() {
                    t.push(target(
                        cache_dir,
                        Text::new(
                            format!("{name} · {profile_name} · {sub}"),
                            format!("{name} · {profile_name} · {sub}"),
                        ),
                        CategoryId::BrowserCache,
                    ));
                }
            }

            // Service Worker CacheStorage
            let sw_cache = path.join("Service Worker/CacheStorage");
            if sw_cache.is_dir() {
                t.push(target(
                    sw_cache,
                    Text::new(
                        format!("{name} · {profile_name} · Service Worker"),
                        format!("{name} · {profile_name} · Service Worker"),
                    ),
                    CategoryId::BrowserCache,
                ));
            }
        }

        // Crashpad 已完成的崩溃报告
        let crashpad = root.join("Crashpad/completed");
        if crashpad.is_dir() {
            t.push(target(
                crashpad,
                Text::new(format!("{name} · Crashpad"), format!("{name} · Crashpad")),
                CategoryId::BrowserCache,
            ));
        }
    }
}

/// Firefox 的 Profile 缓存。
///
/// Firefox 把缓存放在 `~/Library/Application Support/Firefox/Profiles/<profile>/cache2`。
/// 每个 profile 是一串随机字符加名字。
#[cfg(target_os = "macos")]
fn push_firefox_profile_caches(t: &mut Vec<ScanTarget>, app_support: &Path) {
    let profiles_root = app_support.join("Firefox/Profiles");
    if !profiles_root.is_dir() {
        return;
    }
    let Ok(rd) = std::fs::read_dir(&profiles_root) else {
        return;
    };
    for entry in rd.flatten() {
        let profile_dir = entry.path();
        if !profile_dir.is_dir() {
            continue;
        }
        let cache2 = profile_dir.join("cache2");
        if cache2.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            t.push(target(
                cache2,
                Text::new(
                    format!("Firefox · {name} · cache2"),
                    format!("Firefox · {name} · cache2"),
                ),
                CategoryId::BrowserCache,
            ));
        }
    }
}

/// `~/Library/Group Containers` 下的缓存、临时文件和日志。
///
/// 沙盒应用通过 App Group 共享数据，Group Containers 下也有 Caches / tmp / Logs。
/// 跳过包含密码管理器关键词的目录（1Password、Keychain 等）。
#[cfg(target_os = "macos")]
fn push_group_container_caches(t: &mut Vec<ScanTarget>, home: &Path) {
    let group_root = home.join("Library/Group Containers");
    let Ok(rd) = std::fs::read_dir(&group_root) else {
        return;
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        // 跳过密码管理器和敏感应用
        let sensitive = [
            "1password",
            "keychain",
            "bitwarden",
            "lastpass",
            "keepass",
            "dashlane",
            "enpass",
        ];
        if sensitive.iter().any(|s| name.contains(s)) {
            continue;
        }
        let group_dir = entry.path();
        if !group_dir.is_dir() {
            continue;
        }
        // Caches 子目录
        let caches = group_dir.join("Library/Caches");
        if caches.is_dir() {
            t.push(target(
                caches,
                Text::new(
                    format!("组容器缓存 · {}", entry.file_name().to_string_lossy()),
                    format!(
                        "Group Container Cache · {}",
                        entry.file_name().to_string_lossy()
                    ),
                ),
                CategoryId::UserTemp,
            ));
        }
        // tmp 子目录
        let tmp = group_dir.join("Library/tmp");
        if tmp.is_dir() {
            t.push(target(
                tmp,
                Text::new(
                    format!("组容器临时 · {}", entry.file_name().to_string_lossy()),
                    format!(
                        "Group Container Temp · {}",
                        entry.file_name().to_string_lossy()
                    ),
                ),
                CategoryId::UserTemp,
            ));
        }
        // Logs 子目录
        let logs = group_dir.join("Library/Logs");
        if logs.is_dir() {
            t.push(target(
                logs,
                Text::new(
                    format!("组容器日志 · {}", entry.file_name().to_string_lossy()),
                    format!(
                        "Group Container Logs · {}",
                        entry.file_name().to_string_lossy()
                    ),
                ),
                CategoryId::Logs,
            ));
        }
    }
}

/// `~/Downloads` 下的不完整下载文件。
///
/// 浏览器下载中断后会留下 `.download`（Safari）、`.crdownload`（Chrome）、
/// `.part`（Firefox）等临时文件，可安全删除。
#[cfg(target_os = "macos")]
fn push_incomplete_downloads(t: &mut Vec<ScanTarget>, home: &Path) {
    let downloads = home.join("Downloads");
    if !downloads.is_dir() {
        return;
    }
    let Ok(rd) = std::fs::read_dir(&downloads) else {
        return;
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".download") || name.ends_with(".crdownload") || name.ends_with(".part") {
            t.push(target(
                entry.path(),
                Text::new(
                    format!("不完整下载 · {name}"),
                    format!("Incomplete Download · {name}"),
                ),
                CategoryId::UserTemp,
            ));
        }
    }
}

/// DNS 缓存目录。
///
/// macOS 的 DNS 缓存散落在 per-user 的 `$TMPDIR` 下的 `com.apple.dns`
/// 及相关目录中，可安全清理，系统会自动重建。
#[cfg(target_os = "macos")]
fn push_dns_cache_targets(t: &mut Vec<ScanTarget>) {
    let tmpdir = std::env::temp_dir();
    let Some(user_cache_root) = tmpdir.parent() else {
        return;
    };
    for root in [user_cache_root.join("C"), user_cache_root.join("T")] {
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if (name.starts_with("com.apple.dns") || name.starts_with("com.apple.networkd"))
                && entry.file_type().is_ok_and(|ft| ft.is_dir())
            {
                t.push(target(
                    entry.path(),
                    Text::new(format!("DNS 缓存 · {name}"), format!("DNS Cache · {name}")),
                    CategoryId::SystemTemp,
                ));
            }
        }
    }
}

/// `.DS_Store` 文件清理。
///
/// `.DS_Store` 是 Finder 自动生成的目录元数据文件，删除后 Finder 会重新生成。
/// 不做全盘扫描（太慢），只扫常见目录：桌面、文档、下载、用户根目录、Applications。
#[cfg(target_os = "macos")]
fn push_dsstore_targets(t: &mut Vec<ScanTarget>, home: &Path) {
    let scan_dirs: &[&str] = &[
        "Desktop",
        "Documents",
        "Downloads",
        "Movies",
        "Music",
        "Pictures",
        "Public",
    ];
    for dir in scan_dirs {
        let path = home.join(dir);
        if !path.is_dir() {
            continue;
        }
        // 扫描该目录（仅一层）下的 .DS_Store 文件
        let Ok(rd) = std::fs::read_dir(&path) else {
            continue;
        };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".DS_Store" {
                t.push(target(
                    entry.path(),
                    Text::new(
                        format!(".DS_Store · ~/{dir}"),
                        format!(".DS_Store · ~/{dir}"),
                    ),
                    CategoryId::UserTemp,
                ));
            }
            // 子目录里的 .DS_Store（只下一层，不做深度遍历）
            if entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                let sub_ds = entry.path().join(".DS_Store");
                if sub_ds.is_file() {
                    t.push(target(
                        sub_ds,
                        Text::new(
                            format!(".DS_Store · ~/{dir}/{name}"),
                            format!(".DS_Store · ~/{dir}/{name}"),
                        ),
                        CategoryId::UserTemp,
                    ));
                }
            }
        }
    }
}

#[cfg(windows)]
fn push_chromium_browser_targets(
    t: &mut Vec<ScanTarget>,
    user_data_dir: &std::path::Path,
    browser_name: &str,
) {
    if !user_data_dir.exists() {
        return;
    }
    // 1. 常规默认 profile
    let default_cache = user_data_dir.join("Default\\Cache");
    let default_code_cache = user_data_dir.join("Default\\Code Cache");
    t.push(target(
        default_cache,
        Text::new(
            format!("{browser_name} 缓存"),
            format!("{browser_name} cache"),
        ),
        CategoryId::BrowserCache,
    ));
    t.push(target(
        default_code_cache,
        format!("{browser_name} Code Cache"),
        CategoryId::BrowserCache,
    ));

    // 2. 动态枚举多用户 Profile（如 Profile 1, Profile 2, System Profile 等）
    if let Ok(entries) = std::fs::read_dir(user_data_dir) {
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name != "Default"
                && (name.starts_with("Profile ")
                    || name == "Guest Profile"
                    || name == "System Profile")
            {
                let cache = entry.path().join("Cache");
                let code_cache = entry.path().join("Code Cache");
                if cache.exists() || code_cache.exists() {
                    t.push(target(
                        cache,
                        Text::new(
                            format!("{browser_name} 缓存 ({name})"),
                            format!("{browser_name} cache ({name})"),
                        ),
                        CategoryId::BrowserCache,
                    ));
                    t.push(target(
                        code_cache,
                        format!("{browser_name} Code Cache ({name})"),
                        CategoryId::BrowserCache,
                    ));
                }
            }
        }
    }
}

/// CLI 型 agent：`~/.<目录>` 下可安全清理的子目录。
///
/// 这份表是照着本机实际目录逐个核对出来的，不是按命名惯例猜的。
/// 收录标准：**删掉只丢历史/缓存，不影响工具启动与身份**。
/// 因此配置（`settings.json`、`config.toml`）、凭据（`auth.json`、
/// `oauth_creds.json`）、记忆（`memories`）、已安装的插件与技能
/// （`plugins`、`skills`——本机各占约 380 MB，是最大的诱惑也是最不该动的）
/// 一律不在表内。
///
/// 平台无关：目录名和子目录名在 Windows / macOS 上一致，只有根目录
/// （`%USERPROFILE%` ↔ `~`）在调用方拼接。
const CLI_AGENTS: &[(&str, &str, &[&str])] = &[
    (
        ".claude",
        "Claude Code",
        // projects 是会话转录，file-history 是编辑快照，都属于历史而非配置
        &[
            "cache",
            "paste-cache",
            "shell-snapshots",
            "file-history",
            "projects",
            "sessions",
            "backups",
            "session-env",
            "jobs",
            "tasks",
            "daemon",
            "ide",
        ],
    ),
    (
        ".codex",
        "Codex",
        // shell_snapshots 用下划线（与本机实测一致），.claude/.workbuddy 用连字符
        &[
            "cache",
            "log",
            "tmp",
            ".tmp",
            "sessions",
            "archived_sessions",
            "attachments",
            "backup",
            "dictation-history",
            "visualizations",
            "ambient-suggestions",
            "computer-use",
            "computer-use-turn-ended",
            "node_repl",
            "process_manager",
            "mcp-oauth-locks",
            "thread-writer-locks",
            "shell_snapshots",
        ],
    ),
    (".gemini", "Gemini CLI", &["tmp", "chats", "sessions"]),
    (".qwen", "Qwen Code", &["tmp", "todos"]),
    (
        ".augment",
        "Augment",
        &[
            "tmp",
            "sessions",
            "backups",
            "checkpoint-documents",
            "observability",
        ],
    ),
    (".copilot", "Copilot CLI", &["logs", "ide", "session-state"]),
    (
        ".workbuddy",
        "WorkBuddy",
        &[
            "logs",
            "sessions",
            "shell-snapshots",
            "file-history",
            "backup",
            "audit-log",
        ],
    ),
];

/// Electron / VS Code 系应用的标准缓存子目录。
///
/// 刻意**不含** `Service Worker`、`IndexedDB`、`Local Storage`——那些存的是
/// 登录态和应用设置，清掉等于把用户踢下线。
///
/// 平台无关：这些子目录名在 Windows `%APPDATA%` 和 macOS
/// `~/Library/Application Support` 下完全一致（Electron 自己保证的）。
const ELECTRON_CACHE_DIRS: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "CachedData",
    "CachedProfilesData",
    "CachedExtensionVSIXs",
    "blob_storage",
    "logs",
    "Crashpad",
    "CrashReport",
    "fcache",
];

/// Electron 型 AI 编程应用在「 roaming 」根下的目录名。
///
/// Windows 上根是 `%APPDATA%`，macOS 上是 `~/Library/Application Support`。
/// 不存在的路径会在扫描阶段被 `path.exists()` 过滤掉，所以多列几个
/// 候选没有代价——本机没装的应用在别的机器上可能有。
const ROAMING_AGENT_APPS: &[&str] = &[
    "Claude",
    "Cursor",
    "CursorStar",
    "Trae",
    "Trae CN",
    "TRAE SOLO CN",
    "Windsurf",
    "Windsurf - Next",
    "Kiro",
    "Zed",
    "Void",
    "CodeBuddy",
    "CodeRabbit",
    "Antigravity",
    "AutoGLM",
    "WorkBuddy",
    "@genie",
    "devin",
    "Devin - Next",
    "anythingllm-desktop",
    "crush-gui",
];

/// 「 local 」根下的 agent 缓存目录：(目录名, 可清子目录, 中文展示名, 英文展示名)。
/// 子目录为空表示整个目录都是缓存；中英一致的条目两列写同一个字符串。
///
/// Windows 上根是 `%LOCALAPPDATA%`，macOS 上是 `~/Library/Caches`。
const LOCAL_AGENT_DIRS: &[(&str, &[&str], &str, &str)] = &[
    (
        "claude-cli-nodejs",
        &["Cache"],
        "Claude Code Node",
        "Claude Code Node",
    ),
    ("amp", &["logs", "traces"], "Amp", "Amp"),
    ("Zed", &["logs", "hang_traces"], "Zed", "Zed"),
    ("WorkBuddy", &["logs"], "WorkBuddy", "WorkBuddy"),
    ("cursor-updater", &[], "Cursor 更新包", "Cursor updates"),
    (
        "antigravity-updater",
        &[],
        "Antigravity 更新包",
        "Antigravity updates",
    ),
    (
        "@genieworkbuddy-desktop-updater",
        &[],
        "WorkBuddy 更新包",
        "WorkBuddy updates",
    ),
    ("@makadesktop-updater", &[], "Maka 更新包", "Maka updates"),
    (
        "@zcodedesktop-updater",
        &[],
        "zCode 更新包",
        "zCode updates",
    ),
    (
        "adspower_global-updater",
        &[],
        "AdsPower 更新包",
        "AdsPower updates",
    ),
];

/// VS Code 系编辑器里 AI 插件的全局存储（会话缓存都存这儿）。
/// 平台无关：`User/globalStorage/<ext-id>/tasks` 的相对结构两边一致。
const VSCODE_HOSTS: &[&str] = &["Code", "Trae", "Trae CN", "Cursor", "Windsurf - Next"];
/// (插件 ID, 中文展示名, 英文展示名)
const VSCODE_AI_EXTENSIONS: &[(&str, &str, &str)] = &[
    ("saoudrizwan.claude-dev", "Cline 会话缓存", "Cline sessions"),
    (
        "rooveterinaryinc.roo-cline",
        "Roo Code 会话缓存",
        "Roo Code sessions",
    ),
    (
        "kilocode.kilo-code",
        "Kilo Code 会话缓存",
        "Kilo Code sessions",
    ),
    (
        "github.copilot-chat",
        "Copilot Chat 缓存",
        "Copilot Chat cache",
    ),
];

/// 各 agent 存放临时 git worktree 的位置。
///
/// worktree 都开在 agent 自己的目录下（本机可见 `~/.codex/worktrees`
/// 与 `~/.windsurf/worktrees`），所以直接列固定路径即可，
/// 不需要为它做全盘检索。平台无关：`~/.<agent>/worktrees` 两边一致。
const AGENT_WORKTREE_DIRS: &[(&str, &str)] = &[
    (".codex", "Codex"),
    (".windsurf", "Windsurf"),
    (".claude", "Claude Code"),
    (".cursor", "Cursor"),
    (".trae", "Trae"),
    (".augment", "Augment"),
    (".workbuddy", "WorkBuddy"),
    (".gemini", "Gemini CLI"),
];

/// AI 编程助手的缓存、会话残留与临时 worktree。
///
/// 全部是固定路径——不存在的会在扫描阶段被 `path.exists()` 过滤掉，
/// 所以多列几个候选目录的代价只是一次 stat。
///
/// 平台无关：调用方传入平台对应的根目录即可——
/// - Windows: `home = %USERPROFILE%`, `local = %LOCALAPPDATA%`, `roaming = %APPDATA%`
/// - macOS:   `home = ~`, `local = ~/Library/Caches`, `roaming = ~/Library/Application Support`
fn push_ai_agent_targets(t: &mut Vec<ScanTarget>, home: &Path, local: &Path, roaming: &Path) {
    const AGENT: CategoryId = CategoryId::AiAgents;

    // ---- CLI 型 agent ----
    for (dir, label, subs) in CLI_AGENTS {
        for sub in *subs {
            t.push(target(
                home.join(dir).join(sub),
                format!("{label} · {sub}"),
                AGENT,
            ));
        }
    }

    // ---- Electron / VS Code 系应用 ----
    for app in ROAMING_AGENT_APPS {
        for cache in ELECTRON_CACHE_DIRS {
            t.push(target(
                roaming.join(app).join(cache),
                format!("{app} · {cache}"),
                AGENT,
            ));
        }
    }

    // ---- local 根下的缓存与更新包 ----
    for (dir, subs, zh, en) in LOCAL_AGENT_DIRS {
        if subs.is_empty() {
            t.push(target(local.join(dir), Text::new(*zh, *en), AGENT));
        } else {
            for sub in *subs {
                t.push(target(
                    local.join(dir).join(sub),
                    Text::new(format!("{zh} · {sub}"), format!("{en} · {sub}")),
                    AGENT,
                ));
            }
        }
    }

    // ---- VS Code 系 AI 插件的全局存储 ----
    // `User/globalStorage/<ext-id>/tasks` 的相对结构两边一致，用 join 走平台分隔符。
    for host in VSCODE_HOSTS {
        for (ext, zh, en) in VSCODE_AI_EXTENSIONS {
            t.push(target(
                roaming
                    .join(host)
                    .join("User")
                    .join("globalStorage")
                    .join(ext)
                    .join("tasks"),
                Text::new(format!("{host} · {zh}"), format!("{host} · {en}")),
                AGENT,
            ));
        }
    }

    // ---- AI agent 的临时 git worktree（单列一类，风险更高）----
    for (dir, label) in AGENT_WORKTREE_DIRS {
        for name in ["worktrees", ".worktrees"] {
            t.push(target(
                home.join(dir).join(name),
                format!("{label} · {name}"),
                CategoryId::DevWorktrees,
            ));
        }
    }
}

/// 构造一个扫描目标。
///
/// `label` 收 `impl Into<Text>`：`&str` / `String` 会走 [`Text::same`]
/// （路径、`%TEMP%`、品牌名这类中英一致的标签占大多数），需要区分语言的
/// 用 `Text::new(zh, en)` 显式传。
fn target(path: PathBuf, label: impl Into<Text>, category: CategoryId) -> ScanTarget {
    ScanTarget {
        path,
        label: label.into(),
        category,
    }
}

#[cfg(test)]
mod tests {
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
        for a in &targets {
            for b in &targets {
                if a.path == b.path {
                    continue;
                }
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
            if !t.category.default_selected() {
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
            if t.category == CategoryId::RecycleBin {
                continue; // 回收站走 SHEmptyRecycleBin 特殊通道
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
