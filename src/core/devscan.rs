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
//! - **遍历通道**（无管理员权限时的兜底）：从若干「代码根目录」出发做
//!   有界遍历，命中后还要再走一遍子树才能拿到体积，慢得多，因此根目录
//!   和深度都收得比较紧。
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
    Marker { dir: "node_modules", label_zh: "Node 依赖", label_en: "Node dependencies", category: CategoryId::DevBuild, sibling_any: &[] },
    Marker { dir: ".next", label_zh: "Next.js 构建缓存", label_en: "Next.js build cache", category: CategoryId::DevBuild, sibling_any: &[] },
    Marker { dir: ".nuxt", label_zh: "Nuxt 构建缓存", label_en: "Nuxt build cache", category: CategoryId::DevBuild, sibling_any: &[] },
    Marker { dir: ".svelte-kit", label_zh: "SvelteKit 构建缓存", label_en: "SvelteKit build cache", category: CategoryId::DevBuild, sibling_any: &[] },
    Marker { dir: ".turbo", label_zh: "Turborepo 缓存", label_en: "Turborepo cache", category: CategoryId::DevBuild, sibling_any: &[] },
    Marker { dir: ".parcel-cache", label_zh: "Parcel 缓存", label_en: "Parcel cache", category: CategoryId::DevBuild, sibling_any: &[] },
    Marker { dir: ".angular", label_zh: "Angular 构建缓存", label_en: "Angular build cache", category: CategoryId::DevBuild, sibling_any: &[] },
    Marker { dir: "dist", label_zh: "前端构建产物", label_en: "Frontend build output", category: CategoryId::DevBuild, sibling_any: &["package.json"] },
    // ---- Rust ----
    Marker { dir: "target", label_zh: "Rust 构建产物", label_en: "Rust build output", category: CategoryId::DevBuild, sibling_any: &["Cargo.toml"] },
    // ---- Python ----
    Marker { dir: ".venv", label_zh: "Python 虚拟环境", label_en: "Python virtualenv", category: CategoryId::DevBuild, sibling_any: &[] },
    Marker { dir: "venv", label_zh: "Python 虚拟环境", label_en: "Python virtualenv", category: CategoryId::DevBuild, sibling_any: &[] },
    Marker { dir: "__pycache__", label_zh: "Python 字节码缓存", label_en: "Python bytecode cache", category: CategoryId::DevBuild, sibling_any: &[] },
    Marker { dir: ".pytest_cache", label_zh: "pytest 缓存", label_en: "pytest cache", category: CategoryId::DevBuild, sibling_any: &[] },
    Marker { dir: ".mypy_cache", label_zh: "mypy 缓存", label_en: "mypy cache", category: CategoryId::DevBuild, sibling_any: &[] },
    Marker { dir: ".ruff_cache", label_zh: "ruff 缓存", label_en: "ruff cache", category: CategoryId::DevBuild, sibling_any: &[] },
    Marker { dir: ".tox", label_zh: "tox 环境", label_en: "tox environments", category: CategoryId::DevBuild, sibling_any: &[] },
    // ---- C# / .NET ----
    Marker { dir: "bin", label_zh: ".NET 构建产物", label_en: ".NET build output", category: CategoryId::DevBuild, sibling_any: &[".csproj", ".vbproj", ".fsproj", ".sln"] },
    Marker { dir: "obj", label_zh: ".NET 中间产物", label_en: ".NET intermediate output", category: CategoryId::DevBuild, sibling_any: &[".csproj", ".vbproj", ".fsproj", ".sln"] },
    // ---- C / C++ ----
    Marker { dir: "build", label_zh: "C/C++ 构建产物", label_en: "C/C++ build output", category: CategoryId::DevBuild, sibling_any: &["CMakeLists.txt", "Makefile", "meson.build"] },
    Marker { dir: "cmake-build-debug", label_zh: "CLion 构建产物", label_en: "CLion build output", category: CategoryId::DevBuild, sibling_any: &[] },
    Marker { dir: "cmake-build-release", label_zh: "CLion 构建产物", label_en: "CLion build output", category: CategoryId::DevBuild, sibling_any: &[] },
    // ---- JVM / 其它 ----
    Marker { dir: ".gradle", label_zh: "Gradle 项目缓存", label_en: "Gradle project cache", category: CategoryId::DevBuild, sibling_any: &[] },
    Marker { dir: ".dart_tool", label_zh: "Dart/Flutter 缓存", label_en: "Dart/Flutter cache", category: CategoryId::DevBuild, sibling_any: &[] },
    Marker { dir: "vendor", label_zh: "Go/PHP 依赖副本", label_en: "Go/PHP vendored deps", category: CategoryId::DevBuild, sibling_any: &["go.mod", "composer.json"] },
];

/// 常见的代码根目录名，会在用户主目录和各固定磁盘根下探测。
const CODE_ROOT_NAMES: &[&str] = &[
    "code", "dev", "src", "source", "projects", "project", "repos", "repo",
    "work", "workspace", "workspaces", "git", "github", "gitee", "developer",
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
pub fn discover(live: &AtomicBool, prescanned: Option<crate::core::disk::MftScan>) -> Vec<ScanItem> {
    #[cfg(not(windows))]
    let _ = prescanned;
    #[cfg(windows)]
    {
        if crate::platform::windows::security::is_elevated() {
            let t0 = std::time::Instant::now();
            let items = discover_via_mft(live, prescanned);
            crate::log!("发现式扫描走 MFT 通道：{:?}，{} 条", t0.elapsed(), items.len());
            // 卷打不开（非 NTFS、被占用等）时会拿到空结果，此时仍需兜底
            if !items.is_empty() {
                return items;
            }
            crate::log!("MFT 通道无结果，回退遍历通道");
        } else {
            crate::log!("未提权，发现式扫描走遍历通道（慢得多）");
        }
    }
    let t0 = std::time::Instant::now();
    let items = discover_via_walk(live);
    crate::log!("发现式扫描走遍历通道：{:?}，{} 条", t0.elapsed(), items.len());
    items
}

/// MFT 通道：在内存目录树上 DFS，体积直接读聚合值，无需二次遍历。
#[cfg(windows)]
fn discover_via_mft(
    live: &AtomicBool,
    prescanned: Option<crate::core::disk::MftScan>,
) -> Vec<ScanItem> {
    use crate::platform::windows::mft::scan_volume;
    use crate::platform::windows::volume::list_ntfs_volumes;

    let mut prescanned = prescanned;
    let mut out = Vec::new();
    // 逐卷处理而不是并行扫全部：一棵全盘 MftTree 就可能占数百 MB，
    // 同时持有多个卷的树会让内存峰值失控。处理完一卷立刻释放。
    for vol in list_ntfs_volumes() {
        if !live.load(Ordering::Relaxed) {
            break;
        }
        // 阶段一预解析过的那个卷直接接手，别再解析一遍
        let scan = if prescanned.as_ref().is_some_and(|s| s.volume == vol) {
            match prescanned.take() {
                Some(s) => {
                    crate::log!("卷 {vol}: 复用阶段一已解析的 MFT 树，省去一次全盘解析");
                    s
                }
                None => continue,
            }
        } else {
            match scan_volume(vol, 0) {
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
    tree: &crate::platform::windows::mft::MftTree,
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
            || crate::platform::windows::mft::MftTree::is_ntfs_system_meta(child, name)
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

/// 遍历通道：无管理员权限时的兜底。
fn discover_via_walk(live: &AtomicBool) -> Vec<ScanItem> {
    let roots = code_roots();

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
        assert!(zh.starts_with(marker.label_zh), "中文标签没拼上规则名：{zh}");
        assert!(en.starts_with(marker.label_en), "英文标签没拼上规则名：{en}");
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
        assert!(paths.contains(&rust_proj.join("target")), "有 Cargo.toml 的 target 应被识别");
        assert!(!paths.contains(&doc_dir.join("target")), "无旁证的 target 不该被识别");

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
