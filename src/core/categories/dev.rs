//! 开发相关：构建产物、AI 编程助手缓存、编辑器工作区

use super::{target, target_with_recommendation, ScanTarget};
use crate::core::categories::CategoryId;
use crate::core::i18n::Text;
use crate::core::rules::Operation;
use std::path::Path;

/// 开发相关清理目标：AI agent 缓存、构建产物、iOS 备份、编辑器工作区
pub(super) fn push_dev_targets(t: &mut Vec<ScanTarget>, home: &Path) {
    if let (Some(cache), Some(data)) = (
        crate::platform::user_cache_dir(),
        crate::platform::user_data_dir(),
    ) {
        // AI 编程助手的会话记录与缓存
        push_ai_agent_targets(t, home, &cache, &data);
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
/// 每个子目录带一个「默认是否勾选」标记，**不靠名字启发式现推**：
/// 同一条 `matches!(sub, "cache" | "log" | ...)` 过去同时决定了「纯临时的
/// `.tmp` 不勾」和「827 MB 的会话记录也不勾」——两件事，一条规则。
/// 判据是规范第二条：删掉只丢「重新生成 / 重新下载」的勾，
/// 会丢会话、历史、编辑快照的一律不勾（仍然展示，用户可手动选）。
///
/// 平台无关：目录名和子目录名在 Windows / macOS 上一致，只有根目录
/// （`%USERPROFILE%` ↔ `~`）在调用方拼接。
/// CLI agent 的一个可清理子目录：`(子目录名, 默认是否勾选)`。
pub(super) type AgentSubdir = (String, bool);
pub(super) type CliAgent = (String, String, Vec<AgentSubdir>);

fn layouts(catalog: &str) -> Vec<crate::core::rules::Layout> {
    crate::core::rules::current()
        .definition("development")
        .catalogs
        .get(catalog)
        .cloned()
        .unwrap_or_default()
}

fn cli_agents() -> Vec<CliAgent> {
    layouts("cli_agents")
        .into_iter()
        .map(|row| {
            (
                row.path,
                row.zh,
                row.children
                    .into_iter()
                    .map(|child| (child.path, child.recommended))
                    .collect(),
            )
        })
        .collect()
}

/// Electron / Chromium 系 AI 编程应用在「 roaming 」根下的目录名。
///
/// **不再决定覆盖面**：一个应用能不能被扫到，靠的是内容签名
/// （`categories::chromium`），不是它叫什么。这张表只决定**归属**——
/// 名字在表里的应用，它的缓存叶子归到「AI 编程助手缓存」；表外的归到
/// 「应用缓存」。所以漏一个名字的代价只是分类不精准，不再是一条缓存
/// 永远清不掉——这正是旧设计最贵的地方。
///
/// Windows 上根是 `%APPDATA%`，macOS 上是 `~/Library/Application Support`。
pub(super) static AI_APP_DIRS: &crate::core::rules::RuleList = &crate::core::rules::RuleList {
    rule: "development",
    key: "ai_app_dirs",
};

/// 这个应用目录名算不算 AI 工具（只影响归类，见 [`AI_APP_DIRS`]）。
fn is_ai_app_dir(name: &str) -> bool {
    AI_APP_DIRS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(name))
}

/// `%LOCALAPPDATA%` / `~/Library/Caches` 下已被 local agent 规则认领的子目录。
///
/// macOS 扫 `~/Library/Caches` 时用它跳过这些孩子，避免和下面的目标双算。
/// electron-updater 的更新包不在这张表里：它们靠内容签名认领。
#[cfg(any(target_os = "macos", test))]
pub(super) fn local_agent_claimed_children(name: &str) -> Vec<String> {
    layouts("local_agents")
        .into_iter()
        .find(|row| row.path == name)
        .map(|row| row.children.into_iter().map(|child| child.path).collect())
        .unwrap_or_default()
}

/// AI 编程助手的缓存、会话残留与临时 worktree。
///
/// 三种来源，覆盖面各不同：
/// - **固定路径**：`CLI_AGENTS` 逐个子目录列出。不存在的会在扫描阶段被
///   `path.exists()` 过滤掉，多列几个候选的代价只是一次 stat。
/// - **内容签名**：Electron/Chromium 系应用（[`push_chromium_app_caches`]）
///   与 agent 目录下自建的浏览器 profile。这里**不看名字**——名字只决定
///   归到 AI 类还是应用缓存。
/// - **动态发现**：旧版本目录、日志库、编辑器清单，都在 `directories` 规则里。
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
    for (dir, label, subs) in &cli_agents() {
        for (sub, recommended) in subs {
            t.push(target_with_recommendation(
                home.join(dir).join(sub),
                format!("{label} · {sub}"),
                AGENT,
                *recommended,
                Operation::Contents,
                ("engine", "development_candidate"),
            ));
        }
    }
    // 日志库、扩展 tasks 等搜索式布局走 `directories` 规则（见 development.toml），
    // 不再在这里重抄一份目录拼接。
    // agent 目录下自建的浏览器 profile：`~/.gemini/antigravity-browser-profile`
    // 是 Chromium 的 userData 形状，同样只收叶子。
    for (dir, label, _) in &cli_agents() {
        push_chromium_leaves(t, &home.join(dir), label, AGENT);
    }

    // ---- Electron / Chromium 系应用：按内容签名认，不按应用名认 ----
    push_chromium_app_caches(t, roaming);

    // ---- local 根下的缓存与更新包 ----
    for row in layouts("local_agents") {
        if row.children.is_empty() {
            t.push(target(
                local.join(&row.path),
                Text::new(row.zh, row.en),
                AGENT,
                Operation::Contents,
                ("engine", "agent_history"),
            ));
        } else {
            for child in &row.children {
                t.push(target_with_recommendation(
                    local.join(&row.path).join(&child.path),
                    Text::new(
                        format!("{} · {}", row.zh, child.path),
                        format!("{} · {}", row.en, child.path),
                    ),
                    AGENT,
                    child.recommended,
                    Operation::Contents,
                    ("engine", "development_candidate"),
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

    #[cfg(test)]
    crate::core::rules::versions::append_fixture(t, "zed_node", home, roaming);

    // ---- AI agent 的临时 git worktree（单列一类，风险更高）----
    for row in layouts("worktrees") {
        for name in ["worktrees", ".worktrees"] {
            push_worktrees(t, &home.join(&row.path).join(name), &row.zh, &row.en);
        }
    }
    for row in layouts("roaming_worktrees") {
        push_worktrees(t, &roaming.join(&row.path), &row.zh, &row.en);
    }

    // 编辑器扩展清单（`.obsolete`）与孤立工作区（`workspace.json`）也由
    // `directories` 规则读取（见 development.toml），不在这里再抄一份 JSON 解析。
}

fn push_worktrees(t: &mut Vec<ScanTarget>, container: &Path, zh: &str, en: &str) {
    for path in crate::core::worktrees::discover(container) {
        if crate::core::safety::is_protected(&path) {
            continue;
        }
        let Ok(worktree) = crate::core::worktrees::inspect(&path) else {
            continue;
        };
        let registration = worktree.admin;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let label = Text::new(format!("{zh} · {name}"), format!("{en} · {name}"));
        t.push(target(
            path,
            label,
            CategoryId::DevWorktrees,
            Operation::GitWorktree { registration },
            ("engine", "linked_worktree"),
        ));
    }
}

/// 应用目录（`~/Library/Application Support` / `%APPDATA%`）下所有 Chromium
/// 系应用的缓存叶子。
///
/// 这一条取代了按应用名列名单的老做法：覆盖面靠内容签名，名字只决定归类
/// （AI 工具进 `AiAgents`，其余进 `UserCache`）。因此漏一个名字的代价从
/// 「一堆缓存永远清不掉」降成「分类标签不够准」。
///
/// 浏览器不在这里：它们用的是同一个 profile 布局，已经由 `browser.rs` 逐
/// 浏览器认领（包括 `Crashpad/completed` 这类特殊处置），重复入表就是双算。
fn push_chromium_app_caches(t: &mut Vec<ScanTarget>, roaming: &Path) {
    let Ok(entries) = std::fs::read_dir(roaming) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if super::browser::owns_app_support_dir(&name) {
            continue;
        }
        if super::browser::contains_claimed_browser_child(&name) {
            if let Ok(children) = std::fs::read_dir(entry.path()) {
                for child in children.flatten() {
                    let child_name = child.file_name().to_string_lossy().into_owned();
                    if child.file_type().is_ok_and(|kind| kind.is_dir())
                        && !super::browser::claimed_browser_child(&name, &child_name)
                    {
                        let category = if is_ai_app_dir(&child_name) {
                            CategoryId::AiAgents
                        } else {
                            CategoryId::UserCache
                        };
                        push_chromium_leaves(t, &child.path(), &child_name, category);
                    }
                }
            }
            continue;
        }
        let category = if is_ai_app_dir(&name) {
            CategoryId::AiAgents
        } else {
            CategoryId::UserCache
        };
        push_chromium_leaves(t, &entry.path(), &name, category);
    }
}

/// 把一个目录（应用目录 / agent 目录 / 缓存根）下的 Chromium 缓存叶子入表。
///
/// 只列叶子，不列承载它的目录——理由见 `categories::chromium` 头注释。
/// 标签把父目录名放在所有者位置：用户看到的是「Codex · GPUCache」，
/// 而不是一个不透明的绝对路径。
fn push_chromium_leaves(t: &mut Vec<ScanTarget>, dir: &Path, owner: &str, category: CategoryId) {
    for leaf in super::chromium::cache_leaves(dir) {
        // 叶子在 profile 里时把 profile 名也写进标签：`Codex · GPUCache` 与
        // `Codex · Default · GPUCache` 是两条不同的路径，只写叶子名在界面上
        // 就是两行同名条目。
        let trail = super::chromium::leaf_trail(dir, &leaf);
        let Some(leaf_name) = trail.last() else {
            continue;
        };
        t.push(target_with_recommendation(
            leaf,
            format!("{owner} · {}", trail.join(" · ")),
            category,
            super::chromium::leaf_recommended(leaf_name),
            Operation::Contents,
            ("engine", "development_candidate"),
        ));
    }
}

#[cfg(all(test, unix))]
fn push_codex_cli_versions(t: &mut Vec<ScanTarget>, home: &Path) {
    crate::core::rules::versions::append_fixture(t, "codex_cli", home, home);
}
#[cfg(all(test, unix))]
fn push_devin_cli_versions(t: &mut Vec<ScanTarget>, home: &Path, roaming: &Path) {
    crate::core::rules::versions::append_fixture(t, "devin_unix", home, roaming);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 走生产选择器（`directories` 规则）而不是测试专用的拼接，夹具与运行时同一条路径。
    fn rule_directories(home: &Path, roaming: &Path) -> Vec<ScanTarget> {
        let snapshot = crate::core::rules::snapshot();
        let roots = crate::core::rules::directories::fixture_roots(home, roaming);
        crate::core::rules::directories::scan_at(&snapshot, "development", &roots).0
    }

    /// 编辑器清单产出的目标：Tree + `DevBuild`。
    fn manifest_targets(home: &Path, roaming: &Path) -> Vec<std::path::PathBuf> {
        rule_directories(home, roaming)
            .into_iter()
            .filter(|target| {
                target.category == CategoryId::DevBuild && target.operation == Operation::Tree
            })
            .map(|target| target.path)
            .collect()
    }

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

        let targets = manifest_targets(&tmp, &tmp.join("roaming"));

        assert_eq!(targets.len(), 1, "{targets:?}");
        assert!(targets[0].ends_with("pub.here-2.0.0"));
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

        assert!(
            manifest_targets(&tmp, &tmp.join("roaming")).is_empty(),
            "带 .. 的条目不该被当成目录名"
        );
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

        let targets = manifest_targets(&tmp, &tmp.join("roaming"));
        assert_eq!(targets.len(), 4, "四个分叉编辑器都该被扫到：{targets:?}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 只报「清单里写着本地路径、而那个路径确实不在」的工作区目录。
    /// 远程 URI、带百分号编码的 URI 和仍然存在的项目都不入表。
    #[test]
    fn orphaned_workspaces_follow_recorded_local_paths_only() {
        let tmp = crate::core::testing::fixture("qc_orphaned_workspaces");
        let home = tmp.join("home");
        let storage = tmp.join("roaming/Code/User/workspaceStorage");
        let _ = std::fs::remove_dir_all(&tmp);
        let recorded = |hash: &str, uri: &str| {
            let dir = storage.join(hash);
            std::fs::create_dir_all(&dir).unwrap();
            // 用 serde_json 写：Windows 路径里的反斜杠必须转义，否则清单根本解不开。
            std::fs::write(
                dir.join("workspace.json"),
                serde_json::json!({"folder": uri}).to_string(),
            )
            .unwrap();
        };
        let gone = home.join("gone-project");
        let alive = home.join("alive-project");
        std::fs::create_dir_all(&alive).unwrap();
        recorded("orphan", &format!("file://{}", gone.display()));
        recorded("alive", &format!("file://{}", alive.display()));
        recorded("remote", "vscode-remote://ssh-remote+host/home/me/project");
        recorded(
            "encoded",
            &format!("file://{}/my%20project", home.display()),
        );

        let targets = manifest_targets(&home, &tmp.join("roaming"));

        assert_eq!(targets.len(), 1, "{targets:?}");
        assert_eq!(targets[0], storage.join("orphan"));
        assert!(!gone.exists() && alive.is_dir());
        let _ = std::fs::remove_dir_all(&tmp);
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
        for (dir, label, subs) in &cli_agents() {
            for (sub, _) in subs {
                assert!(
                    !NEVER_CLEAN.contains(&sub.as_str()),
                    "{label}（{dir}）把 {sub} 列成了可清理项，这会破坏用户配置"
                );
            }
        }
        for row in layouts("local_agents") {
            for child in &row.children {
                assert!(
                    !NEVER_CLEAN.contains(&child.path.as_str()),
                    "{}（{}）把 {} 列成了可清理项",
                    row.zh,
                    row.path,
                    child.path
                );
            }
        }
    }

    /// 纯临时目录要预选，会话/历史不预选。
    ///
    /// 这条过去由一个字符串启发式（`sub == "cache" || sub == "log" || ...`）
    /// 同时决定，`.tmp` 因此被漏在外面（本机 121 MB）；现在改成表里显式写，
    /// 改错了测试就红。
    #[test]
    fn temp_subdirs_are_recommended_and_history_is_not() {
        let recommended = |dir: &str, sub: &str| {
            cli_agents()
                .iter()
                .find(|(name, _, _)| *name == dir)
                .and_then(|(_, _, subs)| subs.iter().find(|(name, _)| *name == sub))
                .map(|(_, recommended)| *recommended)
                .unwrap_or_else(|| panic!("{dir}/{sub} 不在表里"))
        };
        for (dir, sub) in [
            (".codex", "tmp"),
            (".codex", ".tmp"),
            (".codex", "cache"),
            (".claude", "paste-cache"),
            (".claude", "shell-snapshots"),
            (".gemini", "tmp"),
            (".augment", "observability"),
            (".workbuddy", "logs"),
        ] {
            assert!(recommended(dir, sub), "{dir}/{sub} 是纯临时，该预选");
        }
        for (dir, sub) in [
            (".codex", "sessions"),
            (".codex", "computer-use"),
            (".claude", "projects"),
            (".claude", "file-history"),
            (".augment", "checkpoint-documents"),
            (".workbuddy", "sessions"),
        ] {
            assert!(!recommended(dir, sub), "{dir}/{sub} 是历史/会话，不该预选");
        }
    }

    /// `.grok/logs` 和 Zed 的固定叶子走规则条目，不在扫描函数里再写一份。
    #[test]
    fn fixed_agent_leaves_stay_in_the_development_rule() {
        let snapshot = crate::core::rules::current();
        let entries = &snapshot.definition("development").entries;
        let grok = entries
            .iter()
            .find(|entry| entry.path == ".grok/logs")
            .expect(".grok/logs");
        assert_eq!(grok.root, "home");
        assert_eq!(grok.category, "AiAgents");
        assert!(grok.recommended);
        let cache = entries
            .iter()
            .find(|entry| entry.path == "Zed/node/cache")
            .expect("Zed/node/cache");
        assert_eq!(cache.root, "roaming");
        assert!(cache.recommended);
        let languages = entries
            .iter()
            .find(|entry| entry.path == "Zed/languages")
            .expect("Zed/languages");
        assert!(!languages.recommended);
    }

    /// 本地缓存、插件和 worktree 从常量迁到规则后，原来钉死的名字还要在。
    #[test]
    fn moved_catalogs_keep_the_previous_names() {
        let paths = |catalog: &str| -> Vec<String> {
            layouts(catalog).into_iter().map(|row| row.path).collect()
        };
        let local = paths("local_agents");
        for name in ["claude-cli-nodejs", "amp", "Zed", "WorkBuddy"] {
            assert!(local.iter().any(|path| path == name), "{name} missing");
        }
        let amp = layouts("local_agents")
            .into_iter()
            .find(|row| row.path == "amp")
            .expect("amp");
        assert_eq!(
            amp.children
                .iter()
                .map(|child| child.path.as_str())
                .collect::<Vec<_>>(),
            vec!["logs", "traces"]
        );
        let extensions = paths("vscode_extensions");
        for name in [
            "saoudrizwan.claude-dev",
            "rooveterinaryinc.roo-cline",
            "kilocode.kilo-code",
            "github.copilot-chat",
        ] {
            assert!(extensions.iter().any(|path| path == name), "{name} missing");
        }
        let trees = paths("worktrees");
        assert!(paths("roaming_worktrees")
            .iter()
            .any(|path| path == "Maka/workspaces"));
        for name in [
            ".codex",
            ".windsurf",
            ".claude",
            ".cursor",
            ".trae",
            ".augment",
            ".workbuddy",
            ".gemini",
        ] {
            assert!(trees.iter().any(|path| path == name), "{name} missing");
        }
        let family = paths("vscode_family");
        for name in [
            ".vscode",
            ".vscode-insiders",
            ".cursor",
            ".windsurf",
            ".trae",
            ".qoder",
            ".kiro",
            ".antigravity",
        ] {
            assert!(family.iter().any(|path| path == name), "{name} missing");
        }
    }

    #[test]
    fn owned_agent_session_worktrees_are_filtered_by_core_safety() {
        let root = crate::core::testing::fixture("agent_session_discovery_guard");
        let workspace = root.join("workspace");
        let container = workspace.join("subagent-worktrees");
        let checkout = container.join("child");
        let common = root.join("repo/.git");
        let admin = common.join("worktrees/child");
        std::fs::create_dir_all(&checkout).unwrap();
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::write(common.join("HEAD"), b"ref: refs/heads/main\n").unwrap();
        std::fs::write(admin.join("HEAD"), b"fixture\n").unwrap();
        std::fs::write(admin.join("commondir"), b"../..\n").unwrap();
        std::fs::write(
            admin.join("gitdir"),
            checkout.join(".git").to_string_lossy().as_bytes(),
        )
        .unwrap();
        std::fs::write(
            checkout.join(".git"),
            format!("gitdir: {}\n", admin.display()),
        )
        .unwrap();
        std::fs::write(checkout.join("sentinel"), b"session source").unwrap();
        let mut targets = Vec::new();
        push_worktrees(&mut targets, &container, "fixture", "fixture");
        assert_eq!(
            targets.len(),
            1,
            "fixture must be a discoverable linked checkout"
        );
        std::fs::write(workspace.join("runtime.sqlite"), b"owner metadata").unwrap();
        targets.clear();
        assert!(crate::core::safety::is_protected(&checkout));
        push_worktrees(&mut targets, &container, "fixture", "fixture");
        assert!(
            targets.is_empty(),
            "protected session checkout must not be offered for cleanup"
        );
        assert!(checkout.join("sentinel").is_file());
        assert!(admin.join("HEAD").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ai_agent_recommendations_are_decided_per_target() {
        let root = crate::core::testing::fixture("qc_agent_rules");
        let home = root.join("home");
        let local = root.join("local");
        let roaming = root.join("roaming");
        // Chromium 系的缓存叶子靠内容签名发现，夹具必须把签名建出来。
        for leaf in [
            "Cache",
            "Code Cache",
            "GPUCache",
            "CachedProfilesData",
            "blob_storage",
        ] {
            std::fs::create_dir_all(roaming.join("Cursor").join(leaf)).unwrap();
            std::fs::write(roaming.join("Cursor").join(leaf).join("data"), b"x").unwrap();
        }
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
        let _ = std::fs::remove_dir_all(root);
    }

    /// 覆盖面不再依赖应用名：签名在，表外的应用也能被扫到；名字只影响归类。
    #[test]
    fn chromium_shape_wins_over_the_app_name_list() {
        let root = crate::core::testing::fixture("qc_ai_shape");
        let home = root.join("home");
        let local = root.join("local");
        let roaming = root.join("roaming");
        for app in ["Codex", "SomeToolNobodyListed"] {
            for leaf in ["Cache", "Code Cache", "GPUCache"] {
                let dir = roaming.join(app).join(leaf);
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join("data"), b"x").unwrap();
            }
        }
        let mut targets = Vec::new();
        push_ai_agent_targets(&mut targets, &home, &local, &roaming);

        let category = |path: std::path::PathBuf| {
            targets
                .iter()
                .find(|target| target.path == path)
                .map(|target| target.category)
        };
        let cache = |app: &str| roaming.join(app).join("Cache");
        assert_eq!(category(cache("Codex")), Some(CategoryId::AiAgents));
        // 名字不在表里也照样进表，只是归到「应用缓存」。
        assert_eq!(
            category(cache("SomeToolNobodyListed")),
            Some(CategoryId::UserCache)
        );
        // 承载缓存的目录本身不能入表
        assert!(!targets.iter().any(|t| t.path == roaming.join("Codex")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn browser_vendor_siblings_are_scanned_without_claimed_browser() {
        let root = crate::core::testing::fixture("qc_browser_vendor_siblings");
        let roaming = root.join("Application Support");
        for app in ["Google/Chrome", "Google/OtherApp"] {
            for leaf in ["Cache", "GPUCache"] {
                let dir = roaming.join(app).join(leaf);
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join("data"), b"x").unwrap();
            }
        }
        let mut targets = Vec::new();
        push_chromium_app_caches(&mut targets, &roaming);
        assert!(targets
            .iter()
            .any(|target| target.path == roaming.join("Google/OtherApp/Cache")));
        assert!(!targets
            .iter()
            .any(|target| target.path.starts_with(roaming.join("Google/Chrome"))));
        let _ = std::fs::remove_dir_all(root);
    }

    /// `~/.codex/packages/standalone/releases/<版本>/`：旧版本列出并预选，
    /// `current` 指向的那份、`install.lock`、`auto-update-version` 都不列。
    #[test]
    #[cfg(unix)]
    fn codex_old_versions_are_listed_and_current_is_kept() {
        use super::push_codex_cli_versions;

        let root = crate::core::testing::fixture("qc_codex_versions");
        let standalone = root.join(".codex/packages/standalone");
        for version in [
            "0.147.0-aarch64-apple-darwin",
            "0.155.1-aarch64-apple-darwin",
            "0.157.1-aarch64-apple-darwin",
        ] {
            std::fs::create_dir_all(standalone.join("releases").join(version).join("bin")).unwrap();
        }
        std::fs::write(standalone.join("auto-update-version"), b"0.157.1").unwrap();
        std::fs::write(standalone.join("install.lock"), b"").unwrap();
        std::os::unix::fs::symlink(
            "releases/0.157.1-aarch64-apple-darwin",
            standalone.join("current"),
        )
        .unwrap();
        // 锁文件是刚写的 → 视为正在换版，一律不预选
        let mut targets = Vec::new();
        push_codex_cli_versions(&mut targets, &root);
        assert_eq!(targets.len(), 2, "{targets:?}");
        assert!(
            targets.iter().all(|t| !t.recommended),
            "更新中的锁文件应压掉预选"
        );
        assert!(targets.iter().all(|t| t.category == CategoryId::AiAgents));
        let mut paths: Vec<String> = targets
            .iter()
            .map(|t| t.path.display().to_string())
            .collect();
        paths.sort();
        assert!(paths[0].contains("0.147.0"), "{paths:?}");
        assert!(paths[1].contains("0.155.1"), "{paths:?}");
        assert!(
            !paths
                .iter()
                .any(|p| p.contains("0.157.1") || p.contains("current")),
            "当前版本与软链接不能入表: {paths:?}"
        );

        // 锁文件很旧（上次更新异常退出）→ 旧版本可以预选
        std::fs::remove_file(standalone.join("install.lock")).unwrap();
        let mut targets = Vec::new();
        push_codex_cli_versions(&mut targets, &root);
        assert_eq!(targets.len(), 2);
        assert!(
            targets.iter().all(|t| t.recommended),
            "current 在且无更新时该预选"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// 日志库（`logs_2.sqlite`）要进表但不预选；会话历史与状态库绝不入表。
    #[test]
    fn agent_log_databases_are_listed_but_never_recommended() {
        let root = crate::core::testing::fixture("qc_agent_logs_db");
        let home = root.join("home");
        for agent in [".codex", ".claude"] {
            std::fs::create_dir_all(home.join(agent)).unwrap();
            std::fs::write(home.join(agent).join("logs_2.sqlite"), b"db").unwrap();
        }
        let codex = home.join(".codex");
        for name in [
            // 事务侧文件不是日志库本身，后缀筛选把它挡在外面。
            "logs_2.sqlite-wal",
            "thread_history_1.sqlite",
            "state_5.sqlite",
            "memories_1.sqlite",
        ] {
            std::fs::write(codex.join(name), b"db").unwrap();
        }
        let targets = rule_directories(&home, &root.join("roaming"));
        let log_databases: Vec<_> = targets
            .iter()
            .filter(|target| target.operation == Operation::File)
            .collect();

        assert_eq!(log_databases.len(), 2, "{targets:?}");
        assert!(log_databases
            .iter()
            .any(|target| target.path == codex.join("logs_2.sqlite")));
        assert!(log_databases.iter().all(|target| !target.recommended
            && target.category == CategoryId::AiAgents
            && target.disposal == crate::core::cleaner::Disposal::Permanent));
        assert!(
            log_databases.iter().any(|target| target
                .label
                .get(crate::core::i18n::Language::Zh)
                .contains("Codex · 日志库 logs_2.sqlite")),
            "日志库标签要带所有者：{:?}",
            log_databases
                .iter()
                .map(|target| target.label.get(crate::core::i18n::Language::Zh))
                .collect::<Vec<_>>()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// 开发规则四条目录布局的隔离夹具：日志库、扩展 tasks、扩展清单与孤立工作区。
    fn development_layout_fixture() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf)
    {
        let root = crate::core::testing::fixture("development_layout_baseline");
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        let roaming = root.join("roaming");
        for agent in [".codex", ".claude"] {
            std::fs::create_dir_all(home.join(agent)).unwrap();
            std::fs::write(home.join(agent).join("logs_2.sqlite"), b"db").unwrap();
        }
        std::fs::write(home.join(".codex/thread_history_1.sqlite"), b"db").unwrap();
        std::fs::write(home.join(".codex/logs_2.sqlite-wal"), b"wal").unwrap();
        let tasks = |host: &str, extension: &str| {
            roaming
                .join(host)
                .join("User/globalStorage")
                .join(extension)
                .join("tasks")
        };
        std::fs::create_dir_all(tasks("Code", "saoudrizwan.claude-dev")).unwrap();
        std::fs::create_dir_all(tasks("Code", "example.undeclared")).unwrap();
        std::fs::create_dir_all(tasks("Trae", "github.copilot-chat")).unwrap();
        // 扩展清单：一条目录还在、一条只剩记录、一条未被选中、一条带路径分隔符。
        let extensions = home.join(".vscode/extensions");
        std::fs::create_dir_all(extensions.join("pub.retired-1.0.0")).unwrap();
        std::fs::write(
            extensions.join(".obsolete"),
            br#"{"pub.retired-1.0.0":true,"pub.ghost-0.9.0":true,"pub.kept-2.0.0":false,"../escape":true}"#,
        )
        .unwrap();
        // 孤立工作区：只认清单里写着 home 内本地路径、而该路径确实不存在的那条。
        let alive = home.join("alive-project");
        std::fs::create_dir_all(&alive).unwrap();
        for (hash, uri) in [
            (
                "orphan",
                format!("file://{}", home.join("gone-project").display()),
            ),
            ("alive", format!("file://{}", alive.display())),
            (
                "remote",
                "vscode-remote://ssh-remote+host/home/me/project".to_string(),
            ),
            ("encoded", format!("file://{}/my%20project", home.display())),
        ] {
            let dir = roaming.join("Code/User/workspaceStorage").join(hash);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("workspace.json"),
                serde_json::json!({"folder": uri}).to_string(),
            )
            .unwrap();
        }
        (root, home, roaming)
    }

    fn layout_rows(targets: &[ScanTarget], root: &Path) -> Vec<serde_json::Value> {
        // 记录清单里写着绝对项目路径，基线里用 <root> 归一，两边平台才可比。
        let prefix = root.display().to_string();
        let label = |target: &ScanTarget, language| {
            target
                .label
                .get(language)
                .replace(&prefix, "<root>")
                .replace('\\', "/")
        };
        let mut rows: Vec<_> = targets
            .iter()
            .map(|target| {
                serde_json::json!({
                    "path": target.path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"),
                    "category": format!("{:?}", target.category),
                    "operation": target.operation,
                    "disposal": target.disposal,
                    "recommended": target.recommended,
                    "zh": label(target, crate::core::i18n::Language::Zh),
                    "en": label(target, crate::core::i18n::Language::En),
                })
            })
            .collect();
        rows.sort_by_key(|row| row["path"].as_str().unwrap().to_owned());
        rows
    }

    /// 迁移金样：目标、推荐、操作、处置和双语标签与记录基线逐项一致。
    #[test]
    fn development_directory_layouts_preserve_baseline_and_share_probes() {
        let (root, home, roaming) = development_layout_fixture();
        let snapshot = crate::core::rules::snapshot();
        let roots = crate::core::rules::directories::fixture_roots(&home, &roaming);
        let (targets, reads) =
            crate::core::rules::directories::scan_at(&snapshot, "development", &roots);
        let expected: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../rules/fixtures/development-layout-baseline.json"
        ))
        .unwrap();
        assert_eq!(layout_rows(&targets, &root), expected);
        assert!(targets.iter().all(|target| std::sync::Arc::ptr_eq(
            &target.rule.snapshot,
            &snapshot
        ) && target.rule.id == "development"));
        assert_eq!(
            reads, 3,
            "两条 agent 目录 + 一个 workspaceStorage 库存各读一次；声明候选只探一次"
        );
        // 未声明的扩展 id、幽灵清单条目与事务侧文件都不因为目录存在就进表。
        assert!(!targets
            .iter()
            .any(|target| target.path.ends_with("example.undeclared/tasks")));
        assert!(!targets
            .iter()
            .any(|target| target.path.ends_with("pub.ghost-0.9.0")));
        assert!(!targets
            .iter()
            .any(|target| target.path.ends_with("workspaceStorage/alive")));
        assert!(home.join(".codex/logs_2.sqlite-wal").exists());
        let (again, _) = crate::core::rules::directories::scan_at(&snapshot, "development", &roots);
        assert_eq!(layout_rows(&again, &root), expected, "同一夹具扫描可复现");
        let _ = std::fs::remove_dir_all(root);
    }

    /// 扩展 tasks 的主机 × 扩展交叉展开来自规则，两个名单都不在 Rust 里。
    #[test]
    fn vscode_task_storage_follows_declared_hosts_and_extensions() {
        let root = crate::core::testing::fixture("qc_vscode_tasks");
        let home = root.join("home");
        let roaming = root.join("roaming");
        let global_storage = |host: &str, extension: &str| {
            roaming
                .join(host)
                .join("User/globalStorage")
                .join(extension)
                .join("tasks")
        };
        for (host, extension) in [
            ("Code", "saoudrizwan.claude-dev"),
            ("Code", "example.undeclared"),
            ("Trae", "kilocode.kilo-code"),
            ("Unlisted Editor", "saoudrizwan.claude-dev"),
        ] {
            std::fs::create_dir_all(global_storage(host, extension)).unwrap();
        }
        let targets = rule_directories(&home, &roaming);
        let storage = |host: &str, extension: &str| {
            targets
                .iter()
                .find(|target| target.path == global_storage(host, extension))
        };

        let cline = storage("Code", "saoudrizwan.claude-dev").expect("declared host and extension");
        assert_eq!(cline.operation, Operation::Contents);
        assert_eq!(cline.category, CategoryId::AiAgents);
        assert!(!cline.recommended, "会话缓存不预选");
        assert_eq!(
            cline.label.get(crate::core::i18n::Language::Zh),
            "Code · Cline 会话缓存"
        );
        assert_eq!(
            cline.label.get(crate::core::i18n::Language::En),
            "Code · Cline sessions"
        );
        assert!(storage("Trae", "kilocode.kilo-code").is_some());
        assert!(
            storage("Code", "example.undeclared").is_none(),
            "未声明的扩展 id 不能进表"
        );
        assert!(
            storage("Unlisted Editor", "saoudrizwan.claude-dev").is_none(),
            "未声明的主机不能进表"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn only_vscode_declared_obsolete_extensions_are_recommended() {
        let root = crate::core::testing::fixture("qc_obsolete_ext");
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

        let targets: Vec<_> = rule_directories(&root, &root.join("roaming"))
            .into_iter()
            .filter(|target| target.category == CategoryId::DevBuild)
            .collect();

        assert_eq!(targets.len(), 1, "{targets:?}");
        assert_eq!(targets[0].path, old);
        assert!(targets[0].recommended);
        assert_eq!(
            targets[0].label.get(crate::core::i18n::Language::Zh),
            "过期 VS Code 扩展 · example.tool-1.0.0"
        );
        assert!(current.is_dir());
        let _ = std::fs::remove_dir_all(root);
    }

    /// Devin CLI 旧版本目录与安装包应被列入，当前版本与 current 软链接不能列入。
    /// 软链接创建是 Unix 专有，Windows 上跳过。
    #[test]
    #[cfg(unix)]
    fn devin_cli_old_versions_and_installers_are_listed() {
        use super::push_devin_cli_versions;
        use crate::core::categories::CategoryId;

        let root = crate::core::testing::fixture("qc_devin_versions");
        let _ = std::fs::remove_dir_all(&root);
        let cli_root = root.join(".local/share/devin/cli");
        let versions = cli_root.join("_versions");
        std::fs::create_dir_all(versions.join("3000.6.14/bin")).unwrap();
        std::fs::create_dir_all(versions.join("3000.6.11/bin")).unwrap();
        std::fs::create_dir_all(versions.join("3000.6.7/bin")).unwrap();
        std::fs::create_dir_all(versions.join("_download")).unwrap();
        std::fs::write(versions.join("_download/3000.6.14.tar.gz"), b"pkg").unwrap();
        std::fs::write(versions.join("_download/3000.6.11.tar.gz"), b"pkg").unwrap();
        std::fs::write(versions.join("_download/3000.6.7.tar.gz"), b"pkg").unwrap();
        // current 软链接指向当前版本
        std::os::unix::fs::symlink("3000.6.14", versions.join("current")).unwrap();

        let mut targets = Vec::new();
        push_devin_cli_versions(&mut targets, &root, &root.join("roaming"));

        // 旧版本目录：3000.6.11 和 3000.6.7，当前版本 3000.6.14 不应出现
        let old_dirs: Vec<_> = targets
            .iter()
            .filter(|t| t.category == CategoryId::AiAgents)
            .filter(|t| t.path.is_dir())
            .collect();
        assert_eq!(
            old_dirs.len(),
            2,
            "应有 2 个旧版本目录，实得 {}",
            old_dirs.len()
        );
        assert!(old_dirs.iter().any(|t| t.path.ends_with("3000.6.11")));
        assert!(old_dirs.iter().any(|t| t.path.ends_with("3000.6.7")));
        assert!(!old_dirs.iter().any(|t| t.path.ends_with("3000.6.14")));
        assert!(
            old_dirs.iter().all(|t| t.recommended),
            "旧版本目录应默认勾选"
        );

        // 安装包：3 个 tar.gz
        let installers: Vec<_> = targets
            .iter()
            .filter(|t| t.path.extension().is_some_and(|e| e == "gz"))
            .collect();
        assert_eq!(
            installers.len(),
            3,
            "应有 3 个安装包，实得 {}",
            installers.len()
        );
        assert!(installers.iter().all(|t| t.recommended), "安装包应默认勾选");

        // current 软链接和 _download 目录不应作为旧版本目录出现
        assert!(!targets.iter().any(|t| t.path.ends_with("current")));
        assert!(!targets.iter().any(|t| t.path.ends_with("_download")));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// `_update.lock` 存在且较新时（可能正在更新），旧版本和安装包不预选。
    #[test]
    #[cfg(unix)]
    fn devin_cli_updating_lock_suppresses_recommendation() {
        use super::push_devin_cli_versions;

        let root = crate::core::testing::fixture("qc_devin_lock");
        let _ = std::fs::remove_dir_all(&root);
        let cli_root = root.join(".local/share/devin/cli");
        let versions = cli_root.join("_versions");
        std::fs::create_dir_all(versions.join("3000.6.14/bin")).unwrap();
        std::fs::create_dir_all(versions.join("3000.6.11/bin")).unwrap();
        std::os::unix::fs::symlink("3000.6.14", versions.join("current")).unwrap();
        // 创建一个新鲜的锁文件（0 字节，刚刚修改）
        std::fs::write(cli_root.join("_update.lock"), b"").unwrap();

        let mut targets = Vec::new();
        push_devin_cli_versions(&mut targets, &root, &root.join("roaming"));

        // 锁文件刚创建，所有目标不应预选
        assert!(targets.iter().all(|t| !t.recommended), "正在更新时不应预选");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// `current` 软链接缺失时（安装损坏），仍然列出所有版本目录但不预选——
    /// 分不清哪个是当前版本，不能默认删。
    #[test]
    #[cfg(unix)]
    fn devin_cli_missing_current_link_no_recommendation() {
        use super::push_devin_cli_versions;
        use crate::core::categories::CategoryId;

        let root = crate::core::testing::fixture("qc_devin_no_current");
        let _ = std::fs::remove_dir_all(&root);
        let versions = root.join(".local/share/devin/cli/_versions");
        std::fs::create_dir_all(versions.join("3000.6.14/bin")).unwrap();
        std::fs::create_dir_all(versions.join("3000.6.11/bin")).unwrap();
        // 不创建 current 软链接

        let mut targets = Vec::new();
        push_devin_cli_versions(&mut targets, &root, &root.join("roaming"));

        let old_dirs: Vec<_> = targets
            .iter()
            .filter(|t| t.category == CategoryId::AiAgents && t.path.is_dir())
            .collect();
        // 两个版本目录都列出（分不清当前版本），但都不预选
        assert_eq!(old_dirs.len(), 2);
        assert!(
            old_dirs.iter().all(|t| !t.recommended),
            "current 缺失时不应预选"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
