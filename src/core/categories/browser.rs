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

        // 浏览器缓存（全量覆盖 Default 及所有 Profile 1, Profile 2 ... 配置文件）
        push_chromium_browser_targets(t, &local.join("Google\\Chrome\\User Data"), "Chrome");
        push_chromium_browser_targets(t, &local.join("Microsoft\\Edge\\User Data"), "Edge");
        push_chromium_browser_targets(
            t,
            &local.join("BraveSoftware\\Brave-Browser\\User Data"),
            "Brave",
        );
    }

    #[cfg(target_os = "macos")]
    {
        let cache = home.join("Library/Caches");
        let app_support = home.join("Library/Application Support");

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

        // §补充：参考 Mole 项目完善 macOS 清理目标

        // 更多浏览器缓存（~/Library/Caches 下的产品目录）
        // push_user_cache_dirs 已展开 ~/Library/Caches 下的所有子目录，
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

/// Chromium 系浏览器在 `~/Library/Application Support` 下的缓存子目录。
///
/// Chrome / Arc / Brave / Edge 等都基于 Chromium，缓存布局一致：
/// `<UserDataDir>/<Profile>/Code Cache`、`GPUCache`、着色器缓存等。
/// 这些不在 `~/Library/Caches` 下，需要单独发现。
#[cfg(target_os = "macos")]
pub(super) fn push_browser_app_support_caches(t: &mut Vec<ScanTarget>, app_support: &Path) {
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

            // Service Worker/CacheStorage 可能承载网站离线数据，不能当作普通
            // HTTP/代码缓存清理。
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
