//! 开发相关：构建产物、AI 编程助手缓存、编辑器工作区

use super::{target, target_with_recommendation, ScanTarget};
use crate::core::categories::CategoryId;
use crate::core::i18n::Text;
use std::path::Path;

/// 开发相关清理目标：AI agent 缓存、构建产物、iOS 备份、编辑器工作区
pub(super) fn push_dev_targets(t: &mut Vec<ScanTarget>, home: &Path) {
    #[cfg(windows)]
    {
        let local = crate::platform::windows::real_user_local_appdata();
        let roaming = crate::platform::windows::real_user_roaming_appdata();
        // AI 编程助手的会话记录与缓存
        push_ai_agent_targets(t, home, &local, &roaming);
    }

    #[cfg(target_os = "macos")]
    {
        let cache = home.join("Library/Caches");
        let app_support = home.join("Library/Application Support");

        // AI 编程助手的缓存、会话残留与临时 worktree。
        //
        // macOS 上 Electron 型 agent 的缓存子目录结构与 Windows 完全一致
        // （Electron 自己保证的），只是根从 `%APPDATA%` 换成
        // `~/Library/Application Support`，`%LOCALAPPDATA%` 换成 `~/Library/Caches`。
        // CLI 型 agent（`.claude` / `.codex` 等）直接在 `~` 下，两边一样。
        push_ai_agent_targets(t, home, &cache, &app_support);

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
        // Xcode/Archives 可能是唯一留存的发布归档；CoreSimulator/Devices
        // 包含仍在使用的模拟器及其中的应用数据。两者都不能仅凭目录位置
        // 判定为构建垃圾，因此不进入智能清理候选。

        // iOS 备份（Danger：单个可达 100 GB+，删了不可恢复）
        t.push(target(
            home.join("Library/Application Support/MobileSync/Backup"),
            Text::new("iOS 设备备份", "iOS Device Backup"),
            CategoryId::IosBackup,
        ));
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
pub(super) const CLI_AGENTS: &[(&str, &str, &[&str])] = &[
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
pub(super) const ELECTRON_CACHE_DIRS: &[&str] = &[
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
pub(super) const ROAMING_AGENT_APPS: &[&str] = &[
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
///
/// electron-updater 的更新包目录不在这里：它们的处置和普通缓存不同（要按
/// 子项拆开、还要过年龄门），两平台都由 `updater::push_updater_dirs_under`
/// / `push_user_cache_dirs` 探测内容认领，按名字登记反而会漏掉新应用。
pub(super) const LOCAL_AGENT_DIRS: &[(&str, &[&str], &str, &str)] = &[
    (
        "claude-cli-nodejs",
        &["Cache"],
        "Claude Code Node",
        "Claude Code Node",
    ),
    ("amp", &["logs", "traces"], "Amp", "Amp"),
    ("Zed", &["logs", "hang_traces"], "Zed", "Zed"),
    ("WorkBuddy", &["logs"], "WorkBuddy", "WorkBuddy"),
];

/// VS Code 系编辑器里 AI 插件的全局存储（会话缓存都存这儿）。
/// 平台无关：`User/globalStorage/<ext-id>/tasks` 的相对结构两边一致。
pub(super) const VSCODE_HOSTS: &[&str] = &["Code", "Trae", "Trae CN", "Cursor", "Windsurf - Next"];
/// (插件 ID, 中文展示名, 英文展示名)
pub(super) const VSCODE_AI_EXTENSIONS: &[(&str, &str, &str)] = &[
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
pub(super) const AGENT_WORKTREE_DIRS: &[(&str, &str)] = &[
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
pub(super) fn push_ai_agent_targets(
    t: &mut Vec<ScanTarget>,
    home: &Path,
    local: &Path,
    roaming: &Path,
) {
    const AGENT: CategoryId = CategoryId::AiAgents;

    // ---- CLI 型 agent ----
    for (dir, label, subs) in CLI_AGENTS {
        for sub in *subs {
            t.push(target_with_recommendation(
                home.join(dir).join(sub),
                format!("{label} · {sub}"),
                AGENT,
                matches!(*sub, "cache" | "log" | "logs" | "observability"),
            ));
        }
    }

    // ---- Electron / VS Code 系应用 ----
    for app in ROAMING_AGENT_APPS {
        for cache in ELECTRON_CACHE_DIRS {
            t.push(target_with_recommendation(
                roaming.join(app).join(cache),
                format!("{app} · {cache}"),
                AGENT,
                // CachedProfilesData 可能保存本地唯一的编辑器 Profile，
                // blob_storage 也可能承载未保存的附件或草稿。两者继续展示，
                // 但不能默认勾选。
                !matches!(*cache, "CachedProfilesData" | "blob_storage"),
            ));
        }
    }

    // ---- local 根下的缓存与更新包 ----
    for (dir, subs, zh, en) in LOCAL_AGENT_DIRS {
        if subs.is_empty() {
            t.push(target(local.join(dir), Text::new(*zh, *en), AGENT));
        } else {
            for sub in *subs {
                t.push(target_with_recommendation(
                    local.join(dir).join(sub),
                    Text::new(format!("{zh} · {sub}"), format!("{en} · {sub}")),
                    AGENT,
                    true,
                ));
            }
        }
    }

    // electron-updater 的更新包目录：展开 `%LOCALAPPDATA%` 一层逐个探内容。
    // 不再按应用名列清单——那张 6 个名字的表实测只剩 1 个命中，而真实存在的
    // 4 个一个都没列到。macOS 侧由 `push_user_cache_dirs` 探测
    // `~/Library/Caches`，两边共用同一套签名。
    #[cfg(windows)]
    {
        super::updater::push_updater_dirs_under(t, local);
    }

    // 明确限定到可重建叶子，避免把整个工具状态目录当缓存清掉。
    t.push(target_with_recommendation(
        home.join(".grok/logs"),
        Text::new("Grok · 日志", "Grok · logs"),
        AGENT,
        true,
    ));
    t.push(target_with_recommendation(
        roaming.join("Zed/node/cache"),
        Text::new("Zed · npm 缓存", "Zed · npm cache"),
        AGENT,
        true,
    ));
    t.push(target_with_recommendation(
        roaming.join("Zed/languages"),
        Text::new("Zed · LSP 语言服务器", "Zed · LSP language servers"),
        AGENT,
        false,
    ));
    let zed_node = roaming.join("Zed/node");
    for version in std::fs::read_dir(&zed_node).into_iter().flatten().flatten() {
        if !version.file_name().to_string_lossy().starts_with("node-v")
            || !version.file_type().is_ok_and(|kind| kind.is_dir())
        {
            continue;
        }
        t.push(target_with_recommendation(
            version.path().join("cache"),
            Text::new("Zed · npm 缓存", "Zed · npm cache"),
            AGENT,
            true,
        ));
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

    push_obsolete_vscode_extensions(t, home);
    push_orphaned_editor_workspaces(t, home, roaming);
}

/// 只报告已明确指向"用户主目录下不存在文件夹"的本地工作区。
/// 远程 URI、外接盘和含百分号编码的 URI 都跳过，避免把暂时离线的项目误报。
pub(super) fn push_orphaned_editor_workspaces(
    t: &mut Vec<ScanTarget>,
    home: &Path,
    roaming: &Path,
) {
    for host in VSCODE_HOSTS {
        let root = roaming.join(host).join("User/workspaceStorage");
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let Ok(bytes) = std::fs::read(entry.path().join("workspace.json")) else {
                continue;
            };
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            let Some(uri) = value.get("folder").and_then(|value| value.as_str()) else {
                continue;
            };
            let Some(raw_path) = uri.strip_prefix("file://") else {
                continue;
            };
            if raw_path.contains('%') {
                continue;
            }
            let project = Path::new(raw_path);
            if !project.starts_with(home) || project.exists() {
                continue;
            }
            t.push(target_with_recommendation(
                entry.path(),
                Text::new(
                    format!("孤立工作区 · {host} · {}", project.display()),
                    format!("Orphaned workspace · {host} · {}", project.display()),
                ),
                CategoryId::DevBuild,
                false,
            ));
        }
    }
}

/// VS Code 自己写入 `.obsolete` 的扩展版本已退出当前扩展集合，可以删除。
/// 只信任清单中的单段目录名，并要求目录仍实际存在，避免把 JSON 内容当路径。
pub(super) fn push_obsolete_vscode_extensions(t: &mut Vec<ScanTarget>, home: &Path) {
    let root = home.join(".vscode/extensions");
    let Ok(bytes) = std::fs::read(root.join(".obsolete")) else {
        return;
    };
    let Ok(serde_json::Value::Object(entries)) = serde_json::from_slice(&bytes) else {
        return;
    };
    for (name, obsolete) in entries {
        if obsolete != serde_json::Value::Bool(true)
            || !matches!(
                Path::new(&name).components().collect::<Vec<_>>().as_slice(),
                [std::path::Component::Normal(_)]
            )
        {
            continue;
        }
        let path = root.join(&name);
        if !std::fs::symlink_metadata(&path).is_ok_and(|md| md.is_dir() && !md.is_symlink()) {
            continue;
        }
        t.push(target_with_recommendation(
            path,
            Text::new(
                format!("过期 VS Code 扩展 · {name}"),
                format!("Obsolete VS Code extension · {name}"),
            ),
            CategoryId::DevBuild,
            true,
        ));
    }
}
