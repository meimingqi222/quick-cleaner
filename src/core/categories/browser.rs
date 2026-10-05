//! 浏览器缓存：Chrome / Edge / Firefox / Safari / Brave / Arc / Opera / Vivaldi

use super::{target, ScanTarget};
use crate::core::categories::CategoryId;
use crate::core::i18n::Text;
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
            ));
        }

        // 浏览器 Application Support 下的缓存子目录
        // Chromium 系浏览器在 ~/Library/Application Support 下也存了大量缓存：
        // Code Cache、GPUCache、着色器缓存、Crashpad/completed 等。
        // 这些不在 ~/Library/Caches 下，上面的展开够不到。
        push_browser_app_support_caches(t, &app_support);

        // Firefox Profile 缓存
        push_firefox_profile_caches(t, &app_support);

        // Mail Downloads 中的附件是用户主动打开或保存过的文件，不是缓存，
        // 不能进入智能清理候选。
    }
}

/// Chromium 系浏览器的缓存目标（Windows）。
///
/// 全量覆盖 Default 及所有 Profile 1, Profile 2 ... 配置文件。
#[cfg(windows)]
pub(super) fn push_chromium_browser_targets(
    t: &mut Vec<ScanTarget>,
    user_data_dir: &std::path::Path,
    name_zh: &str,
    name_en: &str,
) {
    if !user_data_dir.exists() {
        return;
    }
    // 1. 常规默认 profile
    let default_cache = user_data_dir.join("Default\\Cache");
    let default_code_cache = user_data_dir.join("Default\\Code Cache");
    t.push(target(
        default_cache,
        Text::new(format!("{name_zh} 缓存"), format!("{name_en} cache")),
        CategoryId::BrowserCache,
    ));
    t.push(target(
        default_code_cache,
        format!("{name_en} Code Cache"),
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
                            format!("{name_zh} 缓存 ({name})"),
                            format!("{name_en} cache ({name})"),
                        ),
                        CategoryId::BrowserCache,
                    ));
                    t.push(target(
                        code_cache,
                        format!("{name_en} Code Cache ({name})"),
                        CategoryId::BrowserCache,
                    ));
                }
            }
        }
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
/// 通用的 Chromium 缓存叶子识别在 `categories::chromium`；浏览器单独留一条
/// 是因为这里多一步「`Crashpad/completed`」的细分——只收已完成的崩溃报告，
/// 不收可能正在写的 `pending/`。
#[cfg(target_os = "macos")]
pub(super) fn push_browser_app_support_caches(t: &mut Vec<ScanTarget>, app_support: &Path) {
    // 每个 profile 下的缓存子目录
    let cache_subdirs: &[&str] = &[
        "Code Cache",
        "GPUCache",
        "DawnCache",
        "GrShaderCache",
        "GraphiteDawnCache",
        "ShaderCache",
    ];

    for row in browser_catalog("app_support") {
        let name_zh = &row.zh;
        let name_en = &row.en;
        let root = app_support.join(&row.path);
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
                            format!("{name_zh} · {profile_name} · {sub}"),
                            format!("{name_en} · {profile_name} · {sub}"),
                        ),
                        CategoryId::BrowserCache,
                    ));
                }
            }

            // Service Worker/CacheStorage 可能承载网站离线数据，不能当作普通
            // HTTP/代码缓存清理。
        }

        // Crashpad 已完成的崩溃报告
        let crashpad = root.join("Crashpad/completed");
        if crashpad.is_dir() {
            t.push(target(
                crashpad,
                Text::new(
                    format!("{name_zh} · Crashpad"),
                    format!("{name_en} · Crashpad"),
                ),
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
pub(super) fn push_firefox_profile_caches(t: &mut Vec<ScanTarget>, app_support: &Path) {
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

#[cfg(test)]
mod tests {
    /// 浏览器根目录不存在时不产出目标。Profile 枚举只发生在目录真的在的时候。
    #[test]
    #[cfg(windows)]
    fn missing_browser_user_data_emits_nothing() {
        let root = crate::core::testing::fixture("qc_missing_browser");
        let missing = root.join("NoSuchBrowser/User Data");
        let mut targets = Vec::new();
        super::push_chromium_browser_targets(&mut targets, &missing, "Chrome", "Chrome");
        assert!(targets.is_empty(), "{targets:?}");
        let _ = std::fs::remove_dir_all(root);
    }
}
