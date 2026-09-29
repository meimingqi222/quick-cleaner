//! Chromium / Electron 应用的缓存叶子识别
//!
//! 这一模块替代了「按应用名列一张 Electron 应用表」的老做法。那张表的教训
//! 写在 `dev.rs` 的注释里也写在 `PITFALLS`：本机实测 21 个名字只命中很少
//! 一部分，而 `~/Library/Application Support` 下真实存在 Chromium 缓存叶子
//! 的 41 个应用里，Codex、CatPawAI、Qoder、MiniMax、Quark、Xiaomi MiMo、
//! Grok Bot、Maka 等等一个都没被列到——**按名字认应用永远追不上新应用**。
//!
//! 换成的判据是**内容签名**：一个目录里同时出现两个以上只有 Chromium 内核
//! 才会生成的缓存子目录名（`Cache` + `Code Cache` + `GPUCache` …），它就是
//! 一个 Chromium 应用的 userData 目录或其 profile 目录。这时才去列**叶子**，
//! 而且只列叶子：
//!
//! - userData 根（`<App>/{Cache,Code Cache,...}`，Electron 的布局）；
//! - profile 目录（`<App>/Default/{Cache,...}`、`<App>/chrome-profile/...`，
//!   Chrome/Chromium 的布局）。
//!
//! **根目录本身永远不进目标表**。`~/.cache/chrome-devtools-mcp` 整个是
//! 227 MB，其中可重建的缓存只有 85 MB，剩下的是那个浏览器 Profile 本体
//! （Cookies / Login Data / DIPS / IndexedDB）——整目录入表等于拿别人的
//! 登录态换 140 MB，这不是清理。`Service Worker`、`IndexedDB`、
//! `Local Storage`、`Session Storage` 同理，一律不在叶子表里（有测试钉着）。

use std::path::{Path, PathBuf};

/// 只有 Chromium 内核才生成的缓存子目录名：凑够 [`SIGNATURE_MIN`] 个就认定
/// 这是 Chromium 的数据目录。
///
/// `Cache` 单独一个不算——太多应用把「缓存」叫 `Cache`，`<App>/Cache` +
/// `<App>/logs` 这种巧合必须挡在门外，否则普通应用的状态目录会被当缓存清。
pub(super) const SIGNATURE_LEAVES: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "GrShaderCache",
    "GraphiteDawnCache",
    "ShaderCache",
    "component_crx_cache",
    "extensions_crx_cache",
];

/// 签名成立之后才一起收的叶子：这些名字太通用，不能单独用来判定 Chromium，
/// 但确认了是 Chromium 之后，它们装的确实是可重建数据。
///
/// `blob_storage` 也在这里，但它**不预选**（见 [`leaf_recommended`]）——
/// Electron 的 blob 存储可能承载未保存的附件或草稿。
pub(super) const EXTRA_LEAVES: &[&str] = &[
    "CachedData",
    "CachedProfilesData",
    "CachedExtensionVSIXs",
    "blob_storage",
    "fcache",
    "logs",
];

/// 名字本身就只可能来自 Chromium 内核（或 VS Code 这类 Electron 骨架）的
/// 叶子：单独一个就足以认定它的宿主是应用数据目录。
///
/// 本机实测的用途：`~/Library/Application Support/Code/CachedExtensionVSIXs`
/// 单项 1 GB，而 `Code` 目录下恰好只有「扩展包缓存 + 日志 + 崩溃转储」——
/// 一个签名叶子都没有，靠「两个通用叶子」的门槛它整目录进不来。
/// `component_crx_cache` / `extensions_crx_cache` 同理。
pub(super) const STRONG_LEAVES: &[&str] = &[
    "component_crx_cache",
    "extensions_crx_cache",
    "CachedExtensionVSIXs",
];

/// 认定「这是 Chromium 数据目录」需要的签名叶子个数。
///
/// 取 2：单个签名叶子名（`Cache`、`GPUCache`）在别的软件里也见过，两个同时
/// 出现基本只有 Chromium 内核干得出来；门限再抬高（3）会在精简过的 profile
/// 上漏判（本机 `Maka` 只有 `Code Cache`+`GPUCache`+`DawnGraphiteCache`）。
/// [`STRONG_LEAVES`] 里的名字不受这个门限约束。
const SIGNATURE_MIN: usize = 2;

/// Profile 目录名。`Default` / `Profile 1` 是 Chrome 系；`chrome-profile`、
/// `<名字>-profile` 是 MCP server 一类工具自建的原生 profile 目录。
fn is_profile_name(name: &str) -> bool {
    name == "Default" || name.starts_with("Profile ") || name.ends_with("-profile")
}

/// 叶子相对其宿主目录的路径段，用来拼双语标签。
///
/// `Cache` → `["Cache"]`；`Default/GPUCache` → `["Default", "GPUCache"]`。
/// 不带上中间段的话，`<App>/GPUCache` 与 `<App>/Default/GPUCache` 在界面上
/// 就是两行同名条目。
pub(super) fn leaf_trail(dir: &Path, leaf: &Path) -> Vec<String> {
    let relative = leaf.strip_prefix(dir).unwrap_or(leaf);
    relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect()
}

/// 这个叶子默认勾不勾。
///
/// 两条例外，其余缓存叶子都默认勾选：
/// - `CachedProfilesData` 可能保存本地唯一的编辑器 Profile；
/// - `blob_storage` 可能承载未保存的附件或草稿。
pub(super) fn leaf_recommended(name: &str) -> bool {
    !matches!(name, "CachedProfilesData" | "blob_storage")
}

/// `dir` 底下可以直接清掉的缓存叶子。不是 Chromium 数据目录时返回空表。
///
/// 三种布局都认：
/// - `dir` 自己是 userData 根 → 收它自己的叶子（Electron 布局）；
/// - `dir` 下的 profile 目录 → 收 profile 下的叶子（`<App>/Default/...`）；
/// - `dir` 下 `<名字>-profile` 容器里的 profile（`chrome-profile/Default/...`，
///   MCP server 一类工具自建的布局）。
///
/// 空目录不收：一个 0 字节的 `GPUCache` 列出来只会把界面撑长，用户点它
/// 什么也得不到。这是本模块唯一「看不见」的东西，代价为零。
pub(super) fn cache_leaves(dir: &Path) -> Vec<PathBuf> {
    let mut leaves = Vec::new();
    let root_confirmed = is_chromium_dir(dir);
    if root_confirmed {
        collect_leaves(dir, &mut leaves);
    }

    // Chrome 的布局里根目录自己一个缓存叶子都没有（全在 profile 下），所以
    // 根没确认并不能否决 profile 这一轮。反过来，根已经确认的场景下 profile
    // 的门槛可以降到 1：本机 `Doubao/Default` 只有一个 `GPUCache`，而它的根上
    // 有 `component_crx_cache` + `extensions_crx_cache`，归属毫无疑义。
    for profile in profile_dirs(dir) {
        let confirmed =
            is_chromium_dir(&profile) || (root_confirmed && count_signature_leaves(&profile) >= 1);
        if confirmed {
            collect_leaves(&profile, &mut leaves);
        }
    }

    leaves
}

/// 这个目录看起来是不是 Chromium 的数据目录。
fn is_chromium_dir(dir: &Path) -> bool {
    STRONG_LEAVES.iter().any(|leaf| dir.join(leaf).is_dir())
        || count_signature_leaves(dir) >= SIGNATURE_MIN
}

/// `dir` 下的 profile 目录。
///
/// 两层：`<App>/Default`、`<App>/Profile 1` 是一层；
/// `~/.cache/<tool>/chrome-profile/Default` 要多下一层——`chrome-profile`
/// 本身不是 profile，它是装着 profile 的容器。
fn profile_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut profiles = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return profiles;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        if is_profile_name(&name) {
            profiles.push(entry.path());
            // 不 `continue`：`chrome-profile` 既是 profile 名的形状，也可能只是
            // 装着 profile 的容器（`chrome-profile/Default/...`），两种都得看。
        }
        if !name.ends_with("-profile") {
            continue;
        }
        if let Ok(inner) = std::fs::read_dir(entry.path()) {
            for child in inner.flatten() {
                let child_name = child.file_name().to_string_lossy().into_owned();
                if is_profile_name(&child_name) && child.file_type().is_ok_and(|kind| kind.is_dir())
                {
                    profiles.push(child.path());
                }
            }
        }
    }
    profiles
}

fn count_signature_leaves(dir: &Path) -> usize {
    SIGNATURE_LEAVES
        .iter()
        .filter(|leaf| dir.join(leaf).is_dir())
        .count()
}

fn collect_leaves(dir: &Path, out: &mut Vec<PathBuf>) {
    for name in SIGNATURE_LEAVES.iter().chain(EXTRA_LEAVES) {
        let path = dir.join(name);
        if !path.is_dir() || !has_content(&path) {
            continue;
        }
        out.push(path);
    }
    // Crashpad/pending 可能仍在写或尚未上报；只收已完成的报告。
    let completed = dir.join("Crashpad/completed");
    if completed.is_dir() && has_content(&completed) {
        out.push(completed);
    }
}

/// 目录里有没有东西。只读一层：缓存叶子都是平铺的大文件，不会藏在多层
/// 子目录里，多读几层只是多花时间。
fn has_content(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch_dir(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        std::fs::write(path.join("data"), b"x").unwrap();
    }

    fn leaf_names(head: &Path) -> Vec<String> {
        cache_leaves(head)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn single_generic_leaf_is_not_a_signature() {
        let root = crate::core::testing::fixture("qc_chromium_signature");
        let app = root.join("SomeApp");
        // `Cache` + `logs` 是任何应用都可能有的组合，不能因此认定是 Chromium。
        touch_dir(&app.join("Cache"));
        touch_dir(&app.join("logs"));
        assert!(cache_leaves(&app).is_empty(), "单个通用叶子不该触发识别");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn electron_user_data_leaves_are_collected_but_not_the_root() {
        let root = crate::core::testing::fixture("qc_chromium_electron");
        let app = root.join("Codex");
        for leaf in ["Cache", "Code Cache", "GPUCache", "DawnGraphiteCache"] {
            touch_dir(&app.join(leaf));
        }
        // 空叶子不收
        std::fs::create_dir_all(app.join("Crashpad")).unwrap();
        // 会话状态永远不碰
        touch_dir(&app.join("Local Storage"));

        let names = leaf_names(&app);
        for expected in ["Cache", "Code Cache", "GPUCache", "DawnGraphiteCache"] {
            assert!(names.contains(&expected.to_string()), "{names:?}");
        }
        assert!(
            !names.contains(&"Crashpad".to_string()),
            "空目录不收: {names:?}"
        );
        assert!(!names.contains(&"Local Storage".to_string()), "{names:?}");
        assert!(
            cache_leaves(&app).iter().all(|p| p != &app),
            "根目录本身不能入表"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn profile_layout_collects_profile_leaves_only() {
        let root = crate::core::testing::fixture("qc_chromium_profile");
        // `~/.cache/chrome-devtools-mcp/chrome-profile/Default/...` 的形状
        let tool = root.join("chrome-devtools-mcp");
        let profile = tool.join("chrome-profile/Default");
        for leaf in ["Cache", "Code Cache", "GPUCache"] {
            touch_dir(&profile.join(leaf));
        }
        touch_dir(&profile.join("Cookies"));
        touch_dir(&tool.join("chrome-profile/IndexedDB"));

        let leaves = cache_leaves(&tool);
        let joined: Vec<String> = leaves.iter().map(|p| p.display().to_string()).collect();
        assert_eq!(leaves.len(), 3, "{joined:?}");
        assert!(joined.iter().all(|p| p.contains("/Default/")), "{joined:?}");
        assert!(
            !joined.iter().any(|p| p.ends_with("chrome-profile")),
            "profile 根不能入表: {joined:?}"
        );
        assert!(
            !joined.iter().any(|p| p.contains("IndexedDB")),
            "{joined:?}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// 根上有 crx 缓存、profile 里只有一个 `GPUCache` 的应用也要收到
    /// （本机 `Doubao` 就是这一形状）。
    #[test]
    fn confirmed_root_lowers_the_profile_bar() {
        let root = crate::core::testing::fixture("qc_chromium_lowered_bar");
        let app = root.join("Doubao");
        touch_dir(&app.join("component_crx_cache"));
        touch_dir(&app.join("extensions_crx_cache"));
        touch_dir(&app.join("Default/GPUCache"));

        let joined: Vec<String> = cache_leaves(&app)
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        assert_eq!(joined.len(), 3, "{joined:?}");
        assert!(
            joined.iter().any(|p| p.ends_with("Default/GPUCache")),
            "{joined:?}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// 只可能来自 Chromium/Electron 骨架的叶子单独一个就够用。
    ///
    /// 本机 `~/Library/Application Support/Code` 只有
    /// `CachedExtensionVSIXs`（1 GB）+ `Crashpad` + `logs`，一个通用签名叶子
    /// 都没有；不给这类名字开一条捷径，整目录的扩展包缓存就进不了表。
    #[test]
    fn strong_leaf_confirms_a_directory_on_its_own() {
        let root = crate::core::testing::fixture("qc_chromium_strong");
        let app = root.join("Code");
        touch_dir(&app.join("CachedExtensionVSIXs"));
        touch_dir(&app.join("logs"));

        let names = leaf_names(&app);
        assert!(
            names.contains(&"CachedExtensionVSIXs".to_string()),
            "{names:?}"
        );
        assert!(names.contains(&"logs".to_string()), "{names:?}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn session_state_and_drafts_are_never_listed() {
        for name in [
            "Service Worker",
            "IndexedDB",
            "Local Storage",
            "Session Storage",
        ] {
            assert!(!SIGNATURE_LEAVES.contains(&name), "{name} 存登录态");
            assert!(!EXTRA_LEAVES.contains(&name), "{name} 存登录态");
        }
        assert!(!leaf_recommended("CachedProfilesData"));
        assert!(!leaf_recommended("blob_storage"));
        assert!(leaf_recommended("Cache"));
        assert!(leaf_recommended("Code Cache"));
    }

    #[test]
    fn crashpad_only_lists_completed_reports() {
        let root = crate::core::testing::fixture("qc_chromium_crashpad");
        let app = root.join("App");
        for leaf in [
            "Cache",
            "GPUCache",
            "Crashpad/completed",
            "Crashpad/pending",
            "CrashReport/pending",
        ] {
            touch_dir(&app.join(leaf));
        }
        let leaves = cache_leaves(&app);
        assert!(leaves.contains(&app.join("Crashpad/completed")));
        assert!(!leaves.contains(&app.join("Crashpad")));
        assert!(!leaves.contains(&app.join("Crashpad/pending")));
        assert!(!leaves.contains(&app.join("CrashReport")));
        let _ = std::fs::remove_dir_all(root);
    }
}
