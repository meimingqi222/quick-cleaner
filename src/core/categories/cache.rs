//! 用户缓存、包管理缓存、缩略图缓存

use super::{target, target_with_recommendation, ScanTarget};
use crate::core::categories::CategoryId;
use crate::core::i18n::Text;
use std::path::Path;

/// 包管理缓存、用户缓存、缩略图缓存
pub(super) fn push_cache_targets(t: &mut Vec<ScanTarget>, home: &Path) {
    push_package_cache_targets(t, home);
    push_user_cache_targets(t, home);
    push_thumbnail_targets(t, home);
}

/// 包管理器缓存（npm / pnpm / cargo / go / pip 等）
fn push_package_cache_targets(t: &mut Vec<ScanTarget>, home: &Path) {
    #[cfg(windows)]
    {
        let local = crate::platform::windows::real_user_local_appdata();

        t.push(target(
            local.join("npm-cache"),
            Text::new("npm 缓存", "npm cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join("npm-cache"),
            Text::new("npm 缓存 (home)", "npm cache (home)"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".pnpm-store"),
            "pnpm store",
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".pnpm-cache"),
            "pnpm cache",
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".cargo\\registry"),
            Text::new("cargo registry 缓存", "cargo registry cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".rustup\\downloads"),
            Text::new("rustup 下载缓存", "rustup downloads"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join("go\\pkg\\mod"),
            Text::new("go module 缓存", "go module cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            local.join("go-build"),
            Text::new("go build 缓存", "go build cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".bun\\install\\cache"),
            Text::new("bun 缓存", "bun cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".gradle\\caches"),
            Text::new("gradle 缓存", "gradle cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".nuget\\packages"),
            Text::new("nuget 包缓存", "nuget package cache"),
            CategoryId::PackageCache,
        ));
        // ~/.m2/repository 可能包含仅在本机 mvn install 的私有构件，不能
        // 当作可重新下载的缓存推荐清理。
        // ~/.cache 里既有纯下载缓存，也可能有工具状态。按顶层子目录拆开，
        // 才能只推荐已确认可重建的 OpenCode 缓存，同时保留其他项目供审阅。
        push_home_cache_targets(t, home);
        t.push(target(
            local.join("uv\\cache"),
            Text::new("uv 缓存", "uv cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            local.join("pip\\cache"),
            Text::new("pip 缓存", "pip cache"),
            CategoryId::PackageCache,
        ));
    }

    #[cfg(target_os = "macos")]
    {
        let cache = home.join("Library/Caches");

        t.push(target(
            home.join(".npm/_cacache"),
            Text::new("npm 缓存", "npm cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".cargo/registry"),
            Text::new("cargo 缓存", "cargo cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join(".rustup/downloads"),
            Text::new("rustup 缓存", "rustup cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join("go/pkg/mod"),
            Text::new("go 缓存", "go cache"),
            CategoryId::PackageCache,
        ));
        push_home_cache_targets(t, home);
        t.push(target(
            home.join("Library/Caches/Homebrew"),
            Text::new("Homebrew 缓存", "Homebrew cache"),
            CategoryId::PackageCache,
        ));
        for (name, zh, en) in [
            ("bun", "Bun 缓存", "Bun cache"),
            ("go-build", "Go 构建缓存", "Go build cache"),
            ("go", "Go 工具缓存", "Go tool cache"),
            ("gopls", "gopls 缓存", "gopls cache"),
            ("goimports", "goimports 缓存", "goimports cache"),
            ("node-gyp", "node-gyp 缓存", "node-gyp cache"),
            ("pip", "pip 缓存", "pip cache"),
            ("typescript", "TypeScript 缓存", "TypeScript cache"),
        ] {
            t.push(target(
                cache.join(name),
                Text::new(zh, en),
                CategoryId::PackageCache,
            ));
        }

        // 包管理器补充
        t.push(target(
            home.join(".gradle/caches"),
            Text::new("Gradle 缓存", "Gradle cache"),
            CategoryId::PackageCache,
        ));
        t.push(target(
            home.join("Library/pnpm/store"),
            Text::new("pnpm store", "pnpm store"),
            CategoryId::PackageCache,
        ));
    }
}

/// 用户缓存（`~/Library/Caches` 展开等）
fn push_user_cache_targets(t: &mut Vec<ScanTarget>, home: &Path) {
    #[cfg(windows)]
    {
        let local = crate::platform::windows::real_user_local_appdata();
        // 缩略图
        t.push(target(
            local.join("Microsoft\\Windows\\Explorer"),
            Text::new("缩略图/图标缓存", "Thumbnail / icon cache"),
            CategoryId::Thumbnails,
        ));
    }

    #[cfg(target_os = "macos")]
    {
        let cache = home.join("Library/Caches");

        // `~/Library/Caches` 剩下的部分。
        //
        // 这里逐个展开顶层子目录，而不是把整个 `~/Library/Caches` 作为一个目标：
        // 它和上面的浏览器 / Homebrew 缓存是父子关系，而 `scanner` 不做嵌套去重
        // （`scan_fixed_inner` 逐目标独立称重后直接相加），父子同时入表会让总量
        // 凭空翻倍。展开后顺带能按目录名给出标签，比一个不透明的大块更有用。
        push_user_cache_dirs(t, &cache);
    }
}

/// 缩略图缓存
fn push_thumbnail_targets(t: &mut Vec<ScanTarget>, home: &Path) {
    #[cfg(target_os = "macos")]
    {
        // QuickLook 缩略图缓存。
        //
        // macOS 15 上经典的 `com.apple.QuickLook.thumbnailcache` 已不存在，
        // 改为散落在 `$TMPDIR` 同级的 `C` 目录下的多个 `com.apple.quicklook.*`
        // 子目录。`$TMPDIR`（即 `/var/folders/<hash>/T`）下也有几个。
        // `<hash>` 是 per-user 的，编译期不知道，必须运行时从 `temp_dir()` 推。
        let _ = home;
        push_quicklook_thumbnail_targets(t);
    }
}

/// 展开 `~/.cache`，避免一个宽泛父目录掩盖子项目的不同安全级别。
pub(super) fn push_home_cache_targets(t: &mut Vec<ScanTarget>, home: &Path) {
    let root = home.join(".cache");
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "opencode" {
            t.push(target_with_recommendation(
                entry.path(),
                Text::new("OpenCode · 缓存", "OpenCode · cache"),
                CategoryId::AiAgents,
                true,
            ));
        } else {
            t.push(target_with_recommendation(
                entry.path(),
                format!("~/.cache/{name}"),
                CategoryId::UserTemp,
                false,
            ));
        }
    }
}

/// `~/Library/Caches` 下已被更具体的目标认领的顶层目录。
///
/// 这些名字必须和上面 BrowserCache / PackageCache / AiAgents 目标里用的
/// 顶层段一致，否则会和 `push_user_cache_dirs` 重复计数。
/// `Google` 覆盖 `Google/Chrome`；`claude-cli-nodejs` / `Zed` 等覆盖
/// `LOCAL_AGENT_DIRS` 里对应的条目。
#[cfg(target_os = "macos")]
const CLAIMED_USER_CACHE_DIRS: &[&str] = &[
    "Google",
    "com.apple.Safari",
    "Microsoft Edge",
    "Homebrew",
    // ---- 浏览器缓存，已显式标为 BrowserCache ----
    "Firefox",
    "BraveSoftware",
    "company.thebrowser.Browser",
    "Chromium",
    "com.operasoftware.Opera",
    "com.vivaldi.Vivaldi",
    // ---- 明确可重建的包管理/编译缓存 ----
    "bun",
    "go-build",
    "go",
    "gopls",
    "goimports",
    "node-gyp",
    "pip",
    "typescript",
    // ---- LOCAL_AGENT_DIRS 里的目录名，避免与 AiAgents 双算 ----
    "claude-cli-nodejs",
    "amp",
    "Zed",
    "WorkBuddy",
    "cursor-updater",
    "antigravity-updater",
    "@genieworkbuddy-desktop-updater",
    "@makadesktop-updater",
    "@zcodedesktop-updater",
    "adspower_global-updater",
];

/// 把 `~/Library/Caches` 的顶层子目录逐个加为清理目标，跳过已被认领的。
#[cfg(target_os = "macos")]
pub(super) fn push_user_cache_dirs(t: &mut Vec<ScanTarget>, cache: &Path) {
    let Ok(rd) = std::fs::read_dir(cache) else {
        return;
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if CLAIMED_USER_CACHE_DIRS.contains(&name.as_str()) {
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
        // `~/Library/Caches` 是约定上的缓存位置，但第三方软件并不总遵守：
        // JetBrains 在这里放 LocalHistory/fileHistory，ms-playwright 也可能放
        // 带登录态的 MCP 浏览器 Profile。未知目录只展示，不能默认勾选。
        t.push(target_with_recommendation(
            entry.path(),
            format!("~/Library/Caches/{name}"),
            CategoryId::UserTemp,
            false,
        ));
    }
}

/// 发现 QuickLook 缩略图缓存目录并加为清理目标。
///
/// macOS 15 上缩略图缓存散落在 per-user 的 `$TMPDIR`（`/var/folders/<hash>/T`）
/// 及其同级 `C` 目录下的多个 `com.apple.quicklook.*` 子目录里。经典路径
/// `com.apple.QuickLook.thumbnailcache` 已不存在。`<hash>` 编译期不知道，
/// 用 `std::env::temp_dir()` 推：它返回 `/var/folders/<hash>/T`，
/// `parent()` 得到 `/var/folders/<hash>`，再拼 `C` 得到缓存目录。
#[cfg(target_os = "macos")]
fn push_quicklook_thumbnail_targets(t: &mut Vec<ScanTarget>) {
    let tmpdir = std::env::temp_dir();
    let Some(user_cache_root) = tmpdir.parent() else {
        return;
    };
    // 两个目录都扫：T（临时）和 C（缓存），QuickLook 在两边都写
    for root in [user_cache_root.join("C"), user_cache_root.join("T")] {
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // 只收 QuickLook 相关的目录，其余的 per-user 缓存不归这一类
            if !name.starts_with("com.apple.quicklook") {
                continue;
            }
            if !entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                continue;
            }
            t.push(target(
                entry.path(),
                Text::new(
                    format!("QuickLook 缓存 · {name}"),
                    format!("QuickLook cache · {name}"),
                ),
                CategoryId::Thumbnails,
            ));
        }
    }
}
