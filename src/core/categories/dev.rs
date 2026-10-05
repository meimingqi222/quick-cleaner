//! 开发相关：构建产物、AI 编程助手缓存、编辑器工作区

use super::{target, target_with_recommendation, ScanTarget};
use crate::core::categories::CategoryId;
use crate::core::i18n::Text;
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

/// VS Code 系编辑器里 AI 插件的全局存储（会话缓存都存这儿）。
/// 平台无关：`User/globalStorage/<ext-id>/tasks` 的相对结构两边一致。
pub(super) static VSCODE_HOSTS: &crate::core::rules::RuleList = &crate::core::rules::RuleList {
    rule: "development",
    key: "vscode_hosts",
};

/// AI 编程助手的缓存、会话残留与临时 worktree。
///
/// 三种来源，覆盖面各不同：
/// - **固定路径**：`CLI_AGENTS` 逐个子目录列出。不存在的会在扫描阶段被
///   `path.exists()` 过滤掉，多列几个候选的代价只是一次 stat。
/// - **内容签名**：Electron/Chromium 系应用（[`push_chromium_app_caches`]）
///   与 agent 目录下自建的浏览器 profile。这里**不看名字**——名字只决定
///   归到 AI 类还是应用缓存。
/// - **动态发现**：旧版本目录、日志库。
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
            ));
        }
    }
    push_agent_log_databases(t, home);
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

    // 固定叶子在 development 规则的 entries 里。这里只补版本化的
    // `node-v*` 缓存：版本号要读目录才知道，而且只能收 `cache`，
    // 不能把整个工具状态目录当缓存清掉。
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
    let extensions = layouts("vscode_extensions");
    for host in VSCODE_HOSTS {
        for row in &extensions {
            t.push(target(
                roaming
                    .join(&host)
                    .join("User")
                    .join("globalStorage")
                    .join(&row.path)
                    .join("tasks"),
                Text::new(
                    format!("{host} · {}", row.zh),
                    format!("{host} · {}", row.en),
                ),
                AGENT,
            ));
        }
    }

    // ---- AI agent 的临时 git worktree（单列一类，风险更高）----
    for row in layouts("worktrees") {
        for name in ["worktrees", ".worktrees"] {
            push_worktrees(t, &home.join(&row.path).join(name), &row.zh, &row.en);
        }
    }
    for row in layouts("roaming_worktrees") {
        push_worktrees(t, &roaming.join(&row.path), &row.zh, &row.en);
    }

    push_devin_cli_versions(t, home, roaming);
    push_codex_cli_versions(t, home);
    push_obsolete_vscode_extensions(t, home);
    push_orphaned_editor_workspaces(t, home, roaming);
}

fn push_worktrees(t: &mut Vec<ScanTarget>, container: &Path, zh: &str, en: &str) {
    for path in crate::core::worktrees::discover(container) {
        if crate::core::safety::is_protected(&path) {
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let label = Text::new(format!("{zh} · {name}"), format!("{en} · {name}"));
        t.push(target(path, label, CategoryId::DevWorktrees));
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
        ));
    }
}

/// agent 根目录下的 `logs*.sqlite` 日志库。
///
/// 本机 `~/.codex/logs_2.sqlite` 单个 279 MB，`.tables` 里只有一张 `logs`
/// 表——而 `~/.codex/log` 这个**目录**早就在表里了：同一个东西的两种形态，
/// 只收目录就漏掉了大头。
///
/// 刻意不预选：日志库开着 WAL/SHM（agent 运行时恒成立），删除级闸门
/// （`safety::is_active_sqlite_member`）会直接拒删，预选只会制造一次必然
/// 失败。用户关掉 agent 再手动勾，才删得掉。
fn push_agent_log_databases(t: &mut Vec<ScanTarget>, home: &Path) {
    for (dir, label, _) in &cli_agents() {
        let Ok(entries) = std::fs::read_dir(home.join(dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // `logs.sqlite` / `logs_2.sqlite`。后缀把 `logs_2.sqlite-wal` 挡在
            // 外面；`thread_history_1.sqlite`、`state_5.sqlite`、
            // `memories_1.sqlite` 是会话历史、状态与记忆，绝不入表。
            if !name.starts_with("logs") || !name.ends_with(".sqlite") {
                continue;
            }
            if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            t.push(target_with_recommendation(
                entry.path(),
                Text::new(
                    format!("{label} · 日志库 {name}"),
                    format!("{label} · log database {name}"),
                ),
                CategoryId::AiAgents,
                false,
            ));
        }
    }
}

/// Devin CLI / Codex CLI 这类自管理版本目录的旧版本回收。
///
/// 两者是同一个形状：一堆版本目录 + 一个 `current` 软链接指向当前版本。
/// 旧版本目录纯属垃圾（本机 Codex 四个旧版本共 1.17 GB），删了最多重新
/// 下载——但**当前版本只能从 `current` 读**，不能按版本号大小猜：用户可能
/// 回滚到旧版，`current` 缺失时一个都不预选（分不清就别默认删）。
fn push_old_version_dirs(
    t: &mut Vec<ScanTarget>,
    versions_dir: &Path,
    keep: Option<&str>,
    label: &str,
    recommended: bool,
) {
    let Ok(entries) = std::fs::read_dir(versions_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // `current` 是软链接、`_download` 是安装包暂存、`install.lock` /
        // `auto-update-version` 是更新器自己的书签：都不是版本目录。
        if name == "current" || name == "_download" || Some(name.as_str()) == keep {
            continue;
        }
        let path = entry.path();
        if !entry
            .file_type()
            .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
        {
            continue;
        }
        t.push(target_with_recommendation(
            path,
            Text::new(
                format!("{label} · 旧版本 {name}"),
                format!("{label} · old version {name}"),
            ),
            CategoryId::AiAgents,
            recommended,
        ));
    }
}

/// `<root>/current` 软链接指向的目录名。读不到就 `None`。
fn current_version_name(root: &Path) -> Option<String> {
    std::fs::read_link(root.join("current"))
        .ok()
        .and_then(|target| target.file_name().map(|n| n.to_string_lossy().into_owned()))
}

/// 安装目录里的锁文件很新 → 可能正在换版，这一轮不预选。
///
/// 锁文件在更新完成后会被删掉；长期残留的锁（本机 Codex 的
/// `install.lock` 空文件从 8 月留到现在）说明上次更新异常退出，此时预选是
/// 安全的。
fn update_in_flight(lock: &Path) -> bool {
    lock.exists() && !super::helpers::is_older_than(lock, std::time::Duration::from_secs(3600))
}

/// Codex CLI 自管理的版本目录：`~/.codex/packages/standalone/releases/<版本>/`。
///
/// 每次 `codex` 自更新都会解压一份新版本目录，旧的从来不清（本机 5 份共
/// 1.4 GB，其中四个旧版本 1.17 GB）。当前版本由 `standalone/current`
/// 软链接指向。
fn push_codex_cli_versions(t: &mut Vec<ScanTarget>, home: &Path) {
    let standalone = home.join(".codex/packages/standalone");
    if !standalone.is_dir() {
        return;
    }
    let current = current_version_name(&standalone);
    let updating = update_in_flight(&standalone.join("install.lock"));
    push_old_version_dirs(
        t,
        &standalone.join("releases"),
        current.as_deref(),
        "Codex CLI",
        current.is_some() && !updating,
    );
}

/// Devin CLI 自管理的版本目录：`_versions/<版本>/` 与 `_download/*.tar.gz`。
///
/// Devin CLI 每次 `devin update` 都会下载新安装包、解压出新版本目录，
/// 但从不回收旧的——旧版本目录和下载包纯粹是垃圾，删掉只代价重新下载。
/// 版本目录的处置与 Codex CLI 共用 [`push_old_version_dirs`]。
///
/// 路径平台相关：
/// - macOS / Linux：`~/.local/share/devin/cli/_versions`
/// - Windows：`%APPDATA%\devin\cli\_versions`
fn push_devin_cli_versions(t: &mut Vec<ScanTarget>, home: &Path, roaming: &Path) {
    const AGENT: CategoryId = CategoryId::AiAgents;

    // Windows 只用 roaming；home 仅供 macOS/Linux 分支使用。
    #[cfg(windows)]
    let _ = home;
    #[cfg(windows)]
    let cli_root = roaming.join("devin/cli");
    #[cfg(not(windows))]
    let cli_root = {
        let _ = roaming;
        home.join(".local/share/devin/cli")
    };

    let versions_dir = cli_root.join("_versions");
    if !versions_dir.is_dir() {
        return;
    }

    // `current` 缺失时无法确定当前版本，不预选任何版本目录——分不清就别默认删。
    // `_update.lock` 很新说明可能正在换版，同样不预选。
    let current_version = current_version_name(&versions_dir);
    let updating = update_in_flight(&cli_root.join("_update.lock"));
    push_old_version_dirs(
        t,
        &versions_dir,
        current_version.as_deref(),
        "Devin CLI",
        current_version.is_some() && !updating,
    );

    // 下载的安装包：装完即废，删了最多重新下载
    let download_dir = versions_dir.join("_download");
    if let Ok(entries) = std::fs::read_dir(&download_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".tar.gz") {
                continue;
            }
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            t.push(target_with_recommendation(
                path,
                Text::new(
                    format!("Devin CLI · 安装包 {name}"),
                    format!("Devin CLI · installer {name}"),
                ),
                AGENT,
                !updating,
            ));
        }
    }
}

/// 只报告已明确指向"用户主目录下不存在文件夹"的本地工作区。
/// 远程 URI、外接盘和含百分号编码的 URI 都跳过，避免把暂时离线的项目误报。
pub(super) fn push_orphaned_editor_workspaces(
    t: &mut Vec<ScanTarget>,
    home: &Path,
    roaming: &Path,
) {
    for host in VSCODE_HOSTS {
        let root = roaming.join(&host).join("User/workspaceStorage");
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
fn vscode_family() -> Vec<crate::core::rules::Layout> {
    layouts("vscode_family")
}

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
    for row in vscode_family() {
        push_obsolete_extensions_for_root(
            t,
            &home.join(&row.path).join("extensions"),
            &row.zh,
            &row.en,
        );
    }
}

fn push_obsolete_extensions_for_root(
    t: &mut Vec<ScanTarget>,
    root: &Path,
    editor_zh: &str,
    editor_en: &str,
) {
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
                format!("过期 {editor_zh} 扩展 · {name}"),
                format!("Obsolete {editor_en} extension · {name}"),
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
        let codex = root.join(".codex");
        std::fs::create_dir_all(&codex).unwrap();
        for name in [
            "logs_2.sqlite",
            "thread_history_1.sqlite",
            "state_5.sqlite",
            "memories_1.sqlite",
        ] {
            std::fs::write(codex.join(name), b"db").unwrap();
        }
        let mut targets = Vec::new();
        push_agent_log_databases(&mut targets, &root);

        assert_eq!(targets.len(), 1, "{targets:?}");
        assert!(targets[0].path.ends_with("logs_2.sqlite"));
        assert!(
            !targets[0].recommended,
            "日志库开着 WAL，预选只会制造必然失败"
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
        let mut targets = Vec::new();

        push_obsolete_vscode_extensions(&mut targets, &root);

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, old);
        assert!(targets[0].recommended);
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
