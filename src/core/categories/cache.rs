//! 用户缓存、包管理缓存、缩略图缓存

use super::{target, target_with_recommendation, ScanTarget};
use crate::core::categories::CategoryId;
use crate::core::i18n::Text;
use std::path::Path;

/// 包管理缓存、用户缓存、缩略图缓存。
///
/// `home` 为 None 时跳过用户级缓存；QuickLook 缩略图从 `$TMPDIR` 推路径，
/// 不依赖 home，仍然加入。
pub(super) fn push_cache_targets(
    t: &mut Vec<ScanTarget>,
    home: Option<&Path>,
    brew_cleanup_at: Option<i64>,
) {
    if let Some(home) = home {
        push_package_cache_targets(t, home, brew_cleanup_at);
        push_user_cache_targets(t, home);
        push_thumbnail_targets(t, home);
    } else {
        #[cfg(target_os = "macos")]
        push_quicklook_thumbnail_targets(t);
    }
}

/// 包管理器缓存（npm / pnpm / cargo / go / pip 等）
fn push_package_cache_targets(t: &mut Vec<ScanTarget>, home: &Path, brew_cleanup_at: Option<i64>) {
    #[cfg(windows)]
    {
        // brew 只在 macOS 分支用得上。Windows 上不消费一下，clippy 的
        // `-D warnings` 会把 CI 卡在「未使用的参数」上。
        let _ = brew_cleanup_at;
        let Some(local) = crate::platform::user_cache_dir() else {
            return;
        };

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
        // 不切到 `registry\cache`：实测删掉它之后，`registry\src` 里带
        // `.cargo-ok` 的解包树仍然救不了 `cargo build --offline`。判据同
        // macOS 分支。
        t.push(target_with_recommendation(
            home.join(".cargo\\registry"),
            Text::new("cargo registry 缓存", "cargo registry cache"),
            CategoryId::PackageCache,
            false,
        ));
        t.push(target(
            home.join(".rustup\\downloads"),
            Text::new("rustup 下载缓存", "rustup downloads"),
            CategoryId::PackageCache,
        ));
        // 不切到 `pkg\mod\cache`：解包树留着也救不了离线构建，而且目标必须
        // 正好是 `…\go\pkg\mod`，否则 `go clean -modcache` 的路由会静默
        // 失效。判据同 macOS 分支。
        //
        // GOPRIVATE / 自建 proxy 拉下来的私有模块也落在这里，内网机器上删了
        // 可能没有上游可拉——与下面 `~/.m2/repository` 同一条判据（规范第 2
        // 条），类别照旧是包缓存，只是不预选。
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
        //
        // 「切到下载缓存那一层，既回收又不碰解包树」试过，不成立：cargo 和 go
        // 都不能只靠解包树完成离线构建（实测见上面两条目标的注释）。分界只能
        // 落在整个目录上。
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
        // 整个 `registry` 一起清，不切到 `registry/cache` 那一层。
        //
        // 「只删 `.crate` 压缩包、留下 `registry/src` 解包树，离线构建照常」
        // 这个想法实测**不成立**：删掉 `registry/cache` 之后，即使
        // `registry/src/<registry>/itoa-1.0.18/` 连同 `.cargo-ok` 原样在位，
        // `cargo build --offline` 仍然报 `failed to download itoa v1.0.18`
        // ——cargo 要的是 `.crate` 本身，解包树不作数。既然切不出「省得少、
        // 还是坏离线构建」以外的子层，就只能整目录展示。registry 也可能含
        // 私有源，而且删除会破坏离线构建，因此不默认勾选。
        t.push(target_with_recommendation(
            home.join(".cargo/registry"),
            Text::new("cargo 缓存", "cargo cache"),
            CategoryId::PackageCache,
            false,
        ));
        t.push(target(
            home.join(".rustup/downloads"),
            Text::new("rustup 缓存", "rustup cache"),
            CategoryId::PackageCache,
        ));
        // 同样不切到 `pkg/mod/cache`。实测：删掉 `cache/` 之后按域名解包的
        // `github.com/google/uuid@v1.6.0/` 完好无损，`GOPROXY=off go build`
        // 照样挂在 `module lookup disabled by GOPROXY=off`——模块图求解读的是
        // `cache/download/…/@v/*.mod`，解包树里的 go.mod 不顶用。
        //
        // 而且目标必须**正好**是 `…/go/pkg/mod`：`cleaner` 把这条路由到
        // `go clean -modcache`（见 `core::owner`），判定走的是路径后缀匹配。
        // 指到子目录会让路由静默失效，退回裸删——正是 `owner.rs` 开头那条
        // 「删完留下不一致索引」想避开的情形，而且删的就是索引所在的那层。
        //
        // GOPRIVATE / 自建 proxy 拉下来的私有模块也落在这里，内网机器上删了
        // 可能没有上游可拉——与 `~/.m2/repository` 同一条判据（规范第 2 条），
        // 类别照旧是包缓存，只是不预选。
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
        // brew 的 owner command 清理：不只上面的下载缓存，还含旧版本
        // keg、断链 Caskroom 残余——命令自己知道怎么安全收缩（见
        // `core::brew`）。节流：距上次真实清理不足一周就不出现；dry-run
        // 失败或没有可清内容也不出现（不出假条目）。体积是 brew 自己
        // dry-run 给出的估算，不是逐文件称的。
        if crate::core::brew::should_offer(brew_cleanup_at) {
            if let Some((bytes, _files)) = crate::core::brew::cleanup_preview() {
                t.push(ScanTarget {
                    path: crate::core::brew::virtual_path(),
                    label: Text::new("Homebrew 清理", "Homebrew cleanup"),
                    category: CategoryId::PackageCache,
                    recommended: CategoryId::PackageCache.default_selected(),
                    size_hint: Some(bytes),
                });
            }
        }
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
        // pnpm 的 store 根随安装方式而变：`~/Library/pnpm/store` 或
        // `~/.pnpm-store`。本机实测前者是 0B 空目录、后者才是真身（509M），
        // 只登记前者等于让那 509M 完全不可见。两个都登记，不存在的那个会被
        // 扫描阶段的 `exists()` 滤掉，没有代价。
        for (dir, zh, en) in [
            (
                home.join("Library/pnpm/store"),
                "pnpm store (Library)",
                "pnpm store (Library)",
            ),
            (home.join(".pnpm-store"), "pnpm store", "pnpm store"),
        ] {
            t.push(target(dir, Text::new(zh, en), CategoryId::PackageCache));
        }
    }
}

/// 用户缓存（`~/Library/Caches` 展开等）
fn push_user_cache_targets(t: &mut Vec<ScanTarget>, home: &Path) {
    #[cfg(windows)]
    let _ = home;
    #[cfg(windows)]
    {
        let Some(local) = crate::platform::user_cache_dir() else {
            return;
        };
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
        } else if let Some((_, zh, en)) = SHOWN_ONLY_HOME_CACHE_DIRS
            .iter()
            .find(|(key, _, _)| *key == name)
        {
            // 确实是包缓存，只是不能预选：类别不能掉进下面的 `UserTemp`
            // 兜底分支，否则用户在「包缓存」里根本找不到它。
            t.push(target_with_recommendation(
                entry.path(),
                Text::new(*zh, *en),
                CategoryId::PackageCache,
                false,
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
    ("pre-commit", "pre-commit 缓存", "pre-commit cache"),
    ("node-gyp", "node-gyp 缓存", "node-gyp cache"),
    ("typescript", "TypeScript 缓存", "TypeScript cache"),
    ("go-build", "Go 构建缓存", "Go build cache"),
    ("gopls", "gopls 缓存", "gopls cache"),
];

/// 是包缓存、但**只展示不预选**的 `~/.cache` 子目录。
///
/// `pypoetry` —— 里面除了下载缓存还住着 `virtualenvs/`，重建它是一次完整的
/// install 事务而不是「解包一下」；参考实现 Mole 也把 `pypoetry/virtualenvs`
/// 放进**恒常合并、用户覆盖不掉**的安全表（`base.sh` 的
/// `SAFETY_WHITELIST_PATTERNS`）。本机没有这个目录，无法确认纯下载子层的
/// 确切路径，所以按规范第 1 条整项降级，而不是凭印象切一个没验证过的路径。
const SHOWN_ONLY_HOME_CACHE_DIRS: &[(&str, &str, &str)] =
    &[("pypoetry", "Poetry 缓存", "Poetry cache")];

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

#[cfg(test)]
mod tests {
    use super::push_home_cache_targets;
    use crate::core::categories::CategoryId;
    // 以下都是 macOS 专属：被测的 `push_user_cache_dirs` 与年龄门夹具本身就带平台门。
    #[cfg(target_os = "macos")]
    use super::push_user_cache_dirs;
    #[cfg(target_os = "macos")]
    use crate::core::categories::helpers::backdate;
    #[cfg(target_os = "macos")]
    use std::path::PathBuf;

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

    /// 只被部分认领的父目录：其余孩子必须入表。
    ///
    /// `browser.rs` 只认领 `Google/Chrome`，而旧写法把 `Google` 整个跳过，于是
    /// 兄弟子项（GoogleUpdater 的下载目录那一类）在界面上彻底隐身——看不见也
    /// 清不掉，比「不默认勾选」更糟。规范说得很清楚：展示不是成本，隐藏才是。
    #[test]
    #[cfg(target_os = "macos")]
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
        for name in ["uv", "pip", "some-tool-nobody-heard-of"] {
            std::fs::create_dir_all(root.join(".cache").join(name)).unwrap();
        }

        let mut targets = Vec::new();
        push_home_cache_targets(&mut targets, &root);

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
        let unknown = targets
            .iter()
            .find(|t| t.path == root.join(".cache").join("some-tool-nobody-heard-of"))
            .expect("表外的目录仍应展示");
        assert_eq!(unknown.category, CategoryId::UserTemp);
        assert!(!unknown.recommended);
        let _ = std::fs::remove_dir_all(root);
    }
}
