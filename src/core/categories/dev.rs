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

/// VS Code 系编辑器的扩展目录根。
///
/// 这一族编辑器（VS Code 及其各家分叉）共用同一套扩展布局：
/// `~/<根>/extensions/` 下每个扩展一个 `<发布者>.<名字>-<版本>` 目录，
/// 同级一个 `.obsolete` JSON 记录已退役的版本。
///
/// 本机实测这六个都存在且都是这个布局（`.vscode` 50 个扩展、`.trae` 33、
/// `.windsurf` 18、`.qoder` 15、`.kiro` 3、`.antigravity` 1）。以前这里
/// 只写死了 `.vscode`，另外五个的 `.obsolete` 完全没人看——`.qoder` 一家
/// 就攒了 39 条记录。不存在的根会被 `read` 失败直接跳过，多列几个的代价
/// 只是一次失败的文件读取。
const VSCODE_FAMILY_EXTENSION_ROOTS: &[(&str, &str)] = &[
    (".vscode", "VS Code"),
    (".vscode-insiders", "VS Code Insiders"),
    (".cursor", "Cursor"),
    (".windsurf", "Windsurf"),
    (".trae", "Trae"),
    (".qoder", "Qoder"),
    (".kiro", "Kiro"),
    (".antigravity", "Antigravity"),
];

/// 编辑器自己写入 `.obsolete` 的扩展版本已退出当前扩展集合，可以删除。
/// 只信任清单中的单段目录名，并要求目录仍实际存在，避免把 JSON 内容当路径。
///
/// **为什么只信 `.obsolete`，不做注册表对账**：Mole 的
/// `0207d72a` 给同一问题加了一套 reconciliation（拿 `extensions.json` 的
/// keep-set 反查没人认领的目录），依据是 `.obsolete` 是「删除日志」而非
/// 「清单」，为空或截断时旧目录无人认领。这个推理成立，但本机六个编辑器
/// 实测下来 **孤儿目录为 0**（目录数与注册数一一对应，`.vscode` 50/50、
/// `.trae` 33/33、`.windsurf` 18/18、`.qoder` 15/15），也就是说这些编辑器
/// 自己收尾是干净的，对账能挖出来的东西是空集。
///
/// 那套对账要引入 keep-set 求并、`package.json` 大小写不敏感比对、编辑器
/// 进程探测、以及一串「拿不准就整类跳过」的兜底——为一个实测收益为零的
/// 场景付这些复杂度不划算。真正会产生孤儿的是「更新到一半被杀掉」这类
/// 异常，等真见到再补，判据留在这里备查。
pub(super) fn push_obsolete_vscode_extensions(t: &mut Vec<ScanTarget>, home: &Path) {
    for (dir, editor) in VSCODE_FAMILY_EXTENSION_ROOTS {
        push_obsolete_extensions_for_root(t, &home.join(dir).join("extensions"), editor);
    }
}

fn push_obsolete_extensions_for_root(t: &mut Vec<ScanTarget>, root: &Path, editor: &str) {
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
                format!("过期 {editor} 扩展 · {name}"),
                format!("Obsolete {editor} extension · {name}"),
            ),
            CategoryId::DevBuild,
            true,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `.obsolete` 里记着、但目录已经不在的条目不能进清理列表——那是
    /// 已经清干净的历史记录，报给用户就是幽灵条目。本机 `.vscode` 的
    /// 120 条记录**全部**属于这一类。
    #[test]
    fn obsolete_entries_without_directories_are_skipped() {
        let tmp = crate::core::testing::fixture("qc_obsolete_ghost");
        let root = tmp.join(".vscode/extensions");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(".obsolete"),
            br#"{"pub.gone-1.0.0":true,"pub.here-2.0.0":true}"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("pub.here-2.0.0")).unwrap();

        let mut t = Vec::new();
        push_obsolete_vscode_extensions(&mut t, &tmp);

        assert_eq!(t.len(), 1, "只有目录还在的那条该进列表");
        assert!(t[0].path.ends_with("pub.here-2.0.0"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 目录名里带路径分隔符的条目必须被拒——`.obsolete` 是 JSON，内容
    /// 不可全信，把它当路径拼接就是目录穿越。
    #[test]
    fn obsolete_entries_with_path_separators_are_rejected() {
        let tmp = crate::core::testing::fixture("qc_obsolete_traversal");
        let root = tmp.join(".vscode/extensions");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".obsolete"), br#"{"../../evil":true}"#).unwrap();
        std::fs::create_dir_all(tmp.join("evil")).unwrap();

        let mut t = Vec::new();
        push_obsolete_vscode_extensions(&mut t, &tmp);
        assert!(t.is_empty(), "带 .. 的条目不该被当成目录名");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 覆盖面回归：VS Code 之外的分叉编辑器也要被扫到。以前这里写死了
    /// `.vscode`，本机另外五个编辑器的 `.obsolete` 完全没人看。
    #[test]
    fn obsolete_scan_covers_vscode_forks_not_just_vscode() {
        let tmp = crate::core::testing::fixture("qc_obsolete_forks");
        let _ = std::fs::remove_dir_all(&tmp);
        for dir in [".cursor", ".windsurf", ".trae", ".qoder"] {
            let root = tmp.join(dir).join("extensions");
            std::fs::create_dir_all(root.join("pub.ext-1.0.0")).unwrap();
            std::fs::write(root.join(".obsolete"), br#"{"pub.ext-1.0.0":true}"#).unwrap();
        }

        let mut t = Vec::new();
        push_obsolete_vscode_extensions(&mut t, &tmp);
        assert_eq!(t.len(), 4, "四个分叉编辑器都该被扫到，实得 {}", t.len());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
