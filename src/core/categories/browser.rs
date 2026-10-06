//! 浏览器缓存：Chrome / Edge / Firefox / Safari / Brave / Arc / Opera / Vivaldi

#[cfg(target_os = "macos")]
use super::target;
use super::{target_with_recommendation, ScanTarget};
use crate::core::categories::CategoryId;
use crate::core::i18n::Text;
use crate::core::rules::Operation;
use std::path::Path;

/// 所有浏览器缓存目标
pub(super) fn push_browser_targets(t: &mut Vec<ScanTarget>, home: &Path) {
    #[cfg(windows)]
    let _ = home;
    #[cfg(windows)]
    {
        let Some(local) = crate::platform::user_cache_dir() else {
            return;
        };

        // 根目录在 browsers 规则的 windows_user_data。目录不存在时不产出目标。
        for row in browser_catalog("windows_user_data") {
            push_chromium_browser_targets(t, &local.join(&row.path), &row.zh, &row.en);
        }
    }

    #[cfg(target_os = "macos")]
    {
        let cache = home.join("Library/Caches");
        let app_support = home.join("Library/Application Support");

        // ~/Library/Caches/<产品名>。Edge 的实际缓存是产品名目录，不是 bundle id。
        // 指到产品目录才能覆盖多 profile。这些名字同时用于跳过目录展开，避免双算。
        for row in browser_catalog("library_caches") {
            t.push(target(
                cache.join(&row.path),
                Text::new(row.zh.as_str(), row.en.as_str()),
                CategoryId::BrowserCache,
                Operation::Contents,
                ("engine", "browser_cache"),
            ));
        }

        // 浏览器 Application Support 下的缓存子目录
        // Chromium 系浏览器在 ~/Library/Application Support 下也存了大量缓存：
        // Code Cache、GPUCache、着色器缓存、Crashpad/completed 等。
        // 这些不在 ~/Library/Caches 下，上面的展开够不到。
        push_browser_app_support_caches(t, &app_support);

        // Firefox 的 `Profiles/<profile>/cache2` 走 macos 规则的
        // named_directories 条目，不在代码里拼目录名。

        // Mail Downloads 中的附件是用户主动打开或保存过的文件，不是缓存，
        // 不能进入智能清理候选。
    }
}

/// Chromium 系浏览器的缓存目标（Windows）。
///
/// 根已由 `browsers.toml` 声明，所以不走内容签名；叶子词汇、profile 名单、
/// Crashpad 规则与开发入口共用 `categories::chromium`。标签沿用「所有者 ·
/// 相对路径」形状，多 profile 时自然带上 profile 名。
#[cfg(any(windows, test))]
pub(super) fn push_chromium_browser_targets(
    t: &mut Vec<ScanTarget>,
    user_data_dir: &std::path::Path,
    name_zh: &str,
    name_en: &str,
) {
    if !user_data_dir.is_dir() {
        return;
    }
    for leaf in super::chromium::declared_leaves(user_data_dir) {
        let trail = super::chromium::leaf_trail(user_data_dir, &leaf);
        let Some(leaf_name) = trail.last() else {
            continue;
        };
        t.push(target_with_recommendation(
            leaf,
            Text::new(
                format!("{name_zh} · {}", trail.join(" · ")),
                format!("{name_en} · {}", trail.join(" · ")),
            ),
            CategoryId::BrowserCache,
            super::chromium::leaf_recommended(leaf_name),
            Operation::Contents,
            ("engine", "browser_cache"),
        ));
    }
}

/// Chromium 系浏览器在 `~/Library/Application Support` 下的根目录：
/// `(相对路径, 显示名)`。
///
/// 抽成常量是因为它现在有两个用途：这里逐浏览器产出缓存目标，
/// `dev::push_chromium_app_caches` 则要靠它**跳过**这些目录——两边认的是
/// 同一份事实，靠各自的名单对不上就是体积双算。
///
/// 不在 macOS 下也编译：跳过名单是纯字符串比较，跨平台共用一个判断比
/// 写两个 `cfg` 分支靠谱（Windows 上浏览器的 userData 在 `%LOCALAPPDATA%`，
/// 本就不在这张表覆盖的目录里，多比一次无害）。
fn browser_catalog(name: &str) -> Vec<crate::core::rules::Layout> {
    crate::core::rules::current()
        .definition("browsers")
        .catalogs
        .get(name)
        .cloned()
        .unwrap_or_default()
}

/// `~/Library/Application Support` 的顶层目录是否整体由浏览器规则认领。
/// `Google/Chrome` 只认领 Chrome，不能把 Google 的其他孩子一起跳过。
pub(super) fn owns_app_support_dir(name: &str) -> bool {
    browser_catalog("app_support")
        .iter()
        .any(|row| row.path == name)
}

pub(super) fn claimed_browser_child(parent: &str, child: &str) -> bool {
    browser_catalog("app_support").iter().any(|row| {
        row.path
            .split_once('/')
            .is_some_and(|(head, tail)| head == parent && tail == child)
    })
}

pub(super) fn contains_claimed_browser_child(parent: &str) -> bool {
    browser_catalog("app_support").iter().any(|row| {
        row.path
            .split_once('/')
            .is_some_and(|(head, _)| head == parent)
    })
}

/// Chromium 系浏览器在 `~/Library/Application Support` 下的缓存子目录。
///
/// Chrome / Arc / Brave / Edge 等都基于 Chromium，缓存布局一致：
/// `<UserDataDir>/<Profile>/Code Cache`、`GPUCache`、着色器缓存等。
/// 这些不在 `~/Library/Caches` 下，需要单独发现。
///
/// 叶子词汇、profile 名单和 `Crashpad/completed` 的细分全部来自
/// `categories::chromium`：浏览器入口不再另抄一份名单。崩溃报告仍然只收
/// 已完成的，不在写的 `pending/` 不收。
#[cfg(any(target_os = "macos", test))]
pub(super) fn push_browser_app_support_caches(t: &mut Vec<ScanTarget>, app_support: &Path) {
    for row in browser_catalog("app_support") {
        let root = app_support.join(&row.path);
        if !root.is_dir() {
            continue;
        }
        for leaf in super::chromium::declared_leaves(&root) {
            let trail = super::chromium::leaf_trail(&root, &leaf);
            let Some(leaf_name) = trail.last() else {
                continue;
            };
            t.push(target_with_recommendation(
                leaf,
                Text::new(
                    format!("{} · {}", row.zh, trail.join(" · ")),
                    format!("{} · {}", row.en, trail.join(" · ")),
                ),
                CategoryId::BrowserCache,
                super::chromium::leaf_recommended(leaf_name),
                Operation::Contents,
                ("engine", "browser_cache"),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"isolated").unwrap();
    }

    /// 浏览器根目录不存在时不产出目标。Profile 枚举只发生在目录真的在的时候。
    #[test]
    fn missing_browser_user_data_emits_nothing() {
        let root = crate::core::testing::fixture("qc_missing_browser");
        let missing = root.join("NoSuchBrowser/User Data");
        let mut targets = Vec::new();
        super::push_chromium_browser_targets(&mut targets, &missing, "Chrome", "Chrome");
        assert!(targets.is_empty(), "{targets:?}");
        let _ = std::fs::remove_dir_all(root);
    }

    /// 规则声明过的浏览器根不再自备一份叶子名单：Default/profile 的叶子都来自
    /// `chromium.toml`，根目录本身和未知子目录都不入表。
    #[test]
    fn declared_browser_roots_share_the_chromium_leaf_vocabulary() {
        let root = crate::core::testing::fixture("qc_browser_leaves");
        let user_data = root.join("Google/Chrome/User Data");
        for leaf in ["Default/Cache", "Default/Code Cache", "Profile 1/Cache"] {
            write(&user_data.join(leaf).join("data"));
        }
        write(&user_data.join("Default/blob_storage/data"));
        std::fs::create_dir_all(user_data.join("Default/GPUCache")).unwrap();
        write(&user_data.join("Local State"));
        let mut targets = Vec::new();

        super::push_chromium_browser_targets(&mut targets, &user_data, "Chrome", "Chrome");

        let entry = |rel: &str| targets.iter().find(|t| t.path == user_data.join(rel));
        assert_eq!(entry("Default/Cache").map(|t| t.recommended), Some(true));
        assert_eq!(entry("Profile 1/Cache").map(|t| t.recommended), Some(true));
        assert_eq!(
            entry("Default/blob_storage").map(|t| t.recommended),
            Some(false),
            "blob 存储可能承载未保存内容，只展示"
        );
        assert!(entry("Default/GPUCache").is_none(), "空缓存目录不产出目标");
        assert!(entry("Local State").is_none());
        assert_eq!(
            entry("Default/Cache").map(|t| t.label.get(crate::core::i18n::Language::Zh)),
            Some("Chrome · Default · Cache")
        );
        assert!(!targets.iter().any(|t| t.path == user_data));
        let _ = std::fs::remove_dir_all(root);
    }

    /// macOS 的 Application Support 浏览器缓存共用同一份叶子词汇与 Crashpad 规则：
    /// 只收 `Crashpad/completed`，非 profile 子目录不扫，未声明的浏览器不产出。
    #[test]
    fn app_support_caches_follow_the_declared_catalog() {
        let root = crate::core::testing::fixture("qc_app_support_browser");
        let app_support = root.join("Library/Application Support");
        let chrome = app_support.join("Google/Chrome");
        for leaf in ["Default/GPUCache", "Default/Code Cache"] {
            write(&chrome.join(leaf).join("data"));
        }
        write(&chrome.join("Crashpad/completed/report.dmp"));
        write(&chrome.join("Crashpad/pending/report.dmp"));
        write(&chrome.join("CachedExtensionVSIXs/extension.vsix"));
        write(&chrome.join("SomeOtherDir/GPUCache/data"));
        write(&app_support.join("Safari/Cache/data"));
        let mut targets = Vec::new();

        super::push_browser_app_support_caches(&mut targets, &app_support);

        let entry = |rel: &str| targets.iter().find(|t| t.path == chrome.join(rel));
        assert_eq!(entry("Default/GPUCache").map(|t| t.recommended), Some(true));
        assert_eq!(
            entry("CachedExtensionVSIXs").map(|t| t.recommended),
            Some(true)
        );
        assert_eq!(
            entry("Crashpad/completed").map(|t| t.label.get(crate::core::i18n::Language::Zh)),
            Some("Chrome · Crashpad · completed")
        );
        assert!(
            entry("Crashpad/pending").is_none(),
            "仍在写的崩溃报告不能被当作可清理产物"
        );
        assert!(
            entry("SomeOtherDir/GPUCache").is_none(),
            "非 profile 子目录不按 Chromium 叶子展开"
        );
        assert!(
            !targets
                .iter()
                .any(|t| t.path.starts_with(app_support.join("Safari"))),
            "只有 catalogs.app_support 声明过的浏览器根会被展开"
        );
        assert!(!targets.iter().any(|t| t.path == chrome));
        let _ = std::fs::remove_dir_all(root);
    }

    /// 浏览器入口金样：Windows User Data 的目标 / 推荐 / 操作 / 处置逐项对照。
    #[test]
    fn browser_layout_matches_the_baseline() {
        let root = crate::core::testing::fixture("qc_browser_layout");
        let _ = std::fs::remove_dir_all(&root);
        let user_data = root.join("Google/Chrome/User Data");
        for leaf in [
            "Default/Cache",
            "Default/Code Cache",
            "Default/GPUCache",
            "Default/DawnGraphiteCache",
            "Default/Service Worker/CacheStorage",
        ] {
            write(&user_data.join(leaf).join("data"));
        }
        write(&user_data.join("Default/blob_storage/data"));
        write(&user_data.join("Default/CachedProfilesData/data"));
        write(&user_data.join("Profile 1/Cache/data"));
        write(&user_data.join("Default/Cookies/x"));
        write(&user_data.join("Local State"));
        let mut targets = Vec::new();
        super::push_chromium_browser_targets(&mut targets, &user_data, "Chrome", "Chrome");
        let mut actual: Vec<serde_json::Value> = targets
            .iter()
            .map(|t| {
                serde_json::json!({
                    "path": t.path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"),
                    "label_zh": t.label.get(crate::core::i18n::Language::Zh),
                    "category": format!("{:?}", t.category),
                    "operation": t.operation,
                    "disposal": t.disposal,
                    "recommended": t.recommended,
                })
            })
            .collect();
        actual.sort_by_key(|row| row["path"].as_str().unwrap().to_owned());
        let expected: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../rules/fixtures/browser-layout-baseline.json"
        ))
        .unwrap();
        assert_eq!(actual, expected);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 随程序发布闭环的浏览器入口证明：Windows User Data 的目标表面逐行
    /// 跟随内置 browsers.toml 的 `windows_user_data`——期望从 embedded
    /// bundle 推导，新增一个浏览器根（只改 TOML + 重编译）后本测试自动
    /// 覆盖新根，无需任何 Rust 改动。
    #[test]
    fn browser_roots_follow_the_embedded_catalog() {
        let root = crate::core::testing::fixture("qc_browser_catalog_surface");
        let _ = std::fs::remove_dir_all(&root);
        let rows = super::browser_catalog("windows_user_data");
        assert!(
            !rows.is_empty(),
            "browsers.toml 的 windows_user_data 不应为空"
        );

        // 每个声明的浏览器根放一个带叶子与登录态的最小布局。
        for row in &rows {
            let user_data = root.join(&row.path);
            for leaf in ["Default/Cache", "Default/Code Cache"] {
                write(&user_data.join(leaf).join("data"));
            }
            write(&user_data.join("Default/Cookies").join("x"));
        }

        let mut targets = Vec::new();
        for row in &rows {
            super::push_chromium_browser_targets(
                &mut targets,
                &root.join(&row.path),
                &row.zh,
                &row.en,
            );
        }

        assert_eq!(targets.len(), rows.len() * 2, "{targets:?}");
        for row in &rows {
            let user_data = root.join(&row.path);
            for leaf in ["Default/Cache", "Default/Code Cache"] {
                let target = targets
                    .iter()
                    .find(|t| t.path == user_data.join(leaf))
                    .unwrap_or_else(|| panic!("{} 的 {leaf} 没有按内置规则表面化", row.path));
                assert!(target.recommended);
                let zh = target.label.get(crate::core::i18n::Language::Zh);
                assert!(zh.starts_with(&row.zh), "标签应带上浏览器名：{zh}");
            }
            assert!(
                !targets
                    .iter()
                    .any(|t| t.path == user_data.join("Default/Cookies")),
                "{} 的登录态不能入表",
                row.path
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }

    /// 「仅配置增量」：给 `browsers` 规则的 `windows_user_data` 加一行
    /// （`NovaSoft/Nova/User Data`），已编译客户端下一次扫描就按同一份
    /// Chromium 叶子词汇把这个新根表面化——没有为这个新浏览器根加任何
    /// Rust 分支。期望不写死具体根名，而是从夹具规则逐行推导。
    #[test]
    fn browser_rule_only_fixture_surfaces_a_new_root_with_shared_leaves() {
        use std::sync::Arc;
        let root = crate::core::testing::fixture("qc_browser_extra");
        let _ = std::fs::remove_dir_all(&root);
        let extra: crate::core::rules::RuleDefinition =
            toml::from_str(include_str!("../../../rules/fixtures/browser-extra.toml")).unwrap();
        let mut bundle = crate::core::rules::current().bundle.clone();
        bundle.rules.retain(|rule| rule.id != "browsers");
        bundle.rules.push(extra);
        bundle.validate().unwrap();
        let snapshot = Arc::new(crate::core::rules::RuleSnapshot { bundle });

        // 新增根在夹具磁盘上造出来：两片缓存叶子 + 登录态反例。
        let user_data = root.join("NovaSoft/Nova/User Data");
        for leaf in ["Default/Cache", "Default/Code Cache"] {
            write(&user_data.join(leaf).join("data"));
        }
        write(&user_data.join("Default/Cookies").join("x"));

        let (nova, targets) = crate::core::rules::with_snapshot(snapshot, || {
            let rows = super::browser_catalog("windows_user_data");
            let nova = rows
                .iter()
                .find(|row| row.path == "NovaSoft/Nova/User Data")
                .expect("夹具必须真的新增了这个浏览器根")
                .clone();
            let mut targets = Vec::new();
            for row in &rows {
                super::push_chromium_browser_targets(
                    &mut targets,
                    &root.join(&row.path),
                    &row.zh,
                    &row.en,
                );
            }
            (nova, targets)
        });

        let entry = |rel: &str| targets.iter().find(|t| t.path == user_data.join(rel));
        let cache = entry("Default/Cache").expect("新根必须按共享叶子词汇表面化");
        assert!(cache.recommended);
        assert!(
            cache
                .label
                .get(crate::core::i18n::Language::Zh)
                .starts_with(&nova.zh),
            "标签应带上新根的自定义名字"
        );
        assert!(entry("Default/Code Cache").is_some());
        assert!(entry("Default/Cookies").is_none(), "登录态不能入表");
        let _ = std::fs::remove_dir_all(root);
    }

    /// 浏览器入口逐次扫描结果一致：同一夹具多次 `push_chromium_browser_targets`
    /// 产出相同的目标集合（无累积、无依赖枚举顺序）。
    #[test]
    fn browser_scan_is_deterministic_across_runs() {
        let root = crate::core::testing::fixture("qc_browser_determinism");
        let _ = std::fs::remove_dir_all(&root);
        let user_data = root.join("Google/Chrome/User Data");
        for leaf in [
            "Default/Cache",
            "Default/Code Cache",
            "Default/GPUCache",
            "Default/DawnGraphiteCache",
        ] {
            write(&user_data.join(leaf).join("data"));
        }
        write(&user_data.join("Default/blob_storage/data"));
        write(&user_data.join("Profile 1/Cache/data"));
        let scan = || {
            let mut targets = Vec::new();
            super::push_chromium_browser_targets(&mut targets, &user_data, "Chrome", "Chrome");
            let mut paths: Vec<String> = targets
                .iter()
                .map(|t| crate::core::safety::norm(&t.path))
                .collect();
            paths.sort();
            paths
        };
        let first = scan();
        assert!(!first.is_empty());
        assert_eq!(first, scan(), "浏览器入口逐次一致");
        assert_eq!(first, scan());
        let _ = std::fs::remove_dir_all(root);
    }
}
