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
}

impl CategoryId {
    pub const ALL: [CategoryId; 10] = [
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
            CategoryId::AiAgents | CategoryId::DevBuild | CategoryId::DevWorktrees
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
        matches!(self, CategoryId::DevBuild | CategoryId::DevWorktrees)
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
                CategoryId::AiAgents => "Claude Code / Codex / Trae / Cursor 等 AI 编程工具的会话记录与缓存",
                CategoryId::DevBuild => "代码目录下的 node_modules / target / .venv / bin·obj 等，可重新构建",
                CategoryId::DevWorktrees => "AI agent 留下的临时 git worktree，可能含未提交改动",
            },
            Language::En => match self {
                CategoryId::SystemTemp => "System temporary files and update leftovers",
                CategoryId::UserTemp => "Application temporary files under user profile",
                CategoryId::BrowserCache => "Cache files from Chrome, Edge, Firefox, Safari",
                CategoryId::PackageCache => "Caches from npm, pnpm, Cargo, Go, pip, etc.",
                CategoryId::Logs => "System and application event logs and crash dumps",
                CategoryId::RecycleBin => "Deleted files in Recycle Bin or Trash",
                CategoryId::Thumbnails => "System thumbnail cache, safe to rebuild",
                CategoryId::AiAgents => "Session records and caches from Claude, Cursor, Trae, etc.",
                CategoryId::DevBuild => "node_modules, target, .venv, bin/obj in projects, rebuildable",
                CategoryId::DevWorktrees => "Temporary worktrees created by AI agents, may contain uncommitted edits",
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
        let windows = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()));

        // 系统临时
        t.push(target(windows.join("Temp"), "Windows\\Temp", CategoryId::SystemTemp));
        t.push(target(windows.join("SoftwareDistribution\\Download"), Text::new("Windows 更新缓存", "Windows Update cache"), CategoryId::SystemTemp));
        t.push(target(windows.join("SystemTemp"), "SystemTemp", CategoryId::SystemTemp));
        t.push(target(PathBuf::from("C:\\tmp"), "C:\\tmp", CategoryId::SystemTemp));

        // 用户临时（精确锚定真实前台用户）
        t.push(target(crate::platform::windows::real_user_temp(), "%TEMP%", CategoryId::UserTemp));
        t.push(target(local.join("CrashDumps"), Text::new("CrashDumps 崩溃转储", "CrashDumps"), CategoryId::UserTemp));

        // 浏览器缓存（全量覆盖 Default 及所有 Profile 1, Profile 2 ... 配置文件）
        push_chromium_browser_targets(&mut t, &local.join("Google\\Chrome\\User Data"), "Chrome");
        push_chromium_browser_targets(&mut t, &local.join("Microsoft\\Edge\\User Data"), "Edge");
        push_chromium_browser_targets(&mut t, &local.join("BraveSoftware\\Brave-Browser\\User Data"), "Brave");

        // 包管理缓存
        t.push(target(local.join("npm-cache"), Text::new("npm 缓存", "npm cache"), CategoryId::PackageCache));
        t.push(target(home.join("npm-cache"), Text::new("npm 缓存 (home)", "npm cache (home)"), CategoryId::PackageCache));
        t.push(target(home.join(".pnpm-store"), "pnpm store", CategoryId::PackageCache));
        t.push(target(home.join(".pnpm-cache"), "pnpm cache", CategoryId::PackageCache));
        t.push(target(home.join(".cargo\\registry"), Text::new("cargo registry 缓存", "cargo registry cache"), CategoryId::PackageCache));
        t.push(target(home.join(".rustup\\downloads"), Text::new("rustup 下载缓存", "rustup downloads"), CategoryId::PackageCache));
        t.push(target(home.join("go\\pkg\\mod"), Text::new("go module 缓存", "go module cache"), CategoryId::PackageCache));
        t.push(target(local.join("go-build"), Text::new("go build 缓存", "go build cache"), CategoryId::PackageCache));
        t.push(target(home.join(".bun"), Text::new("bun 缓存", "bun cache"), CategoryId::PackageCache));
        t.push(target(home.join(".gradle\\caches"), Text::new("gradle 缓存", "gradle cache"), CategoryId::PackageCache));
        t.push(target(home.join(".nuget\\packages"), Text::new("nuget 包缓存", "nuget package cache"), CategoryId::PackageCache));
        t.push(target(home.join(".m2\\repository"), Text::new("maven 本地仓库", "maven local repository"), CategoryId::PackageCache));
        t.push(target(home.join(".cache"), "~/.cache", CategoryId::PackageCache));
        t.push(target(local.join("uv\\cache"), Text::new("uv 缓存", "uv cache"), CategoryId::PackageCache));
        t.push(target(local.join("pip\\cache"), Text::new("pip 缓存", "pip cache"), CategoryId::PackageCache));

        // 日志
        t.push(target(windows.join("Logs"), "Windows\\Logs", CategoryId::Logs));
        t.push(target(local.join("D3DSCache"), Text::new("D3D 着色器缓存", "D3D shader cache"), CategoryId::Logs));

        // 回收站（只统计真实前台用户自己的 SID 子目录）
        if let Some(sid) = crate::platform::windows::real_user_sid() {
            for letter in 'A'..='Z' {
                let rb = PathBuf::from(format!("{letter}:\\$Recycle.Bin")).join(&sid);
                if rb.exists() {
                    t.push(target(
                        rb,
                        Text::new(format!("{letter}: 回收站"), format!("{letter}: Recycle Bin")),
                        CategoryId::RecycleBin,
                    ));
                }
            }
        }

        // 缩略图
        t.push(target(local.join("Microsoft\\Windows\\Explorer"), Text::new("缩略图/图标缓存", "Thumbnail / icon cache"), CategoryId::Thumbnails));

        // AI 编程助手的会话记录与缓存
        push_ai_agent_targets(&mut t, &home, &local, &roaming);
    }

    #[cfg(target_os = "macos")]
    {
        let cache = home.join("Library/Caches");
        let logs = home.join("Library/Logs");

        // 系统与用户临时/缓存
        t.push(target(PathBuf::from("/private/tmp"), "/private/tmp", CategoryId::SystemTemp));
        t.push(target(PathBuf::from("/private/var/tmp"), "/private/var/tmp", CategoryId::SystemTemp));
        t.push(target(cache.clone(), "~/Library/Caches", CategoryId::UserTemp));

        // 浏览器缓存
        t.push(target(cache.join("Google/Chrome/Default"), Text::new("Chrome 缓存", "Chrome cache"), CategoryId::BrowserCache));
        t.push(target(cache.join("com.apple.Safari"), Text::new("Safari 缓存", "Safari cache"), CategoryId::BrowserCache));
        t.push(target(cache.join("com.microsoft.edgemac"), Text::new("Edge 缓存", "Edge cache"), CategoryId::BrowserCache));

        // 包管理缓存
        t.push(target(home.join(".npm/_cacache"), Text::new("npm 缓存", "npm cache"), CategoryId::PackageCache));
        t.push(target(home.join(".cargo/registry"), Text::new("cargo 缓存", "cargo cache"), CategoryId::PackageCache));
        t.push(target(home.join(".rustup/downloads"), Text::new("rustup 缓存", "rustup cache"), CategoryId::PackageCache));
        t.push(target(home.join("go/pkg/mod"), Text::new("go 缓存", "go cache"), CategoryId::PackageCache));
        t.push(target(home.join(".cache"), "~/.cache", CategoryId::PackageCache));
        t.push(target(home.join("Library/Caches/Homebrew"), Text::new("Homebrew 缓存", "Homebrew cache"), CategoryId::PackageCache));

        // 日志
        t.push(target(logs, "~/Library/Logs", CategoryId::Logs));
        t.push(target(PathBuf::from("/Library/Logs"), "/Library/Logs", CategoryId::Logs));

        // 废纸篓
        t.push(target(home.join(".Trash"), Text::new("废纸篓", "Trash"), CategoryId::RecycleBin));
    }

    t
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
        Text::new(format!("{browser_name} 缓存"), format!("{browser_name} cache")),
        CategoryId::BrowserCache,
    ));
    t.push(target(default_code_cache, format!("{browser_name} Code Cache"), CategoryId::BrowserCache));

    // 2. 动态枚举多用户 Profile（如 Profile 1, Profile 2, System Profile 等）
    if let Ok(entries) = std::fs::read_dir(user_data_dir) {
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name != "Default" && (name.starts_with("Profile ") || name == "Guest Profile" || name == "System Profile") {
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
                    t.push(target(code_cache, format!("{browser_name} Code Cache ({name})"), CategoryId::BrowserCache));
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
const CLI_AGENTS: &[(&str, &str, &[&str])] = &[
    (
        ".claude",
        "Claude Code",
        // projects 是会话转录，file-history 是编辑快照，都属于历史而非配置
        &["cache", "paste-cache", "shell-snapshots", "file-history", "projects",
          "sessions", "backups", "session-env", "jobs", "tasks", "daemon", "ide"],
    ),
    (
        ".codex",
        "Codex",
        &["cache", "log", "tmp", ".tmp", "sessions", "archived_sessions",
          "attachments", "backup", "dictation-history", "visualizations",
          "ambient-suggestions", "computer-use", "computer-use-turn-ended",
          "node_repl", "process_manager", "mcp-oauth-locks", "thread-writer-locks"],
    ),
    (".gemini", "Gemini CLI", &["tmp", "chats", "sessions"]),
    (".qwen", "Qwen Code", &["tmp", "todos"]),
    (
        ".augment",
        "Augment",
        &["tmp", "sessions", "backups", "checkpoint-documents", "observability"],
    ),
    (".copilot", "Copilot CLI", &["logs", "ide", "session-state"]),
    (
        ".workbuddy",
        "WorkBuddy",
        &["logs", "sessions", "shell-snapshots", "file-history", "backup", "audit-log"],
    ),
];

/// Electron / VS Code 系应用的标准缓存子目录。
///
/// 刻意**不含** `Service Worker`、`IndexedDB`、`Local Storage`——那些存的是
/// 登录态和应用设置，清掉等于把用户踢下线。
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

/// `%APPDATA%` 下 Electron 型 AI 编程应用的目录名。
const ROAMING_AGENT_APPS: &[&str] = &[
    "Claude", "Cursor", "CursorStar", "Trae", "Trae CN", "TRAE SOLO CN",
    "Windsurf", "Windsurf - Next", "Kiro", "Zed", "Void",
    "CodeBuddy", "CodeRabbit", "Antigravity", "AutoGLM", "WorkBuddy",
    "@genie", "devin", "Devin - Next", "anythingllm-desktop", "crush-gui",
];

/// `%LOCALAPPDATA%` 下的 agent 缓存目录：(目录名, 可清子目录, 中文展示名, 英文展示名)。
/// 子目录为空表示整个目录都是缓存；中英一致的条目两列写同一个字符串。
const LOCAL_AGENT_DIRS: &[(&str, &[&str], &str, &str)] = &[
    ("claude-cli-nodejs", &["Cache"], "Claude Code Node", "Claude Code Node"),
    ("amp", &["logs", "traces"], "Amp", "Amp"),
    ("Zed", &["logs", "hang_traces"], "Zed", "Zed"),
    ("WorkBuddy", &["logs"], "WorkBuddy", "WorkBuddy"),
    ("cursor-updater", &[], "Cursor 更新包", "Cursor updates"),
    ("antigravity-updater", &[], "Antigravity 更新包", "Antigravity updates"),
    ("@genieworkbuddy-desktop-updater", &[], "WorkBuddy 更新包", "WorkBuddy updates"),
    ("@makadesktop-updater", &[], "Maka 更新包", "Maka updates"),
    ("@zcodedesktop-updater", &[], "zCode 更新包", "zCode updates"),
    ("adspower_global-updater", &[], "AdsPower 更新包", "AdsPower updates"),
];

/// VS Code 系编辑器里 AI 插件的全局存储（会话缓存都存这儿）。
const VSCODE_HOSTS: &[&str] = &["Code", "Trae", "Trae CN", "Cursor", "Windsurf - Next"];
/// (插件 ID, 中文展示名, 英文展示名)
const VSCODE_AI_EXTENSIONS: &[(&str, &str, &str)] = &[
    ("saoudrizwan.claude-dev", "Cline 会话缓存", "Cline sessions"),
    ("rooveterinaryinc.roo-cline", "Roo Code 会话缓存", "Roo Code sessions"),
    ("kilocode.kilo-code", "Kilo Code 会话缓存", "Kilo Code sessions"),
    ("github.copilot-chat", "Copilot Chat 缓存", "Copilot Chat cache"),
];

/// 各 agent 存放临时 git worktree 的位置。
///
/// worktree 都开在 agent 自己的目录下（本机可见 `~/.codex/worktrees`
/// 与 `~/.windsurf/worktrees`），所以直接列固定路径即可，
/// 不需要为它做全盘检索。
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
#[cfg(windows)]
fn push_ai_agent_targets(
    t: &mut Vec<ScanTarget>,
    home: &Path,
    local: &Path,
    roaming: &Path,
) {
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

    // ---- LocalAppData 下的缓存与更新包 ----
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
    for host in VSCODE_HOSTS {
        for (ext, zh, en) in VSCODE_AI_EXTENSIONS {
            t.push(target(
                roaming
                    .join(host)
                    .join(r"User\globalStorage")
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

    /// 绝不能出现在清理目标里的东西：配置、凭据、用户自己装的插件与技能。
    ///
    /// 这些目录名一旦被误加进 `CLI_AGENTS`，用户清一次就得重新登录、
    /// 重装插件。用测试钉死比靠 review 可靠。
    const NEVER_CLEAN: &[&str] = &[
        "settings.json", "config.toml", "auth.json", "oauth_creds.json",
        ".credentials.json", "memories", "prompts", "rules", "skills",
        "plugins", "extensions", "plans", "brain", "connectors",
        "CLAUDE.md", "AGENTS.md", "GEMINI.md",
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
        for stateful in ["Service Worker", "IndexedDB", "Local Storage", "Session Storage"] {
            assert!(
                !ELECTRON_CACHE_DIRS.contains(&stateful),
                "{stateful} 存的是登录态/应用状态，不能当缓存清"
            );
        }
    }

    #[test]
    fn all_targets_are_absolute_and_categorised() {
        for t in all_targets() {
            assert!(t.path.is_absolute(), "{:?} 不是绝对路径", t.path);
            // 两种语言都得有文案，别只填一半
            for lang in Language::ALL {
                assert!(!t.label.get(lang).is_empty(), "{:?} 缺 {lang:?} 标签", t.path);
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
