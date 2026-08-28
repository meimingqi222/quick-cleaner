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
use crate::core::scanner::ScanItem;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

#[cfg(not(windows))]
mod macos;
#[cfg(windows)]
mod mft;
mod walk;

#[cfg(not(windows))]
use macos::{discover_via_macos_tree, discover_via_macos_tree_arc};
#[cfg(not(windows))]
pub use macos::{load_or_build_macos_root_index, remember_macos_root_index};
#[cfg(windows)]
use mft::discover_via_mft;
use walk::discover_via_walk;

/// 具名代码根目录（`~/code`、`D:\repos` …）往下最多走多少层。
///
/// 6 层足够覆盖 `~/code/<组织>/<仓库>/<子包>/<模块>/node_modules` 这种
/// 常见深度，再深就得不偿失了。
pub(super) const NAMED_ROOT_DEPTH: usize = 6;

/// 用户主目录本身只浅扫。
///
/// `~` 底下混着 Downloads、文档、各种应用私有目录，按 6 层扫会把整轮
/// 扫描拖到分钟级。放仓库在 `~` 下的人一般也就一两层深。
pub(super) const HOME_DEPTH: usize = 2;

/// 遍历时直接跳过的目录名（大小写不敏感）。
///
/// `.git` 里全是对象文件，走进去纯属浪费；`AppData` / `Windows` 之类由
/// 固定路径规则负责，不该让发现式扫描重复趟一遍。
pub(super) const SKIP_DIRS: &[&str] = &[
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
pub(super) struct Marker {
    /// 目录名（小写比较）
    pub(super) dir: &'static str,
    /// 给用户看的说明（中文）
    pub(super) label_zh: &'static str,
    /// 给用户看的说明（英文）
    pub(super) label_en: &'static str,
    pub(super) category: CategoryId,
    /// 需要在**同级**看到其中任意一个才算数；空数组表示名字本身足够特征化。
    /// 以 `.` 开头的条目按扩展名匹配（如 `.csproj`）。
    pub(super) sibling_any: &'static [&'static str],
}

/// CACHEDIR.TAG 规范签名（<https://bford.info/cachedir/>）：文件必须以
/// 这 43 字节开头，后面可跟任意注释。越来越多的工具会主动写这个文件
/// 声明「我是缓存」——Python 3.8+ 的 `__pycache__`、Rust sccache、各类
/// 应用缓存目录都是。自声明是比按名字猜更强的信号，且正是 Mole 用来
/// 识别缓存目录的机制。
pub(super) const CACHEDIR_SIGNATURE: &[u8] =
    b"Signature: 8a477f597d28d172789f06886806bc55";

/// CACHEDIR.TAG 命中用的伪 Marker。不进 [`MARKERS`] 表（`dir` 占位符
/// 不参与名字匹配），只在三通道各自的 CACHEDIR 分支里显式引用。
pub(super) static CACHEDIR_MARKER: Marker = Marker {
    dir: "<cachedir>",
    label_zh: "缓存目录（CACHEDIR.TAG）",
    label_en: "Cache directory (CACHEDIR.TAG)",
    category: CategoryId::DevBuild,
    sibling_any: &[],
};

/// 目录里有没有一份**签名合法**的 `CACHEDIR.TAG`。
///
/// 签名验证是关键，不能只看文件存在：同名伪造（用户目录恰好放了个空
/// 的 `CACHEDIR.TAG`）过不了签名关，而「把任意目录声明成缓存」正是
/// 要防的误判。只读前 256 字节：签名在文件开头，后面是可选注释，
/// 没必要读全文。读失败（权限、竞态删除）按「不是」处理——这只是
/// 发现信号，判错的代价是多下钻一层，不是放行删除。
pub(super) fn has_cachedir_tag(dir: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(dir.join("CACHEDIR.TAG")) else {
        return false;
    };
    let mut buf = [0u8; 256];
    let n = file.read(&mut buf).unwrap_or(0);
    buf[..n].starts_with(CACHEDIR_SIGNATURE)
}

pub(super) const MARKERS: &[Marker] = &[
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
pub(super) const CODE_ROOT_NAMES: &[&str] = &[
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
    for vol in crate::platform::windows::volume::list_volumes() {
        // VolumeId 的 Display 是 "C:"（带冒号），直接拼会得到 "C::\name"。
        // 取盘符再拼才是正确的 "C:\name"。
        if let Some(letter) = vol.drive_letter() {
            for name in CODE_ROOT_NAMES {
                roots.push((
                    PathBuf::from(format!(r"{letter}:\{name}")),
                    NAMED_ROOT_DEPTH,
                ));
            }
        }
    }

    roots.retain(|(p, _)| p.is_dir());
    roots.sort();
    roots.dedup_by(|a, b| a.0 == b.0);
    roots
}

pub(super) struct Hit {
    pub(super) path: PathBuf,
    /// 命中的规则本身。标签是双语的，直到建 `ScanItem` 时才展开。
    pub(super) marker: &'static Marker,
}

/// 发现所有开发垃圾目录并测算体积。
///
/// 有管理员权限时走 MFT，否则退回文件系统遍历。
///
/// `prescanned` 是阶段一为了查表已经解析好的那个卷。它本来跑完就要被丢掉，
/// 接过来直接用能省掉一整次全盘 MFT 解析（本机 C 盘 3.3 秒）。所有权在这里
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

/// 给命中的目录生成双语标签：规则名 + 短路径。
pub(super) fn item_label(marker: &Marker, path: &Path) -> Text {
    let sp = short_path(path);
    Text::new(
        format!("{} · {sp}", marker.label_zh),
        format!("{} · {sp}", marker.label_en),
    )
}

pub(super) fn has_sibling(file_names: &[String], required: &[&str]) -> bool {
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

pub(super) fn short_path(path: &Path) -> String {
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
    #[cfg(not(windows))]
    use macos::{collect_tree, refresh_macos_index};
    use walk::collect;

    /// 签名验证是 CACHEDIR.TAG 判定的核心：文件不存在、空文件、内容
    /// 伪造都不能算命中；只有以规范签名开头的才算。
    #[test]
    fn cachedir_tag_requires_valid_signature() {
        let tmp = std::env::temp_dir().join("qc_cachedir_sig");
        let dir = tmp.join("cache-dir");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&dir).unwrap();

        // 没有文件
        assert!(!has_cachedir_tag(&dir));
        // 空文件（同名伪造最常见的形状）
        std::fs::write(dir.join("CACHEDIR.TAG"), b"").unwrap();
        assert!(!has_cachedir_tag(&dir));
        // 内容像但不是签名
        std::fs::write(dir.join("CACHEDIR.TAG"), b"Signature: deadbeef\n").unwrap();
        assert!(!has_cachedir_tag(&dir));
        // 合法签名 + 注释（规范允许签名后跟任意注释）
        let mut valid = CACHEDIR_SIGNATURE.to_vec();
        valid.extend_from_slice(b"\n# managed by some tool");
        std::fs::write(dir.join("CACHEDIR.TAG"), valid).unwrap();
        assert!(has_cachedir_tag(&dir));

        let _ = std::fs::remove_dir_all(&tmp);
    }

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

    /// 小目录过去会被增量策略直接跳过并“保留旧数据”，导致已删除文件在
    /// 重启或重新分析后复活。任何规模的变更根都必须以当前文件系统重建。
    #[cfg(not(windows))]
    #[test]
    fn incremental_refresh_removes_deleted_file_from_small_directory() {
        use crate::core::disk::VolumeId;
        use crate::platform::macos::{fsevents::Changes, walk};

        let base = std::env::temp_dir().join("qc_devscan_incremental_delete");
        let changed_dir = base.join("small");
        let deleted = changed_dir.join("gone.bin");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&changed_dir).unwrap();
        std::fs::write(&deleted, vec![b'x'; 4096]).unwrap();

        let live = AtomicBool::new(true);
        let volume = VolumeId::from_mount_point(base.clone());
        let original = walk::scan_root(&base, volume.clone(), &live).unwrap();
        assert!(original.tree.find_node_by_path(&deleted).is_some());

        std::fs::remove_file(&deleted).unwrap();
        let changes = Changes {
            paths: vec![deleted.clone()],
            last_event_id: 1,
            requires_full_scan: false,
            full_scan_reason: None,
            filtered_cache_events: 0,
            raw_event_count: 1,
        };
        let refreshed = refresh_macos_index(&volume, original, &changes, &live)
            .expect("小目录删除应能增量更新");

        assert!(
            refreshed.tree.find_node_by_path(&deleted).is_none(),
            "已删除文件不能残留在增量索引里"
        );
        assert_eq!(refreshed.file_count, 0);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(not(windows))]
    #[test]
    fn incremental_refresh_skips_benign_root_event() {
        use crate::core::disk::VolumeId;
        use crate::platform::macos::{fsevents::Changes, walk};

        // requires_full_scan=false 时的根路径事件只是根目录自身的元数据
        // 变化（权限/修改时间），不是 FSEvents 合并事件。应该跳过该条事件、
        // 继续返回 Some(scan)，而不是放弃整个增量。
        let base = std::env::temp_dir().join("qc_devscan_root_event");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let live = AtomicBool::new(true);
        let volume = VolumeId::from_mount_point(base.clone());
        let original = walk::scan_root(&base, volume.clone(), &live).unwrap();
        let original_records = original.records_read;
        let changes = Changes {
            paths: vec![base.clone()],
            last_event_id: 2,
            requires_full_scan: false,
            full_scan_reason: None,
            filtered_cache_events: 0,
            raw_event_count: 1,
        };

        let refreshed = refresh_macos_index(&volume, original, &changes, &live)
            .expect("benign root event should not abort incremental refresh");
        assert_eq!(
            refreshed.records_read, original_records,
            "根路径元数据事件不应改变记录数"
        );
        let _ = std::fs::remove_dir_all(base);
    }

    /// 单个路径元数据读取失败时，增量更新应跳过该路径继续处理，
    /// 而不是放弃整个增量回退全量扫描。
    #[cfg(not(windows))]
    #[test]
    fn incremental_refresh_skips_metadata_errors() {
        use crate::core::disk::VolumeId;
        use crate::platform::macos::{fsevents::Changes, walk};
        use std::os::unix::ffi::OsStringExt;

        let base = std::env::temp_dir().join("qc_devscan_metadata_error");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let live = AtomicBool::new(true);
        let volume = VolumeId::from_mount_point(base.clone());
        let original = walk::scan_root(&base, volume.clone(), &live).unwrap();
        let invalid = base.join(std::ffi::OsString::from_vec(b"invalid\0path".to_vec()));
        let changes = Changes {
            paths: vec![invalid],
            last_event_id: 3,
            requires_full_scan: false,
            full_scan_reason: None,
            filtered_cache_events: 0,
            raw_event_count: 1,
        };

        // 修复后：单个 metadata 失败不再放弃整个增量，应返回 Some。
        let refreshed = refresh_macos_index(&volume, original, &changes, &live)
            .expect("单个路径失败不应放弃整个增量更新");
        // 原始索引内容应保留（没有因失败路径而丢失）
        assert!(refreshed.tree.valid(refreshed.tree.root()));
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(not(windows))]
    #[test]
    fn incremental_refresh_updates_file_without_rescanning_parent() {
        use crate::core::disk::VolumeId;
        use crate::platform::macos::{fsevents::Changes, walk};
        use std::os::unix::fs::MetadataExt;

        let base = std::env::temp_dir().join("qc_devscan_incremental_file");
        let changed = base.join("direct.bin");
        let untouched = base.join("large-subtree/keep.bin");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(untouched.parent().unwrap()).unwrap();
        std::fs::write(&changed, vec![b'x'; 4096]).unwrap();
        std::fs::write(&untouched, vec![b'y'; 4096]).unwrap();

        let live = AtomicBool::new(true);
        let volume = VolumeId::from_mount_point(base.clone());
        let original = walk::scan_root(&base, volume.clone(), &live).unwrap();
        std::fs::write(&changed, vec![b'z'; 16 * 1024]).unwrap();
        let expected_size = std::fs::metadata(&changed).unwrap().blocks() * 512;
        let changes = Changes {
            paths: vec![changed.clone()],
            last_event_id: 1,
            requires_full_scan: false,
            full_scan_reason: None,
            filtered_cache_events: 0,
            raw_event_count: 1,
        };

        let refreshed = refresh_macos_index(&volume, original, &changes, &live).unwrap();
        let changed_node = refreshed.tree.find_node_by_path(&changed).unwrap();
        assert_eq!(refreshed.tree.size_of(changed_node), expected_size);
        assert!(
            refreshed.tree.find_node_by_path(&untouched).is_some(),
            "更新直属文件不能影响未变化的庞大子树"
        );
        assert_eq!(refreshed.file_count, 2);

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
