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
        // GOPRIVATE / 自建 proxy 拉下来的私有模块也落在这里，内网机器上删了
        // 可能没有上游可拉——与下面 `~/.m2/repository` 同一条判据（规范第 2 条），
        // 类别照旧是包缓存，只是不预选。
        t.push(target_with_recommendation(
            home.join("go\\pkg\\mod"),
            Text::new("go module 缓存", "go module cache"),
            CategoryId::PackageCache,
            false,
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
        // 同 go module 缓存：企业私有仓的构件可能只有本机有过。
        t.push(target_with_recommendation(
            home.join(".gradle\\caches"),
            Text::new("gradle 缓存", "gradle cache"),
            CategoryId::PackageCache,
            false,
        ));
        // 私有 feed（Azure Artifacts、ProGet、内部 NuGet 源）下的包常常只有
        // 本机有过——同 go module / gradle 的判据。
        t.push(target_with_recommendation(
            home.join(".nuget\\packages"),
            Text::new("nuget 包缓存", "nuget package cache"),
            CategoryId::PackageCache,
            false,
        ));
        // 包缓存这条线的分界（规范第 2 条）：**这个目录是公共 registry 的本机
        // 镜像，还是本机某份产物的唯一副本？**
        // - 只是镜像：npm `_cacache`、pip、uv、cargo registry、bun、rustup、
        //   Homebrew、typescript、node-gyp。删了最坏是重新下载，可以预选。
        // - 常是唯一副本：`~/.m2/repository`（`mvn install` 的私有构件直接写
        //   进这个目录）、go module 缓存、`~/.gradle/caches`、`~/.nuget/packages`
        //   ——这几个生态里「只在内网有、上游根本没有」是日常而不是例外，不预选。
        //   npm 私服和私有 cargo registry 也能构造出同样情形，所以这条界线是按
        //   普遍性下的判断，不是机械推导出来的；要挪动谁，得给出新的证据。
        // ~/.cache 里既有纯下载缓存，也可能有工具状态。按顶层子目录拆开，
        // 才能只推荐已确认可重建的缓存，同时保留其他项目供审阅。
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
        // 私有模块的唯一副本，同 Windows 分支：不预选。
        t.push(target_with_recommendation(
            home.join("go/pkg/mod"),
            Text::new("go 缓存", "go cache"),
            CategoryId::PackageCache,
            false,
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

        // 包管理缓存补充
        // 私有仓构件的唯一副本，同 Windows 分支：不预选。
        t.push(target_with_recommendation(
            home.join(".gradle/caches"),
            Text::new("Gradle 缓存", "Gradle cache"),
            CategoryId::PackageCache,
            false,
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
    let _ = home;
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
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (t, home);
    }
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
        } else if let Some((_, zh, en)) = REBUILDABLE_HOME_CACHE_DIRS
            .iter()
            .find(|(key, _, _)| *key == name)
        {
            t.push(target(
                entry.path(),
                Text::new(*zh, *en),
                CategoryId::PackageCache,
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

/// 按 XDG 约定落在 `~/.cache` 下、能重新下载或就地重建的缓存。
///
/// 判据和 `~/Library/Caches` 里那张表一致：工具自带 `cache clean` /
/// `cache purge` 一类子命令，或删掉后下一次构建自己长回来。表外的
/// 目录仍然整项展示，只是不默认勾选。
const REBUILDABLE_HOME_CACHE_DIRS: &[(&str, &str, &str)] = &[
    ("uv", "uv 缓存", "uv cache"),
    ("pip", "pip 缓存", "pip cache"),
    ("pypoetry", "Poetry 缓存", "Poetry cache"),
    ("pre-commit", "pre-commit 缓存", "pre-commit cache"),
    ("node-gyp", "node-gyp 缓存", "node-gyp cache"),
    ("typescript", "TypeScript 缓存", "TypeScript cache"),
    ("go-build", "Go 构建缓存", "Go build cache"),
    ("gopls", "gopls 缓存", "gopls cache"),
];

/// `~/Library/Caches` 下已被**整目录**认领的顶层目录。
///
/// 这些名字必须和上面 BrowserCache / PackageCache 目标里用的顶层段一致：
/// `scan_fixed_inner` 逐目标独立称重后相加、不做嵌套去重，父子同时入表会让
/// 展示体积凭空翻倍。
///
/// 只登记「整个目录就是一个目标」的名字。目标只是某几个孩子的，进下面那张
/// `PARTIALLY_CLAIMED_USER_CACHE_DIRS`——在这里整目录跳过，会让没被认领的
/// 兄弟子项既不进表也不展示。
#[cfg(target_os = "macos")]
const CLAIMED_USER_CACHE_DIRS: &[&str] = &[
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
];

/// 顶层目录里只有部分子项是目标：`(父目录, 已入表的孩子)`。
///
/// 父目录仍然不能入表（会和孩子的体积双算），但它的**其余孩子必须入表**——
/// 否则那些子项在界面上彻底隐身，用户既看不见也清不掉。`Google` 是最典型的
/// 一个：`browser.rs` 只认领 `Google/Chrome`，而 GoogleUpdater 的下载目录就
/// 躺在它的兄弟位置上。
#[cfg(target_os = "macos")]
const PARTIALLY_CLAIMED_USER_CACHE_DIRS: &[(&str, &[&str])] = &[
    ("Google", &["Chrome"]),
    ("claude-cli-nodejs", &["Cache"]),
    ("amp", &["logs", "traces"]),
    ("Zed", &["logs", "hang_traces"]),
    ("WorkBuddy", &["logs"]),
];

/// 把 `~/Library/Caches` 的顶层子目录逐个加为清理目标，跳过已被认领的。
///
/// 分成三步判定，而不是一律丢进同一个桶：
/// 1. 内容命中更新包签名 → 按子项拆开，更新包叶子可默认勾选；
/// 2. 没命中 → 整目录作为「分不清」的一项展示，不预选；
/// 3. `com.apple.*` → 不做探测，见下方说明。
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
        let dir = entry.path();
        // 该父目录里已由更具体规则认领走的孩子（普通目录为空）。父目录不能
        // 入表（会和孩子的体积双算），但其余孩子必须入表，否则它们隐身。
        let claimed = PARTIALLY_CLAIMED_USER_CACHE_DIRS
            .iter()
            .find(|(parent, _)| *parent == name)
            .map(|(_, kids)| *kids)
            .unwrap_or_default();
        let stem = super::updater::display_stem(&name);
        // 签名判定是按第三方更新器的产物形态做的，对 Apple 守护进程的目录
        // 没有意义：上面那张敏感表只列了确认危险的，其余 `com.apple.*` 并不
        // 因此安全，所以一律不探测、只展示。
        let hit = !name.starts_with("com.apple.")
            && super::updater::push_updater_artifacts(t, &dir, &stem);
        if hit || !claimed.is_empty() {
            push_residual_children(t, &dir, &name, claimed);
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
        ));
    }
}

/// 父目录没能入表时，把它的顶层子项逐个补进目标表：形态仍然分不清，只展示、
/// 不默认勾选。`skip` 是已经由别的规则认领走的孩子，不能重复入表。
///
/// 两种拆分共用这条路：内容命中更新包签名（`updater.rs`），以及父目录只被
/// 部分认领（`PARTIALLY_CLAIMED_USER_CACHE_DIRS`）。共同前提是父目录本身不能
/// 入表——`scan_fixed_inner` 逐目标独立称重后相加、不做嵌套去重，父子同时入表
/// 会让体积翻倍。但**兄弟不该为父目录的缺席陪葬**：`Cache.db` 这类占着目录里
/// 最大一块体积的东西，不列出来就等于从界面上消失。
#[cfg(target_os = "macos")]
fn push_residual_children(t: &mut Vec<ScanTarget>, dir: &Path, name: &str, skip: &[&str]) {
    for child in super::updater::residual_children(dir, skip) {
        t.push(target_with_recommendation(
            dir.join(&child),
            format!("~/Library/Caches/{name}/{child}"),
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
