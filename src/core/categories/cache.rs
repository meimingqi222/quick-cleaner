//! 用户缓存、包管理缓存、缩略图缓存

#[cfg(any(target_os = "macos", test))]
use super::target_with_recommendation;
#[cfg(target_os = "macos")]
use super::target_with_size;
use super::ScanTarget;
#[cfg(any(target_os = "macos", test))]
use crate::core::categories::CategoryId;
#[cfg(target_os = "macos")]
use crate::core::i18n::Text;
#[cfg(any(target_os = "macos", test))]
use crate::core::rules::Operation;
use std::path::Path;

/// 包管理缓存、用户缓存、缩略图缓存。
///
/// `~/.cache` 布局与包缓存派发在 cache 规则的 `home_cache` 目录条目里，由
/// 通用目录能力统一枚举；`home` 为 None 时跳过用户级缓存。
pub(super) fn push_cache_targets(
    t: &mut Vec<ScanTarget>,
    home: Option<&Path>,
    brew_cleanup_at: Option<i64>,
) {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = home {
            push_user_cache_targets(t, home);
        }
        // brew cleanup 不是一个目录，体积和是否出现都取决于 dry-run 与节流。
        if crate::core::brew::should_offer(brew_cleanup_at) {
            if let Some((bytes, _files)) = crate::core::brew::cleanup_preview() {
                t.push(target_with_size(
                    crate::core::brew::virtual_path(),
                    Text::new("Homebrew 清理", "Homebrew cleanup"),
                    CategoryId::PackageCache,
                    bytes,
                    Operation::Brew,
                    ("engine", "brew_cleanup"),
                ));
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (t, home, brew_cleanup_at);
}

/// 用户缓存（`~/Library/Caches` 展开等）
#[cfg(target_os = "macos")]
fn push_user_cache_targets(t: &mut Vec<ScanTarget>, home: &Path) {
    let cache = home.join("Library/Caches");

    // `~/Library/Caches` 剩下的部分。
    //
    // 这里逐个展开顶层子目录，而不是把整个 `~/Library/Caches` 作为一个目标：
    // 它和上面的浏览器 / Homebrew 缓存是父子关系，而 `scanner` 不做嵌套去重
    // （`scan_fixed_inner` 逐目标独立称重后直接相加），父子同时入表会让总量
    // 凭空翻倍。展开后顺带能按目录名给出标签，比一个不透明的大块更有用。
    push_user_cache_dirs(t, &cache);
}

/// `~/Library/Caches` 下已被整目录认领的名字，以及只认领了某个孩子的父目录。
///
/// 来源就是会产出这些目标的规则：`Library/Caches/<name>` 的条目，和浏览器
/// 规则的 `library_caches` 目录。一段路径是整目录认领；两段路径只认领孩子，
/// 父目录的其余孩子仍然入表。local agent 的孩子不在这里，扫描时另外并上。
#[cfg(any(target_os = "macos", test))]
pub(super) fn library_cache_claims() -> (Vec<String>, Vec<(String, Vec<String>)>) {
    let snapshot = crate::core::rules::current();
    let mut whole = std::collections::BTreeSet::new();
    let mut partial: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    let mut note = |path: &str| {
        let Some(rest) = path.strip_prefix("Library/Caches/") else {
            return;
        };
        if rest.is_empty() {
            return;
        }
        match rest.split_once('/') {
            None => {
                whole.insert(rest.to_string());
            }
            Some((parent, child))
                if !parent.is_empty() && !child.is_empty() && !child.contains('/') =>
            {
                partial
                    .entry(parent.to_string())
                    .or_default()
                    .insert(child.to_string());
            }
            Some(_) => {}
        }
    };
    for rule in &snapshot.bundle.rules {
        if rule.platform != "all" && rule.platform != "macos" {
            continue;
        }
        for entry in &rule.entries {
            if entry.root == "home" {
                note(&entry.path);
            }
        }
        if let Some(rows) = rule.catalogs.get("library_caches") {
            for row in rows {
                note(&format!("Library/Caches/{}", row.path));
            }
        }
    }
    (
        whole.into_iter().collect(),
        partial
            .into_iter()
            .map(|(parent, kids)| (parent, kids.into_iter().collect()))
            .collect(),
    )
}

/// 把 `~/Library/Caches` 的顶层子目录逐个加为清理目标，跳过已被认领的。
///
/// 分成三步判定，而不是一律丢进同一个桶：
/// 1. 内容命中更新包签名 → 按子项拆开，更新包叶子可默认勾选；
/// 2. 没命中 → 整目录作为「分不清」的一项展示，不预选；
/// 3. 探测排除名单（cache 规则 `updater_probe_exclude_prefixes`）→ 不做探测，
///    见下方说明。
#[cfg(any(target_os = "macos", test))]
pub(super) fn push_user_cache_dirs(t: &mut Vec<ScanTarget>, cache: &Path) {
    let Ok(rd) = std::fs::read_dir(cache) else {
        return;
    };
    let (whole, partial) = library_cache_claims();
    let probe_excludes = crate::core::rules::current()
        .list("cache", "updater_probe_exclude_prefixes")
        .to_vec();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if whole.iter().any(|claimed| claimed == &name) {
            continue;
        }
        // 跳过 Apple 系统服务缓存：这些涉及认证、iCloud、安全等关键服务，
        // 清理后可能导致重新登录、iCloud 同步异常、安全提示等问题。
        if super::helpers::is_sensitive_apple_cache(&name) {
            continue;
        }
        // 只要目录：Caches 顶层的散落文件通常是 App 自己的状态，不碰。
        if !entry.file_type().is_ok_and(|ft| ft.is_dir()) {
            continue;
        }
        let dir = entry.path();
        // 该父目录里已由更具体规则认领走的孩子（普通目录为空）。父目录不能
        // 入表（会和孩子的体积双算），但其余孩子必须入表，否则它们隐身。
        let rule_claimed = super::dev::local_agent_claimed_children(&name);
        let table_claimed = partial
            .iter()
            .find(|(parent, _)| parent == &name)
            .map(|(_, kids)| kids.as_slice())
            .unwrap_or(&[]);
        let claimed: Vec<&str> = table_claimed
            .iter()
            .map(String::as_str)
            .chain(rule_claimed.iter().map(String::as_str))
            .collect();
        let stem = super::updater::display_stem(&name);
        // 签名判定是按第三方更新器的产物形态做的，对 Apple 守护进程的目录
        // 没有意义：上面那张敏感表只列了确认危险的，其余探测排除名单里的
        // 目录并不因此安全，所以一律不探测、只展示。
        let hit = !probe_excludes
            .iter()
            .any(|prefix| name.starts_with(prefix.as_str()))
            && super::updater::push_updater_artifacts(t, &dir, &stem);
        if hit || !claimed.is_empty() {
            push_residual_children(t, &dir, &name, &claimed);
            continue;
        }
        // `~/Library/Caches` 是约定上的缓存位置，但第三方软件并不总遵守：
        // JetBrains 在这里放 LocalHistory/fileHistory，ms-playwright 也可能放
        // 带登录态的 MCP 浏览器 Profile。未知目录只展示，不能默认勾选。
        t.push(target_with_recommendation(
            dir,
            format!("~/Library/Caches/{name}"),
            CategoryId::UserTemp,
            false,
            Operation::Contents,
            ("engine", "cache_candidate"),
        ));
    }
}

/// 父目录没能入表时，把它的顶层子项逐个补进目标表：形态仍然分不清，只展示、
/// 不默认勾选。`skip` 是已经由别的规则认领走的孩子，不能重复入表。
///
/// 两种拆分共用这条路：内容命中更新包签名（`updater.rs`），以及父目录只被
/// 部分认领。共同前提是父目录本身不能
/// 入表——`scan_fixed_inner` 逐目标独立称重后相加、不做嵌套去重，父子同时入表
/// 会让体积翻倍。但**兄弟不该为父目录的缺席陪葬**：`Cache.db` 这类占着目录里
/// 最大一块体积的东西，不列出来就等于从界面上消失。
#[cfg(any(target_os = "macos", test))]
fn push_residual_children(t: &mut Vec<ScanTarget>, dir: &Path, name: &str, skip: &[&str]) {
    for child in super::updater::residual_children(dir, skip) {
        t.push(target_with_recommendation(
            dir.join(&child),
            format!("~/Library/Caches/{name}/{child}"),
            CategoryId::UserTemp,
            false,
            Operation::Contents,
            ("engine", "cache_candidate"),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::library_cache_claims;
    use super::push_user_cache_dirs;
    #[cfg(target_os = "macos")]
    use crate::core::categories::helpers::backdate;
    use crate::core::categories::CategoryId;
    use std::path::{Path, PathBuf};

    /// 生产选择器的隔离入口：cache 规则的目录条目在给定 home 上展开。
    fn home_cache_scan(home: &Path) -> Vec<super::ScanTarget> {
        let snapshot = crate::core::rules::current();
        let (mut targets, _) = crate::core::rules::directories::scan_at(
            &snapshot,
            "cache",
            &crate::core::rules::directories::fixture_roots(home, &home.join("roaming")),
        );
        targets.sort_by(|a, b| a.path.cmp(&b.path));
        targets
    }

    fn home_cache_row(target: &super::ScanTarget, root: &Path) -> serde_json::Value {
        serde_json::json!({
            "relative": target
                .path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/"),
            "label": target.label.get(crate::core::i18n::Language::Zh),
            "category": format!("{:?}", target.category),
            "recommended": target.recommended,
            "operation": format!("{:?}", target.operation),
            "disposal": format!("{:?}", target.disposal),
        })
    }

    /// 「仅配置增量」：给 cache 规则的包缓存目录清单加一行（`cache-tool`），
    /// 通用 `catalog_children` 能力立刻把它展开成目标——没有为它加专用 Rust 分支。
    #[test]
    fn cache_catalog_rule_only_fixture_surfaces_a_new_package() {
        use std::sync::Arc;
        let root = crate::core::testing::fixture("cache_catalog_extra");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".cache/cache-tool")).unwrap();
        let extra: crate::core::rules::RuleDefinition = toml::from_str(include_str!(
            "../../../rules/fixtures/cache-catalog-extra.toml"
        ))
        .unwrap();
        let mut bundle = crate::core::rules::current().bundle.clone();
        bundle.rules.retain(|rule| rule.id != "cache");
        bundle.rules.push(extra);
        bundle.validate().unwrap();
        let snapshot = Arc::new(crate::core::rules::RuleSnapshot { bundle });
        let (targets, _) = crate::core::rules::directories::scan_at(
            &snapshot,
            "cache",
            &crate::core::rules::directories::fixture_roots(&root, &root.join("roaming")),
        );
        let found = targets
            .iter()
            .find(|target| target.path == root.join(".cache/cache-tool"))
            .expect("the added package catalog must surface as a target");
        assert_eq!(found.category, CategoryId::PackageCache);
        assert!(found.recommended);
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
        let root = crate::core::testing::fixture("qc_mixed_cache");
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
        let root = crate::core::testing::fixture("qc_updater_age");
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

    /// `~/.cache/<tool>` 命中 Chromium 数据目录时只收叶子。
    ///
    /// 实测案例：`~/.cache/chrome-devtools-mcp` 整个 228 MB，可重建的只有
    /// 85 MB，剩下是那个浏览器 Profile 本体（Cookies / Login Data / DIPS）。
    /// 整目录入表是拿登录态换空间，整条只展示不勾选又等于缓存永远清不掉。
    #[test]
    fn home_cache_chromium_profile_yields_leaves_not_the_root() {
        let root = crate::core::testing::fixture("qc_home_cache_mcp");
        let tool = root.join(".cache/chrome-devtools-mcp");
        let profile = tool.join("chrome-profile/Default");
        for leaf in ["Cache", "Code Cache", "GPUCache"] {
            std::fs::create_dir_all(profile.join(leaf)).unwrap();
            std::fs::write(profile.join(leaf).join("data"), b"x").unwrap();
        }
        std::fs::create_dir_all(profile.join("Cookies")).unwrap();
        std::fs::create_dir_all(tool.join("chrome-profile/IndexedDB")).unwrap();

        let targets = home_cache_scan(&root);
        let paths: Vec<&PathBuf> = targets.iter().map(|t| &t.path).collect();

        assert_eq!(targets.len(), 3, "{paths:?}");
        assert!(targets.iter().all(|t| t.category == CategoryId::UserCache));
        assert!(targets.iter().all(|t| t.recommended), "缓存叶子该预选");
        assert!(paths.contains(&&profile.join("Cache")), "{paths:?}");
        for forbidden in [tool.clone(), tool.join("chrome-profile"), profile.clone()] {
            assert!(
                !paths.contains(&&forbidden),
                "{:?} 是 Profile 本体，不能入表",
                forbidden
            );
        }
        assert!(
            !paths
                .iter()
                .any(|p| p.ends_with("Cookies") || p.ends_with("IndexedDB")),
            "登录态不能入表: {paths:?}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// 只被部分认领的父目录：其余孩子必须入表。
    ///
    /// `browser.rs` 只认领 `Google/Chrome`，而旧写法把 `Google` 整个跳过，于是
    /// 兄弟子项（GoogleUpdater 的下载目录那一类）在界面上彻底隐身——看不见也
    /// 清不掉，比「不默认勾选」更糟。规范说得很清楚：展示不是成本，隐藏才是。
    #[test]
    fn partially_claimed_parent_still_shows_its_other_children() {
        let root = crate::core::testing::fixture("qc_partial_claim");
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
        let root = crate::core::testing::fixture("qc_apple_cache");
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
        let root = crate::core::testing::fixture("qc_home_cache");
        let _ = std::fs::remove_dir_all(&root);
        for name in [
            "uv",
            "pip",
            "pypoetry",
            "opencode",
            "some-tool-nobody-heard-of",
        ] {
            std::fs::create_dir_all(root.join(".cache").join(name)).unwrap();
        }

        let targets = home_cache_scan(&root);

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
        let poetry = targets
            .iter()
            .find(|t| t.path == root.join(".cache/pypoetry"))
            .expect("~/.cache/pypoetry 没有入表");
        assert_eq!(poetry.category, CategoryId::PackageCache);
        assert!(!poetry.recommended, "virtualenvs 还在这个目录里，不能预选");
        let opencode = targets
            .iter()
            .find(|t| t.path == root.join(".cache/opencode"))
            .expect("~/.cache/opencode 没有入表");
        assert_eq!(opencode.category, CategoryId::AiAgents);
        assert!(opencode.recommended);
        let unknown = targets
            .iter()
            .find(|t| t.path == root.join(".cache").join("some-tool-nobody-heard-of"))
            .expect("表外的目录仍应展示");
        assert_eq!(unknown.category, CategoryId::UserTemp);
        assert!(!unknown.recommended);
        let _ = std::fs::remove_dir_all(root);
    }

    /// 迁移金样：`~/.cache` 枚举改走声明式 `catalog_children` 后，目标、标签、
    /// 类别、推荐与处置和旧 Rust 构造（删除前的 `push_home_cache_targets`）
    /// 逐项一致。基线由迁移前的实现产出后提交，路径归一为相对 `<root>`。
    #[test]
    fn home_cache_catalog_children_preserve_the_migration_baseline() {
        let root = crate::core::testing::fixture("qc_home_cache_migration");
        let _ = std::fs::remove_dir_all(&root);
        for name in ["uv", "pypoetry", "opencode", "some-unknown-tool"] {
            std::fs::create_dir_all(root.join(".cache").join(name)).unwrap();
        }
        let profile = root.join(".cache/chrome-devtools-mcp/chrome-profile/Default");
        for leaf in ["Cache", "GPUCache"] {
            std::fs::create_dir_all(profile.join(leaf)).unwrap();
            std::fs::write(profile.join(leaf).join("data"), b"x").unwrap();
        }
        std::fs::create_dir_all(profile.join("Cookies")).unwrap();
        std::fs::create_dir_all(root.join(".cache/empty-leaf-only/Default/GPUCache")).unwrap();
        std::fs::write(root.join(".cache/loose.txt"), b"file").unwrap();

        let targets = home_cache_scan(&root);
        let rows: Vec<serde_json::Value> = targets
            .iter()
            .map(|target| home_cache_row(target, &root))
            .collect();
        let baseline: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../rules/fixtures/cache-catalog-baseline.json"
        ))
        .unwrap();
        assert_eq!(rows, baseline);
        let _ = std::fs::remove_dir_all(root);
    }

    /// 派发与叶子推荐跟着规则走：新增一个目录只需在 cache.toml 加一行；
    /// 一个叶子要不要预选由 chromium.toml 的 shown_leaves 决定。
    #[test]
    fn catalog_children_policy_and_leaf_recommendation_follow_the_rule() {
        use std::sync::Arc;
        let root = crate::core::testing::fixture("qc_home_cache_policy");
        let _ = std::fs::remove_dir_all(&root);
        // 签名成立的最小布局：两个签名叶子。Cache 是被改推荐的目标，
        // GPUCache 是对照组。
        let profile = root.join(".cache/fixturetool/chrome-profile/Default");
        for leaf in ["Cache", "GPUCache"] {
            std::fs::create_dir_all(profile.join(leaf)).unwrap();
            std::fs::write(profile.join(leaf).join("data"), b"x").unwrap();
        }
        std::fs::create_dir_all(root.join(".cache/fixturepkg")).unwrap();

        let targets = home_cache_scan(&root);
        let cache_leaf = targets
            .iter()
            .find(|t| t.path == profile.join("Cache"))
            .expect("签名叶子该入表");
        assert!(cache_leaf.recommended, "普通缓存叶子该预选");
        let fallback = targets
            .iter()
            .find(|t| t.path == root.join(".cache/fixturepkg"))
            .expect("规则没派发的新目录仍走兜底展示");
        assert_eq!(fallback.category, CategoryId::UserTemp);
        assert!(!fallback.recommended);

        let original = crate::core::rules::snapshot();
        let mut bundle = original.bundle.clone();
        let cache = bundle
            .rules
            .iter_mut()
            .find(|rule| rule.id == "cache")
            .unwrap();
        cache.version += 1;
        cache
            .catalogs
            .get_mut("packages")
            .unwrap()
            .push(crate::core::rules::Layout {
                path: "fixturepkg".into(),
                zh: "夹具包缓存".into(),
                en: "Fixture package cache".into(),
                children: Vec::new(),
            });
        let chromium = bundle
            .rules
            .iter_mut()
            .find(|rule| rule.id == "chromium")
            .unwrap();
        chromium.version += 1;
        chromium
            .lists
            .get_mut("shown_leaves")
            .unwrap()
            .push("Cache".into());
        bundle.validate().unwrap();
        let changed = Arc::new(crate::core::rules::RuleSnapshot { bundle });
        let targets = crate::core::rules::with_snapshot(changed.clone(), || home_cache_scan(&root));
        let package = targets
            .iter()
            .find(|t| t.path == root.join(".cache/fixturepkg"))
            .expect("新增目录只需规则加一行");
        assert_eq!(package.category, CategoryId::PackageCache);
        assert!(package.recommended);
        let cache_leaf = targets
            .iter()
            .find(|t| t.path == profile.join("Cache"))
            .expect("进 shown_leaves 的叶子仍要展示");
        assert!(!cache_leaf.recommended, "shown_leaves 里的叶子只展示");
        assert!(
            targets
                .iter()
                .find(|t| t.path == profile.join("GPUCache"))
                .is_some_and(|t| t.recommended),
            "对照组叶子不受影响"
        );
        assert!(Arc::ptr_eq(&package.rule.snapshot, &changed));
        let _ = std::fs::remove_dir_all(root);
    }

    /// `~/Library/Caches` 的整目录认领跟会产出这些目标的规则走，不另维护一张表。
    #[test]
    fn library_cache_claims_follow_the_rules_that_emit_them() {
        let (whole, partial) = library_cache_claims();
        let snap = crate::core::rules::current();

        assert!(
            !whole.iter().any(|name| name == "Google"),
            "Google stays a partial parent: {whole:?}"
        );
        let chrome = partial
            .iter()
            .find(|(parent, _)| parent == "Google")
            .expect("Google/Chrome is a partial claim");
        assert_eq!(chrome.1, ["Chrome".to_string()]);

        for name in ["claude-cli-nodejs", "amp", "Zed", "WorkBuddy"] {
            assert!(
                !whole.iter().any(|claimed| claimed == name),
                "{name} was copied from local_agents into whole-directory claims"
            );
        }
        assert!(
            partial.iter().all(|(parent, kids)| {
                !(parent == "go" && kids.iter().any(|child| child == "pkg" || child == "mod"))
            }),
            "go/pkg/mod must stay a package path, not a Library/Caches claim: {partial:?}"
        );
        assert!(
            whole.iter().any(|name| name == "go"),
            "Library/Caches/go is still a whole-directory claim"
        );

        for rule in snap
            .bundle
            .rules
            .iter()
            .filter(|rule| rule.platform == "all" || rule.platform == "macos")
        {
            for entry in &rule.entries {
                if entry.root != "home" {
                    continue;
                }
                let Some(rest) = entry.path.strip_prefix("Library/Caches/") else {
                    continue;
                };
                if rest.is_empty() || rest.contains('/') {
                    continue;
                }
                assert!(
                    whole.iter().any(|name| name == rest),
                    "{rest} from {} is not a whole-directory claim",
                    rule.id
                );
            }
            if let Some(rows) = rule.catalogs.get("library_caches") {
                for row in rows {
                    match row.path.split_once('/') {
                        Some((parent, child)) if !child.is_empty() && !child.contains('/') => {
                            assert!(
                                !whole.iter().any(|name| name == parent),
                                "{parent} must stay a partial parent"
                            );
                            let kids = partial
                                .iter()
                                .find(|(name, _)| name == parent)
                                .unwrap_or_else(|| panic!("{parent} missing from partial claims"));
                            assert!(
                                kids.1.iter().any(|name| name == child),
                                "{child} missing under {parent}"
                            );
                        }
                        None => assert!(
                            whole.iter().any(|name| name == &row.path),
                            "{} is not a whole-directory claim",
                            row.path
                        ),
                        Some(_) => {}
                    }
                }
            }
        }
    }

    /// 随程序发布闭环（schema=9 重验）：编译产物里的 `~/.cache` 目标表面完全
    /// 跟随内置 TOML——期望逐行从 embedded bundle 的 catalog_policy 推导。
    /// 只改 rules/cache.toml 加一行并重编译（build.rs 重新内嵌规则包），下一
    /// 次扫描就表面化新目标；本测试在改动前后都应保持绿，且断言自动覆盖新行。
    #[test]
    fn bundled_scan_surface_follows_the_embedded_catalog() {
        use crate::core::rules::directories::{CatalogPolicy, Selection};
        use std::collections::BTreeMap;

        let root = crate::core::testing::fixture("qc_bundled_catalog_surface");
        let _ = std::fs::remove_dir_all(&root);
        let cache = root.join(".cache");
        let snapshot = crate::core::rules::snapshot();
        let rule = snapshot.definition("cache");
        let entry = rule
            .directories
            .iter()
            .find(|entry| entry.id == "home_cache")
            .expect("home_cache entry");
        let catalog_policy: &BTreeMap<String, CatalogPolicy> = match &entry.select {
            Selection::CatalogChildren { catalog_policy, .. } => catalog_policy,
            _ => panic!("home_cache must use catalog_children"),
        };

        // 磁盘上放齐全部被派发目录 + 一个表外目录。
        let mut dispatched: Vec<(String, String, bool)> = Vec::new();
        for (catalog, policy) in catalog_policy {
            for row in &rule.catalogs[catalog] {
                std::fs::create_dir_all(cache.join(&row.path)).unwrap();
                dispatched.push((
                    row.path.clone(),
                    policy.category.clone(),
                    policy.recommended,
                ));
            }
        }
        std::fs::create_dir_all(cache.join("not-in-any-catalog")).unwrap();

        let targets = home_cache_scan(&root);
        assert_eq!(
            targets.len(),
            dispatched.len() + 1,
            "目标表面必须恰好是内置规则的派发行 + 一个兜底"
        );
        for (name, category, recommended) in &dispatched {
            let dir = cache.join(name);
            let target = targets
                .iter()
                .find(|t| t.path == dir)
                .unwrap_or_else(|| panic!("内置规则声明的 {name} 没有表面化"));
            assert_eq!(
                target.category,
                CategoryId::from_rule(category).unwrap(),
                "{name} 类别应来自 catalog_policy"
            );
            assert_eq!(
                target.recommended, *recommended,
                "{name} 推荐应来自 catalog_policy"
            );
        }
        let fallback = targets
            .iter()
            .find(|t| t.path == cache.join("not-in-any-catalog"))
            .expect("表外目录仍要兜底展示");
        assert!(!fallback.recommended);
        let _ = std::fs::remove_dir_all(root);
    }
}
