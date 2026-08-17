//! 开发环境垃圾的发现式扫描
//!
//! 和 `categories::all_targets()` 里那些固定路径不同，构建产物没有确定
//! 位置——`node_modules`、`target`、`.venv` 散落在用户所有代码目录里，
//! 只能靠遍历去发现。这里按目录名 + 兄弟文件特征识别可回收的产物目录。
//!
//! 注意：AI agent 的缓存与临时 worktree **不走这里**。它们都固定落在
//! agent 自己的目录下（`~/.codex/worktrees`、`~/.claude/cache` …），
//! 直接在 `categories` 里列路径即可，为它们做全盘检索纯属浪费。
//!
//! 两条实现通道：
//!
//! - **MFT 通道**（有管理员权限时）：`$MFT` 里已经存着全盘每个目录的
//!   名字和聚合体积，直接在内存树上做 DFS，识别与称重一次完成，全盘
//!   几秒即可。
//! - **macOS 索引通道**：对当前用户目录做完整 `getattrlistbulk` 扫描，
//!   持久化目录树并用 FSEvents 增量更新；索引不可用时才回退到有界遍历。
//! - **遍历通道**（兜底）：从若干「代码根目录」出发做有界遍历，命中后还要
//!   再走一遍子树才能拿到体积，慢得多。
//!
//! 三条设计约束：
//!
//! 1. **命中即止**：识别出 `node_modules` 后不再往里走，否则光是遍历
//!    嵌套依赖就能耗掉整轮扫描的时间。
//! 2. **要有旁证**：`target`、`build`、`bin`、`dist` 这些名字太普通，
//!    必须在同级看到 `Cargo.toml`、`CMakeLists.txt`、`*.csproj`、
//!    `package.json` 之类的工程文件才认定，避免误伤同名普通目录。
//! 3. **默认不勾选**：这些目录删掉不会坏系统，但会让下次构建变慢，甚至
//!    （worktree）丢掉未提交的改动，所以交给用户主动选。

use crate::core::categories::CategoryId;
use crate::core::i18n::Text;
use crate::core::scanner::{measure_dir, ScanItem};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// 具名代码根目录（`~/code`、`D:\repos` …）往下最多走多少层。
///
/// 6 层足够覆盖 `~/code/<组织>/<仓库>/<子包>/<模块>/node_modules` 这种
/// 常见深度，再深就得不偿失了。
const NAMED_ROOT_DEPTH: usize = 6;

/// 用户主目录本身只浅扫。
///
/// `~` 底下混着 Downloads、文档、各种应用私有目录，按 6 层扫会把整轮
/// 扫描拖到分钟级。放仓库在 `~` 下的人一般也就一两层深。
const HOME_DEPTH: usize = 2;

/// 遍历时直接跳过的目录名（大小写不敏感）。
///
/// `.git` 里全是对象文件，走进去纯属浪费；`AppData` / `Windows` 之类由
/// 固定路径规则负责，不该让发现式扫描重复趟一遍。
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "appdata",
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
    "$recycle.bin",
    "system volume information",
    "onedrivetemp",
];

/// 一条开发垃圾的识别规则。
struct Marker {
    /// 目录名（小写比较）
    dir: &'static str,
    /// 给用户看的说明（中文）
    label_zh: &'static str,
    /// 给用户看的说明（英文）
    label_en: &'static str,
    category: CategoryId,
    /// 需要在**同级**看到其中任意一个才算数；空数组表示名字本身足够特征化。
    /// 以 `.` 开头的条目按扩展名匹配（如 `.csproj`）。
    sibling_any: &'static [&'static str],
}

const MARKERS: &[Marker] = &[
    // ---- Node / 前端 ----
    Marker {
        dir: "node_modules",
        label_zh: "Node 依赖",
        label_en: "Node dependencies",
        category: CategoryId::DevBuild,
        sibling_any: &[],
    },
    Marker {
        dir: ".next",
        label_zh: "Next.js 构建缓存",
        label_en: "Next.js build cache",
        category: CategoryId::DevBuild,
        sibling_any: &[],
    },
    Marker {
        dir: ".nuxt",
        label_zh: "Nuxt 构建缓存",
        label_en: "Nuxt build cache",
        category: CategoryId::DevBuild,
        sibling_any: &[],
    },
    Marker {
        dir: ".svelte-kit",
        label_zh: "SvelteKit 构建缓存",
        label_en: "SvelteKit build cache",
        category: CategoryId::DevBuild,
        sibling_any: &[],
    },
    Marker {
        dir: ".turbo",
        label_zh: "Turborepo 缓存",
        label_en: "Turborepo cache",
        category: CategoryId::DevBuild,
        sibling_any: &[],
    },
    Marker {
        dir: ".parcel-cache",
        label_zh: "Parcel 缓存",
        label_en: "Parcel cache",
        category: CategoryId::DevBuild,
        sibling_any: &[],
    },
    Marker {
        dir: ".angular",
        label_zh: "Angular 构建缓存",
        label_en: "Angular build cache",
        category: CategoryId::DevBuild,
        sibling_any: &[],
    },
    Marker {
        dir: "dist",
        label_zh: "前端构建产物",
        label_en: "Frontend build output",
        category: CategoryId::DevBuild,
        sibling_any: &["package.json"],
    },
    // ---- Rust ----
    Marker {
        dir: "target",
        label_zh: "Rust 构建产物",
        label_en: "Rust build output",
        category: CategoryId::DevBuild,
        sibling_any: &["Cargo.toml"],
    },
    // ---- Python ----
    Marker {
        dir: ".venv",
        label_zh: "Python 虚拟环境",
        label_en: "Python virtualenv",
        category: CategoryId::DevBuild,
        sibling_any: &[],
    },
    Marker {
        dir: "venv",
        label_zh: "Python 虚拟环境",
        label_en: "Python virtualenv",
        category: CategoryId::DevBuild,
        sibling_any: &[],
    },
    Marker {
        dir: "__pycache__",
        label_zh: "Python 字节码缓存",
        label_en: "Python bytecode cache",
        category: CategoryId::DevBuild,
        sibling_any: &[],
    },
    Marker {
        dir: ".pytest_cache",
        label_zh: "pytest 缓存",
        label_en: "pytest cache",
        category: CategoryId::DevBuild,
        sibling_any: &[],
    },
    Marker {
        dir: ".mypy_cache",
        label_zh: "mypy 缓存",
        label_en: "mypy cache",
        category: CategoryId::DevBuild,
        sibling_any: &[],
    },
    Marker {
        dir: ".ruff_cache",
        label_zh: "ruff 缓存",
        label_en: "ruff cache",
        category: CategoryId::DevBuild,
        sibling_any: &[],
    },
    Marker {
        dir: ".tox",
        label_zh: "tox 环境",
        label_en: "tox environments",
        category: CategoryId::DevBuild,
        sibling_any: &[],
    },
    // ---- C# / .NET ----
    Marker {
        dir: "bin",
        label_zh: ".NET 构建产物",
        label_en: ".NET build output",
        category: CategoryId::DevBuild,
        sibling_any: &[".csproj", ".vbproj", ".fsproj", ".sln"],
    },
    Marker {
        dir: "obj",
        label_zh: ".NET 中间产物",
        label_en: ".NET intermediate output",
        category: CategoryId::DevBuild,
        sibling_any: &[".csproj", ".vbproj", ".fsproj", ".sln"],
    },
    // ---- C / C++ ----
    Marker {
        dir: "build",
        label_zh: "C/C++ 构建产物",
        label_en: "C/C++ build output",
        category: CategoryId::DevBuild,
        sibling_any: &["CMakeLists.txt", "Makefile", "meson.build"],
    },
    Marker {
        dir: "cmake-build-debug",
        label_zh: "CLion 构建产物",
        label_en: "CLion build output",
        category: CategoryId::DevBuild,
        sibling_any: &[],
    },
    Marker {
        dir: "cmake-build-release",
        label_zh: "CLion 构建产物",
        label_en: "CLion build output",
        category: CategoryId::DevBuild,
        sibling_any: &[],
    },
    // ---- JVM / 其它 ----
    Marker {
        dir: ".gradle",
        label_zh: "Gradle 项目缓存",
        label_en: "Gradle project cache",
        category: CategoryId::DevBuild,
        sibling_any: &[],
    },
    Marker {
        dir: ".dart_tool",
        label_zh: "Dart/Flutter 缓存",
        label_en: "Dart/Flutter cache",
        category: CategoryId::DevBuild,
        sibling_any: &[],
    },
    Marker {
        dir: "vendor",
        label_zh: "Go/PHP 依赖副本",
        label_en: "Go/PHP vendored deps",
        category: CategoryId::DevBuild,
        sibling_any: &["go.mod", "composer.json"],
    },
];

/// 常见的代码根目录名，会在用户主目录和各固定磁盘根下探测。
const CODE_ROOT_NAMES: &[&str] = &[
    "code",
    "dev",
    "src",
    "source",
    "projects",
    "project",
    "repos",
    "repo",
    "work",
    "workspace",
    "workspaces",
    "git",
    "github",
    "gitee",
    "developer",
];

/// 列出要遍历的代码根目录。
///
/// 只取真实存在的，避免为不存在的路径付出 IO。
pub fn code_roots() -> Vec<(PathBuf, usize)> {
    let mut roots: Vec<(PathBuf, usize)> = Vec::new();

    if let Some(home) = dirs::home_dir() {
        for name in CODE_ROOT_NAMES {
            roots.push((home.join(name), NAMED_ROOT_DEPTH));
        }
        // 主目录本身也浅扫一层：不少人把仓库直接放在 ~ 下
        roots.push((home, HOME_DEPTH));
    }

    #[cfg(windows)]
    for vol in crate::platform::windows::volume::list_ntfs_volumes() {
        for name in CODE_ROOT_NAMES {
            roots.push((PathBuf::from(format!(r"{vol}:\{name}")), NAMED_ROOT_DEPTH));
        }
    }

    roots.retain(|(p, _)| p.is_dir());
    roots.sort();
    roots.dedup_by(|a, b| a.0 == b.0);
    roots
}

struct Hit {
    path: PathBuf,
    /// 命中的规则本身。标签是双语的，直到建 `ScanItem` 时才展开。
    marker: &'static Marker,
}

/// 发现所有开发垃圾目录并测算体积。
///
/// 有管理员权限时走 MFT，否则退回文件系统遍历。
///
/// `prescanned` 是阶段一为了查表已经解析好的那个卷。它本来跑完就要被丢掉，
/// 接过来直接用能省掉一整次全盘 MFT 解析（本机 C 盘 3.3 秒）。所有权在这里
/// 终结，用完即释放，内存峰值和原来逐卷解析时一样是一棵树。
pub fn discover(
    live: &AtomicBool,
    prescanned: Option<crate::core::disk::ScanResult>,
) -> Vec<ScanItem> {
    discover_inner(live, prescanned)
}

/// macOS 专用：接受 `Arc<ScanResult>` 的 discover 变体，避免 clone 6.6M 条目。
#[cfg(not(windows))]
pub fn discover_arc(
    live: &AtomicBool,
    prescanned: Option<std::sync::Arc<crate::core::disk::ScanResult>>,
) -> Vec<ScanItem> {
    // macOS 路径直接用 Arc，不需要 unwrap_or_clone
    #[cfg(not(windows))]
    {
        let t0 = std::time::Instant::now();
        let items = discover_via_macos_tree_arc(live, prescanned);
        if !items.is_empty() {
            crate::log!(
                "发现式扫描走并行遍历器通道：{:?}，{} 条",
                t0.elapsed(),
                items.len()
            );
            return items;
        }
        crate::log!("并行遍历器通道无结果，回退纯遍历通道");
    }
    let t0 = std::time::Instant::now();
    let items = discover_via_walk(live);
    crate::log!(
        "发现式扫描走遍历通道：{:?}，{} 条",
        t0.elapsed(),
        items.len()
    );
    items
}

fn discover_inner(
    live: &AtomicBool,
    prescanned: Option<crate::core::disk::ScanResult>,
) -> Vec<ScanItem> {
    #[cfg(windows)]
    {
        if crate::platform::windows::security::is_elevated() {
            let t0 = std::time::Instant::now();
            let items = discover_via_mft(live, prescanned);
            crate::log!(
                "发现式扫描走 MFT 通道：{:?}，{} 条",
                t0.elapsed(),
                items.len()
            );
            // 卷打不开（非 NTFS、被占用等）时会拿到空结果，此时仍需兜底
            if !items.is_empty() {
                return items;
            }
            crate::log!("MFT 通道无结果，回退遍历通道");
        } else {
            crate::log!("未提权，发现式扫描走遍历通道（慢得多）");
        }
    }
    #[cfg(not(windows))]
    {
        // macOS：用并行 getattrlistbulk 遍历器构建 SizeTree，然后在树上 DFS。
        // 比纯 walkdir 遍历快 3 倍以上（12s vs 40s 量级），且不需要提权。
        let t0 = std::time::Instant::now();
        let arc_prescanned = prescanned.map(std::sync::Arc::new);
        let items = discover_via_macos_tree(live, arc_prescanned);
        if !items.is_empty() {
            crate::log!(
                "发现式扫描走并行遍历器通道：{:?}，{} 条",
                t0.elapsed(),
                items.len()
            );
            return items;
        }
        crate::log!("并行遍历器通道无结果，回退纯遍历通道");
    }
    let t0 = std::time::Instant::now();
    let items = discover_via_walk(live);
    crate::log!(
        "发现式扫描走遍历通道：{:?}，{} 条",
        t0.elapsed(),
        items.len()
    );
    items
}

/// MFT 通道：在内存目录树上 DFS，体积直接读聚合值，无需二次遍历。
#[cfg(windows)]
fn discover_via_mft(
    live: &AtomicBool,
    prescanned: Option<crate::core::disk::ScanResult>,
) -> Vec<ScanItem> {
    use crate::platform::windows::mft::scan_volume;
    use crate::platform::windows::volume::list_volumes;

    let mut prescanned = prescanned;
    let mut out = Vec::new();
    // 逐卷处理而不是并行扫全部：一棵全盘 SizeTree 就可能占数百 MB，
    // 同时持有多个卷的树会让内存峰值失控。处理完一卷立刻释放。
    for vol in list_volumes() {
        if !live.load(Ordering::Relaxed) {
            break;
        }
        // 阶段一预解析过的那个卷直接接手，别再解析一遍
        let scan = if prescanned.as_ref().is_some_and(|s| &s.volume == &vol) {
            match prescanned.take() {
                Some(s) => {
                    crate::log!("卷 {vol}: 复用阶段一已解析的 MFT 树，省去一次全盘解析");
                    s
                }
                None => continue,
            }
        } else {
            match scan_volume(&vol, 0) {
                Ok(s) => s,
                Err(_) => continue,
            }
        };
        let tree = &scan.tree;
        let mut hits = Vec::new();
        collect_mft(tree, tree.root(), 0, live, &mut hits);

        let mut cache = std::collections::HashMap::new();
        for idx in hits.into_iter() {
            let size = tree.size_of(idx.0);
            if size == 0 {
                continue;
            }
            let path = PathBuf::from(tree.path_of_with(idx.0, &mut cache));
            out.push(ScanItem {
                label: item_label(idx.1, &path),
                path,
                size,
                file_count: tree.file_count_of(idx.0),
                category: idx.1.category,
                // MFT 记录里没有直接可用的修改时间，这一列对开发垃圾也没有
                // 展示价值（构建产物的时间戳随时在变）。
                last_modified: 0,
            });
        }
    }
    out
}

/// MFT 树上的 DFS。命中即止，与遍历通道保持完全一致的判定规则。
#[cfg(windows)]
fn collect_mft(
    tree: &crate::platform::windows::mft::SizeTree,
    dir: u32,
    depth: usize,
    live: &AtomicBool,
    out: &mut Vec<(u32, &'static Marker)>,
) {
    // 树在内存里，可以比遍历通道走得更深
    const MFT_MAX_DEPTH: usize = 12;
    if depth > MFT_MAX_DEPTH || !live.load(Ordering::Relaxed) {
        return;
    }

    let kids = tree.child_indices(dir);
    if kids.is_empty() {
        return;
    }

    // 本层的文件名，供旁证判定使用
    let files: Vec<String> = kids
        .iter()
        .filter(|&&c| tree.valid(c) && !tree.is_dir(c))
        .map(|&c| tree.entry_name(c).to_ascii_lowercase())
        .collect();

    for &child in kids {
        if !tree.valid(child) || !tree.is_dir(child) {
            continue;
        }
        let name = tree.entry_name(child);
        let lower = name.to_ascii_lowercase();
        if SKIP_DIRS.contains(&lower.as_str())
            || crate::platform::windows::mft::SizeTree::is_ntfs_system_meta(child, name)
        {
            continue;
        }
        match MARKERS
            .iter()
            .find(|m| m.dir == lower && has_sibling(&files, m.sibling_any))
        {
            Some(marker) => out.push((child, marker)),
            None => collect_mft(tree, child, depth + 1, live, out),
        }
    }
}

/// macOS 并行遍历器通道：用 `walk::scan_root` 构建 `SizeTree`，然后在树上 DFS。
///
/// 对应 Windows 侧的 `discover_via_mft`。区别是 macOS 没有 NTFS $MFT，
/// 用并行 `getattrlistbulk` 遍历器代替。不需要提权（TCC 不拦第三方目录）。
#[cfg(not(windows))]
fn discover_via_macos_tree(
    live: &AtomicBool,
    prescanned: Option<std::sync::Arc<crate::core::disk::ScanResult>>,
) -> Vec<ScanItem> {
    // 如果调用方已经准备好了索引（scan_fixed 之前加载的），直接复用。
    let scan = if let Some(pre) = prescanned {
        pre
    } else {
        match load_or_build_macos_index(live) {
            Some(s) => s,
            None => return Vec::new(),
        }
    };
    collect_tree_and_build_items(&scan, live)
}

/// macOS Arc 版本：接受 `Arc<ScanResult>`，避免 clone 6.6M 条目。
#[cfg(not(windows))]
fn discover_via_macos_tree_arc(
    live: &AtomicBool,
    prescanned: Option<std::sync::Arc<crate::core::disk::ScanResult>>,
) -> Vec<ScanItem> {
    discover_via_macos_tree(live, prescanned)
}

/// 在 SizeTree 上 DFS 匹配 marker，构建 ScanItem 列表。
#[cfg(not(windows))]
fn collect_tree_and_build_items(
    scan: &crate::core::disk::ScanResult,
    live: &AtomicBool,
) -> Vec<ScanItem> {
    let tree = &scan.tree;
    let mut hits = Vec::new();
    // 树本身已经是完整索引，不再使用固定深度限制；只依靠 SKIP_DIRS 和命中即止。
    collect_tree(tree, tree.root(), 0, usize::MAX, live, &mut hits);
    let mut cache = std::collections::HashMap::new();
    hits.into_iter()
        .filter_map(|(idx, marker)| {
            let size = tree.size_of(idx);
            if size == 0 {
                return None;
            }
            let path = PathBuf::from(tree.path_of_with(idx, &mut cache));
            Some(ScanItem {
                label: item_label(marker, &path),
                path,
                size,
                file_count: tree.file_count_of(idx),
                category: marker.category,
                last_modified: 0,
            })
        })
        .collect()
}

/// 加载或构建 macOS 用户目录索引。
///
/// 这是 macOS 扫描的核心入口，被 `scan_fixed`（查表）和 `scan_discovered`（DFS）
/// 共同复用。
#[cfg(not(windows))]
pub fn load_or_build_macos_index(
    live: &AtomicBool,
) -> Option<std::sync::Arc<crate::core::disk::ScanResult>> {
    let home = dirs::home_dir()?;
    load_or_build_macos_index_for(&home, "用户目录", live)
}

/// 加载或构建 macOS 整盘索引（磁盘透镜用）。
///
/// 根目录是 `/`，包含 Users、Applications、Library 等顶层目录。
/// 首次扫描全 `/` 可能比用户目录慢 3-5 倍（全 SSD 约 1-2 分钟），
/// 但持久化 + FSEvents 增量后，后续打开磁盘透镜与现在一样快。
#[cfg(not(windows))]
pub fn load_or_build_macos_root_index(
    live: &AtomicBool,
) -> Option<std::sync::Arc<crate::core::disk::ScanResult>> {
    let root = std::path::PathBuf::from("/");
    load_or_build_macos_index_for(&root, "整盘", live)
}

/// 通用 macOS 索引加载/构建。
///
/// 流程：
/// 1. 尝试加载持久化索引
/// 2. 有索引 → 回放 FSEvents → 无变化直接复用 / 有变化增量更新 / 不可信全量重建
/// 3. 无索引 → 全量扫描并持久化
///
/// 返回 `Arc<ScanResult>` 以便后台保存线程共享所有权，避免克隆 6.6M 条目。
#[cfg(not(windows))]
fn load_or_build_macos_index_for(
    root: &std::path::Path,
    label: &str,
    live: &AtomicBool,
) -> Option<std::sync::Arc<crate::core::disk::ScanResult>> {
    use crate::platform::macos::walk;

    if !live.load(Ordering::Relaxed) {
        return None;
    }

    let t0 = std::time::Instant::now();
    let volume = crate::core::disk::VolumeId::from_mount_point(root.to_path_buf());
    let scan: std::sync::Arc<crate::core::disk::ScanResult> = if let Some(loaded) =
        crate::platform::macos::cache::load_index(&volume)
    {
        crate::log!(
            "加载 {} 索引：{} 条记录，上次事件 ID {}，耗时 {:?}",
            label,
            loaded.scan.records_read,
            loaded.last_event_id,
            t0.elapsed()
        );
        let t_fse = std::time::Instant::now();
        match crate::platform::macos::fsevents::changes_since(root, loaded.last_event_id) {
            Some(changes) if changes.paths.is_empty() && !changes.requires_full_scan => {
                crate::log!(
                    "复用 {} 索引：{} 条记录，FSEvents 无变化（回放耗时 {:?}）",
                    label,
                    loaded.scan.records_read,
                    t_fse.elapsed()
                );
                std::sync::Arc::new(loaded.scan)
            }
            Some(changes) => {
                if !changes.requires_full_scan {
                    let t_refresh = std::time::Instant::now();
                    match refresh_macos_index(&volume, loaded.scan, &changes, live) {
                        Some(scan) => {
                            crate::log!(
                                "增量更新 {} 索引：{} 个事件路径，{} 条记录，耗时 {:?}",
                                label,
                                changes.paths.len(),
                                scan.records_read,
                                t_refresh.elapsed()
                            );
                            // 异步保存：后台线程序列化+压缩+写盘，
                            // 不阻塞 scan_fixed 和 scan_discovered
                            let arc = std::sync::Arc::new(scan);
                            spawn_save_index(volume.clone(), arc.clone(), changes.last_event_id);
                            arc
                        }
                        None => {
                            crate::log!(
                                "增量更新返回 None（变更根目录 >512 或父节点缺失），回退全量扫描"
                            );
                            match full_macos_scan(root, &volume, live) {
                                Ok(scan) => std::sync::Arc::new(scan),
                                Err(error) => {
                                    crate::log!("{} {} 扫描失败: {error}", label, root.display());
                                    return None;
                                }
                            }
                        }
                    }
                } else {
                    crate::log!(
                        "{} 索引需要全量重建：{} 个事件路径，原因={:?}，原始 {} 事件，过滤缓存 {}",
                        label,
                        changes.paths.len(),
                        changes.full_scan_reason,
                        changes.raw_event_count,
                        changes.filtered_cache_events
                    );
                    match full_macos_scan(root, &volume, live) {
                        Ok(scan) => std::sync::Arc::new(scan),
                        Err(error) => {
                            crate::log!("{} {} 扫描失败: {error}", label, root.display());
                            return None;
                        }
                    }
                }
            }
            None => {
                crate::log!(
                        "{} 索引的 FSEvents 水位不可回放（since={}），执行一致性重扫，FSEvents 耗时 {:?}",
                        label,
                        loaded.last_event_id,
                        t_fse.elapsed()
                    );
                let checkpoint = crate::platform::macos::fsevents::current_event_id();
                let scan = match walk::scan_root(root, volume.clone(), live) {
                    Ok(scan) => scan,
                    Err(error) => {
                        crate::log!("{} {} 扫描失败: {error}", label, root.display());
                        return None;
                    }
                };
                let arc = std::sync::Arc::new(scan);
                spawn_save_index(volume.clone(), arc.clone(), checkpoint);
                arc
            }
        }
    } else {
        crate::log!("未找到 {} 索引，执行首次全量扫描", label);
        let scan = match full_macos_scan(root, &volume, live) {
            Ok(scan) => scan,
            Err(error) => {
                crate::log!("{} {} 扫描失败: {error}", label, root.display());
                return None;
            }
        };
        crate::log!(
            "{} 首次全量扫描完成：{} 条记录，耗时 {:?}",
            label,
            scan.records_read,
            t0.elapsed()
        );
        std::sync::Arc::new(scan)
    };
    Some(scan)
}

/// 后台线程异步保存索引，不阻塞扫描流程。
#[cfg(not(windows))]
fn spawn_save_index(
    volume: crate::core::disk::VolumeId,
    scan: std::sync::Arc<crate::core::disk::ScanResult>,
    last_event_id: u64,
) {
    std::thread::spawn(move || {
        let t = std::time::Instant::now();
        crate::platform::macos::cache::save_index(&volume, &scan, last_event_id);
        crate::log!("异步保存索引完成：{:?}", t.elapsed());
    });
}

/// 全量重建索引，并在扫描开始前保存 FSEvents 检查点。
#[cfg(not(windows))]
fn full_macos_scan(
    root: &Path,
    volume: &crate::core::disk::VolumeId,
    live: &AtomicBool,
) -> Result<crate::core::disk::ScanResult, crate::core::disk::ScanError> {
    let checkpoint = crate::platform::macos::fsevents::current_event_id();
    let scan = crate::platform::macos::walk::scan_root(root, volume.clone(), live)?;
    crate::platform::macos::cache::save_index(volume, &scan, checkpoint);
    Ok(scan)
}

/// 用 FSEvents 变更路径重扫局部子树，避免每次小改动都重扫整个用户目录。
///
/// 直接在 `SizeTree` 上就地操作：删除旧子树、追加新子树、重建 CSR 索引。
/// 不再走 `snapshot_entries` → `from_snapshot` 的全量 PathBuf 转换路径，
/// 避免为更新一个 `node_modules` 目录而把 6.6M 节点全部转成路径再重建。
///
/// 删除和重命名会重扫对应父目录，日志丢失等不可信情况在 FSEvents 层
/// 标记为需要全量扫描。
#[cfg(not(windows))]
fn refresh_macos_index(
    volume: &crate::core::disk::VolumeId,
    mut scan: crate::core::disk::ScanResult,
    changes: &crate::platform::macos::fsevents::Changes,
    live: &AtomicBool,
) -> Option<crate::core::disk::ScanResult> {
    use crate::platform::macos::walk;

    let mount = volume.mount_point();
    let mut roots: Vec<PathBuf> = changes
        .paths
        .iter()
        .filter(|path| path.starts_with(mount))
        .map(|path| {
            if path.is_dir() {
                path.clone()
            } else {
                path.parent().unwrap_or(mount).to_path_buf()
            }
        })
        .collect();
    // 去掉等于 mount 本身的路径——它会吞掉所有其他路径，导致
    // 4959 个变更被归并成 1 个根（mount），然后触发"根==mount"放弃增量。
    // 这些路径的真正变更根是它们的直接子目录，保留更深的路径即可。
    roots.retain(|path| path != mount);
    roots.sort_by_key(|path| path.components().count());
    roots.dedup();
    let mut covered = Vec::new();
    roots.retain(|path| {
        if covered
            .iter()
            .any(|parent: &PathBuf| path.starts_with(parent))
        {
            return false;
        }
        covered.push(path.clone());
        true
    });
    crate::log!(
        "refresh_macos_index: {} 个原始路径 → 去重后 {} 个独立变更根",
        changes.paths.len(),
        roots.len()
    );
    if roots.is_empty() {
        crate::log!("refresh_macos_index: 去重后无变更根，跳过增量更新");
        return None;
    }
    // 太多彼此独立的变化目录时，重扫局部区域反而比一次完整扫描更慢。
    // 但有了并行扫描 + 跳过小目录/超大子树优化后，500 个根也能在几秒内完成。
    if roots.len() > 512 {
        crate::log!(
            "refresh_macos_index: 独立变更根 {} 个 > 512，放弃增量",
            roots.len()
        );
        return None;
    }

    // 并行扫描所有独立变更根，然后串行追加到树。
    // 之前串行扫描 65 个根要 5.7s（Notion 2s + Edge Cache 2s + Telegram 600ms），
    // 并行后墙钟时间等于最慢的那个子树。
    //
    // 优化：旧树中记录数 < 200 的根跳过重扫。macOS Container 里有些
    // 几十条记录的小目录因 TCC 权限检查阻塞 6 秒，跳过它们省 90% 增量耗时，
    // 体积误差 < 1MB（相对 78GB 总量可忽略）。
    // 同理跳过 iCloud Drive（~/Library/Mobile Documents）：文件可能被驱逐
    // 到云端，getattrlistbulk 会阻塞等网络 I/O，23 秒扫 8K 条记录。
    // 超大子树（>100K 记录）也跳过：重扫 /Users/yuqiang（6.6M 记录）要 74 秒，
    // /System、/Library 等也很慢。这些目录变化通常很小，跳过误差可忽略。
    const SKIP_THRESHOLD: u64 = 200;
    const SKIP_LARGE_THRESHOLD: u64 = 100_000;
    use rayon::prelude::*;
    let t_par = std::time::Instant::now();
    struct SubtreeResult {
        root: PathBuf,
        scan: crate::core::disk::ScanResult,
    }
    let scan_results: Vec<SubtreeResult> = roots
        .par_iter()
        .filter_map(|root| {
            if !live.load(Ordering::Relaxed) || !root.exists() {
                return None;
            }
            // 跳过 iCloud Drive 目录——文件可能被驱逐到云端，
            // 扫描会阻塞在网络 I/O 上数十秒
            if root.to_string_lossy().contains("Library/Mobile Documents/") {
                crate::log!("  跳过 iCloud Drive 目录 {}，保留旧数据", root.display());
                return None;
            }
            // 检查旧树中该根的记录数
            if let Some(old_node) = scan.tree.find_node_by_path(root) {
                let old_files = scan.tree.file_count_of(old_node);
                if old_files < SKIP_THRESHOLD {
                    crate::log!(
                        "  跳过小目录 {}：旧记录 {} < {}，保留旧数据",
                        root.display(),
                        old_files,
                        SKIP_THRESHOLD
                    );
                    return None;
                }
                // 超大子树跳过：重扫 /Users/yuqiang（6.6M 记录）要 74 秒，
                // /System、/Library 等也很慢。这些目录变化占比极小，
                // 跳过重扫的体积误差可忽略。
                if old_files > SKIP_LARGE_THRESHOLD {
                    crate::log!(
                        "  跳过超大子树 {}：旧记录 {} > {}，保留旧数据",
                        root.display(),
                        old_files,
                        SKIP_LARGE_THRESHOLD
                    );
                    return None;
                }
            }
            let local_volume = crate::core::disk::VolumeId::from_mount_point(root.clone());
            let t_sub = std::time::Instant::now();
            match walk::scan_root_few_threads(root, local_volume, live) {
                Ok(s) => {
                    let dur = t_sub.elapsed();
                    let sub_records = s.records_read;
                    crate::log!(
                        "  增量重扫 {}：{} 条记录，耗时 {:?}",
                        root.display(),
                        sub_records,
                        dur
                    );
                    Some(SubtreeResult {
                        root: root.clone(),
                        scan: s,
                    })
                }
                Err(e) => {
                    crate::log!(
                        "refresh_macos_index: 子树 {} 扫描失败: {}",
                        root.display(),
                        e
                    );
                    None
                }
            }
        })
        .collect();

    if !live.load(Ordering::Relaxed) {
        crate::log!("refresh_macos_index: 扫描被取消");
        return None;
    }
    // 统计实际需要扫描的根数（排除被跳过的小目录、iCloud Drive 和超大子树）
    let skipped = roots
        .iter()
        .filter(|r| {
            if !r.exists() {
                return false;
            }
            if r.to_string_lossy().contains("Library/Mobile Documents/") {
                return true;
            }
            scan.tree
                .find_node_by_path(r)
                .map(|n| {
                    let cnt = scan.tree.file_count_of(n);
                    !(SKIP_THRESHOLD..=SKIP_LARGE_THRESHOLD).contains(&cnt)
                })
                .unwrap_or(false)
        })
        .count();
    let expected = roots.iter().filter(|r| r.exists()).count() - skipped;
    if scan_results.len() < expected {
        crate::log!(
            "refresh_macos_index: {}/{} 子树扫描失败（跳过 {} 个小目录），放弃增量",
            expected - scan_results.len(),
            expected,
            skipped
        );
        return None;
    }
    crate::log!(
        "refresh_macos_index: 并行扫描 {} 个子树（跳过 {} 个小目录），总耗时 {:?}",
        scan_results.len(),
        skipped,
        t_par.elapsed()
    );

    // 串行追加到树——append_subtree 会修改树结构，不能并行
    for sr in scan_results {
        // 在树中定位旧节点并就地移除
        if let Some(old_node) = scan.tree.find_node_by_path(&sr.root) {
            scan.tree.remove_subtree_inplace(old_node);
        }

        let root_name = sr
            .root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let parent_path = sr.root.parent().unwrap_or(mount);
        let parent_node = scan.tree.find_node_by_path(parent_path);

        if let Some(parent_idx) = parent_node {
            scan.tree
                .append_subtree(parent_idx, &sr.scan.tree, &root_name);
        } else {
            crate::log!(
                "refresh_macos_index: 父目录 {} 不在树中，跳过 {}",
                parent_path.display(),
                sr.root.display()
            );
        }
    }

    // 重建 CSR 子节点索引（一次 O(n) 整数操作，无 PathBuf 分配）
    scan.tree.rebuild_child_arrays();

    // 更新扫描元数据
    let total_size = scan.tree.size_of(scan.tree.root());
    let file_count = scan.tree.count_used_files();
    let dir_count = scan.tree.count_used_dirs();
    let records = file_count + dir_count;
    scan.total_size = total_size;
    scan.file_count = file_count;
    scan.dir_count = dir_count;
    scan.records_read = records;
    scan.records_expected = records;
    scan.unique_size = total_size;
    scan.unique_files = file_count;
    Some(scan)
}

/// macOS SizeTree 上的 DFS，与遍历通道 `collect` 保持完全一致的判定规则。
#[cfg(not(windows))]
fn collect_tree(
    tree: &crate::core::disk::SizeTree,
    dir: u32,
    depth: usize,
    max_depth: usize,
    live: &AtomicBool,
    out: &mut Vec<(u32, &'static Marker)>,
) {
    if depth > max_depth || !live.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }

    let kids = tree.child_indices(dir);
    if kids.is_empty() {
        return;
    }

    // 本层的文件名，供旁证判定使用
    let files: Vec<String> = kids
        .iter()
        .filter(|&&c| tree.valid(c) && !tree.is_dir(c))
        .map(|&c| tree.entry_name(c).to_ascii_lowercase())
        .collect();

    for &child in kids {
        if !tree.valid(child) || !tree.is_dir(child) {
            continue;
        }
        let name = tree.entry_name(child);
        let lower = name.to_ascii_lowercase();
        if SKIP_DIRS.contains(&lower.as_str()) {
            continue;
        }
        match MARKERS
            .iter()
            .find(|m| m.dir == lower && has_sibling(&files, m.sibling_any))
        {
            Some(marker) => out.push((child, marker)),
            None => collect_tree(tree, child, depth + 1, max_depth, live, out),
        }
    }
}

/// 遍历通道：无管理员权限时的兜底。
fn discover_via_walk(live: &AtomicBool) -> Vec<ScanItem> {
    discover_via_walk_roots(&code_roots(), live)
}

/// 只遍历指定的代码根目录，供 macOS 的主目录浅扫复用。
fn discover_via_walk_roots(roots: &[(PathBuf, usize)], live: &AtomicBool) -> Vec<ScanItem> {
    // 各根目录之间并行；每个根内部的递归也会继续分叉。
    let hits: Vec<Hit> = roots
        .par_iter()
        .flat_map_iter(|(root, max_depth)| {
            let mut out = Vec::new();
            collect(root, 0, *max_depth, live, &mut out);
            out
        })
        .collect();

    // 体积测算同样并行，这是整轮里最花时间的一步。
    hits.par_iter()
        .filter(|_| live.load(Ordering::Relaxed))
        .map(|hit| {
            let acc = measure_dir(&hit.path, live);
            ScanItem {
                label: item_label(hit.marker, &hit.path),
                path: hit.path.clone(),
                size: acc.0,
                file_count: acc.1,
                category: hit.marker.category,
                last_modified: acc.2,
            }
        })
        .filter(|item| item.size > 0)
        .collect()
}

/// 列表里显示的标签：规则名 + 缩短过的路径，两种语言各拼一条。
fn item_label(marker: &Marker, path: &Path) -> Text {
    let sp = short_path(path);
    Text::new(
        format!("{} · {sp}", marker.label_zh),
        format!("{} · {sp}", marker.label_en),
    )
}

/// 递归查找命中项。命中后不再深入该目录。
fn collect(dir: &Path, depth: usize, max_depth: usize, live: &AtomicBool, out: &mut Vec<Hit>) {
    if depth > max_depth || !live.load(Ordering::Relaxed) {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };

    // 先把这一层的条目读完，兄弟文件判定需要完整的同级视图。
    let mut subdirs: Vec<(PathBuf, String)> = Vec::new();
    let mut file_names: Vec<String> = Vec::new();
    for entry in rd.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        if ft.is_dir() {
            // 符号链接/junction 不跟进，避免走进别的卷甚至成环
            if ft.is_symlink() {
                continue;
            }
            subdirs.push((entry.path(), name));
        } else {
            file_names.push(name.to_ascii_lowercase());
        }
    }

    for (path, name) in subdirs {
        let lower = name.to_ascii_lowercase();
        if SKIP_DIRS.contains(&lower.as_str()) {
            continue;
        }
        match MARKERS
            .iter()
            .find(|m| m.dir == lower && has_sibling(&file_names, m.sibling_any))
        {
            Some(marker) => out.push(Hit { path, marker }),
            // 命中的目录不再下钻；没命中的继续往下找
            None => collect(&path, depth + 1, max_depth, live, out),
        }
    }
}

/// 同级是否存在规则要求的旁证文件。
fn has_sibling(file_names: &[String], required: &[&str]) -> bool {
    if required.is_empty() {
        return true;
    }
    required.iter().any(|want| {
        let want = want.to_ascii_lowercase();
        if let Some(ext) = want.strip_prefix('.') {
            // 形如 ".csproj"：按扩展名匹配任意文件名
            file_names
                .iter()
                .any(|f| f.rsplit('.').next() == Some(ext) && f.len() > ext.len() + 1)
        } else {
            file_names.iter().any(|f| f == &want)
        }
    })
}

/// 列表里只显示最后两级路径，完整路径太长会把行挤爆。
fn short_path(path: &Path) -> String {
    let parts: Vec<_> = path.components().collect();
    let tail: Vec<String> = parts
        .iter()
        .rev()
        .take(3)
        .rev()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    if parts.len() > 3 {
        format!("…\\{}", tail.join("\\"))
    } else {
        tail.join("\\")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::i18n::Language;

    /// 列表标签两种语言都要拼全：规则名跟着语言走，路径两边一样。
    #[test]
    fn item_label_carries_both_languages() {
        let marker = MARKERS
            .iter()
            .find(|m| m.dir == "node_modules")
            .expect("node_modules 规则应该在表里");
        let label = item_label(marker, Path::new(r"D:\code\demo\node_modules"));

        let zh = label.get(Language::Zh);
        let en = label.get(Language::En);
        assert!(
            zh.starts_with(marker.label_zh),
            "中文标签没拼上规则名：{zh}"
        );
        assert!(
            en.starts_with(marker.label_en),
            "英文标签没拼上规则名：{en}"
        );
        assert_ne!(zh, en, "两种语言不该是同一串");
        // 路径那一段与语言无关，两边必须一致
        let tail = |s: &str| s.split_once(" · ").map(|(_, t)| t.to_string());
        assert_eq!(tail(zh), tail(en));
        assert!(tail(zh).is_some_and(|t| t.contains("demo")), "路径没拼进去");
    }

    #[test]
    fn sibling_rule_matches_exact_names() {
        let files = vec!["cargo.toml".to_string(), "readme.md".to_string()];
        assert!(has_sibling(&files, &["Cargo.toml"]));
        assert!(!has_sibling(&files, &["package.json"]));
    }

    #[test]
    fn sibling_rule_matches_extensions() {
        let files = vec!["myapp.csproj".to_string()];
        assert!(has_sibling(&files, &[".csproj", ".sln"]));
        assert!(!has_sibling(&files, &[".fsproj"]));
    }

    #[test]
    fn empty_sibling_rule_always_matches() {
        assert!(has_sibling(&[], &[]));
    }

    /// `.csproj` 这类扩展名规则不能被一个恰好叫 ".csproj" 的文件骗过
    #[test]
    fn extension_rule_requires_a_stem() {
        let files = vec![".csproj".to_string()];
        assert!(!has_sibling(&files, &[".csproj"]));
    }

    #[test]
    fn markers_reference_only_dev_categories() {
        for m in MARKERS {
            // 发现式扫描只负责构建产物；worktree 已改为固定路径
            assert_eq!(
                m.category,
                CategoryId::DevBuild,
                "{} 归到了错误的类别",
                m.dir
            );
            assert_eq!(m.dir, m.dir.to_ascii_lowercase(), "{} 必须是小写", m.dir);
        }
    }

    #[test]
    fn discovers_marked_dirs_and_skips_unmarked_lookalikes() {
        let base = std::env::temp_dir().join("qc_devscan_test");
        let _ = std::fs::remove_dir_all(&base);

        // 命中：有 Cargo.toml 旁证的 target
        let rust_proj = base.join("proj-rs");
        std::fs::create_dir_all(rust_proj.join("target")).unwrap();
        std::fs::write(rust_proj.join("Cargo.toml"), b"[package]").unwrap();
        std::fs::write(rust_proj.join("target").join("blob.bin"), vec![b'x'; 4096]).unwrap();

        // 不命中：没有任何工程文件旁证的同名 target 目录
        let doc_dir = base.join("notes");
        std::fs::create_dir_all(doc_dir.join("target")).unwrap();
        std::fs::write(doc_dir.join("target").join("a.txt"), b"hello").unwrap();

        let live = AtomicBool::new(true);
        let mut hits = Vec::new();
        collect(&base, 0, NAMED_ROOT_DEPTH, &live, &mut hits);

        let paths: Vec<_> = hits.iter().map(|h| h.path.clone()).collect();
        assert!(
            paths.contains(&rust_proj.join("target")),
            "有 Cargo.toml 的 target 应被识别"
        );
        assert!(
            !paths.contains(&doc_dir.join("target")),
            "无旁证的 target 不该被识别"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn does_not_descend_into_a_matched_directory() {
        let base = std::env::temp_dir().join("qc_devscan_nested");
        let _ = std::fs::remove_dir_all(&base);

        // node_modules 里往往还嵌着 node_modules，只应报告最外层那个
        let nested = base.join("node_modules").join("pkg").join("node_modules");
        std::fs::create_dir_all(&nested).unwrap();

        let live = AtomicBool::new(true);
        let mut hits = Vec::new();
        collect(&base, 0, NAMED_ROOT_DEPTH, &live, &mut hits);

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, base.join("node_modules"));

        let _ = std::fs::remove_dir_all(&base);
    }

    /// macOS 树通道的 `collect_tree` 应与遍历通道的 `collect` 行为一致：
    /// 同样的标记目录、同样的旁证判定、同样的「命中即止」语义。
    #[cfg(not(windows))]
    #[test]
    fn collect_tree_finds_same_markers_as_collect() {
        use crate::core::disk::VolumeId;
        use crate::platform::macos::walk;

        let base = std::env::temp_dir().join("qc_devscan_tree_test");
        let _ = std::fs::remove_dir_all(&base);

        // 命中：有 Cargo.toml 旁证的 target
        let rust_proj = base.join("proj-rs");
        std::fs::create_dir_all(rust_proj.join("target")).unwrap();
        std::fs::write(rust_proj.join("Cargo.toml"), b"[package]").unwrap();
        std::fs::write(rust_proj.join("target").join("blob.bin"), vec![b'x'; 4096]).unwrap();

        // 不命中：无旁证的 target
        let doc_dir = base.join("notes");
        std::fs::create_dir_all(doc_dir.join("target")).unwrap();
        std::fs::write(doc_dir.join("target").join("a.txt"), b"hello").unwrap();

        let live = AtomicBool::new(true);

        // 遍历通道（基准）
        let mut walk_hits = Vec::new();
        collect(&base, 0, NAMED_ROOT_DEPTH, &live, &mut walk_hits);
        let walk_paths: Vec<_> = walk_hits.iter().map(|h| h.path.clone()).collect();

        // 树通道：扫描 base 的父目录（temp_dir），在树里定位到 base 再 DFS
        let vol = VolumeId::from_mount_point(base.clone());
        // scan_root 需要 base 存在且可读——上面 create_dir_all 已保证
        let scan = walk::scan_root(&base, vol, &live).expect("扫描测试目录应当成功");
        let tree = &scan.tree;
        let root_node = tree.root();
        let mut tree_hits = Vec::new();
        collect_tree(tree, root_node, 0, NAMED_ROOT_DEPTH, &live, &mut tree_hits);

        // 把树上的节点转成路径再比较
        let tree_paths: Vec<_> = tree_hits
            .iter()
            .map(|(idx, _)| PathBuf::from(tree.path_of(*idx)))
            .collect();

        assert!(
            tree_paths.contains(&rust_proj.join("target")),
            "树通道应识别有 Cargo.toml 的 target"
        );
        assert!(
            !tree_paths.contains(&doc_dir.join("target")),
            "树通道不该识别无旁证的 target"
        );
        assert_eq!(walk_paths.len(), tree_paths.len(), "两条通道命中数应一致");

        let _ = std::fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod bench_probe {
    use std::sync::atomic::AtomicBool;
    use std::time::Instant;

    /// 手动跑：对比两条通道的耗时与命中数。
    /// `cargo test --lib compare_channels -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn compare_channels() {
        let live = AtomicBool::new(true);

        #[cfg(windows)]
        {
            let elevated = crate::platform::windows::security::is_elevated();
            println!("是否已提权: {elevated}");
            if elevated {
                let t = Instant::now();
                let items = super::discover_via_mft(&live, None);
                let total: u64 = items.iter().map(|i| i.size).sum();
                println!(
                    "MFT   通道: {:>7.2} 秒, {} 项, 合计 {}",
                    t.elapsed().as_secs_f64(),
                    items.len(),
                    crate::core::model::fmt_size(total)
                );
            }
        }

        #[cfg(not(windows))]
        {
            let t = Instant::now();
            let items = super::discover_via_macos_tree(&live, None);
            let total: u64 = items.iter().map(|i| i.size).sum();
            println!(
                "树    通道: {:>7.2} 秒, {} 项, 合计 {}",
                t.elapsed().as_secs_f64(),
                items.len(),
                crate::core::model::fmt_size(total)
            );
        }

        let t = Instant::now();
        let items = super::discover_via_walk(&live);
        let total: u64 = items.iter().map(|i| i.size).sum();
        println!(
            "遍历 通道: {:>7.2} 秒, {} 项, 合计 {}",
            t.elapsed().as_secs_f64(),
            items.len(),
            crate::core::model::fmt_size(total)
        );
    }
}
