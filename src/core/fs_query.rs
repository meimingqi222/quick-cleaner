//! 统一文件系统检索与索引抽象层 (FSIndexEngine)
//!
//! 将 Windows (NTFS $MFT 内存表)、macOS (getattrlistbulk + 缓存索引树) 与
//! 实时多线程遍历 (LiveWalk) 抽象为统一门面，上层业务无需感知底层平台差异。

use crate::core::disk::SizeTree;
use crate::core::safety::is_protected;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

/// 统一文件检索过滤条件
#[derive(Clone, Debug)]
pub struct QueryFilter {
    pub roots: Vec<PathBuf>,
    pub max_depth: usize,
    pub min_size: u64,
    pub max_size: u64,
    pub extensions: Option<HashSet<String>>,
    pub include_dirs: bool,
}

impl Default for QueryFilter {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            max_depth: usize::MAX,
            min_size: 0,
            max_size: u64::MAX,
            extensions: None,
            include_dirs: false,
        }
    }
}

impl QueryFilter {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            ..Default::default()
        }
    }

    pub fn max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    pub fn size_range(mut self, min: u64, max: u64) -> Self {
        self.min_size = min;
        self.max_size = max;
        self
    }

    pub fn extensions(mut self, exts: &[&str]) -> Self {
        self.extensions = Some(exts.iter().map(|s| s.to_lowercase()).collect());
        self
    }

    pub fn include_dirs(mut self, include: bool) -> Self {
        self.include_dirs = include;
        self
    }

    pub fn matches_file(&self, path: &Path, size: u64) -> bool {
        if size < self.min_size || size > self.max_size {
            return false;
        }
        if let Some(ref exts) = self.extensions {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if !exts.contains(&ext) {
                return false;
            }
        }
        true
    }
}

/// 检索出的统一文件元数据
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedFile {
    pub path: PathBuf,
    pub size: u64,
    pub mtime: u64,
    pub is_dir: bool,
}

/// 通用文件系统检索 Trait
pub trait FileIndexQuery: Send + Sync {
    /// 1. 范围文件查询（支持深度、体积范围、扩展名过滤，带中断检查）
    fn query_files(&self, filter: &QueryFilter, live: &AtomicBool) -> Vec<IndexedFile>;

    /// 2. 毫秒级单路径/目录体积称重 (O(1) 查表或极速遍历)
    ///
    /// 返回 `(size, files, newest_mtime)`（字节 / 文件数 / 子树内最新 mtime 秒）。
    /// 路径不存在、符号链接或非普通文件返回 `None`。
    fn measure_path(&self, path: &Path, live: &AtomicBool) -> Option<(u64, u64, u64)>;

    /// 3. 大文件 Top-N 极速提取 (专门优化大文件视图)
    fn query_large_files(
        &self,
        roots: &[PathBuf],
        min_size: u64,
        limit: usize,
        live: &AtomicBool,
    ) -> Vec<IndexedFile>;

    /// 4. 重复文件候选分桶（自动按 size 归类，筛选 count >= 2 的体积桶）
    /// 每个桶值是 `(路径, 修改时间)`，调用方无需再调 `std::fs::metadata` 取 mtime。
    fn query_duplicate_buckets(
        &self,
        roots: &[PathBuf],
        min_size: u64,
        live: &AtomicBool,
    ) -> HashMap<u64, Vec<(PathBuf, u64)>>;
}

/// 统一检索门面引擎 (FSIndexEngine)
pub struct FSIndexEngine<'a> {
    tree: Option<&'a SizeTree>,
}

impl<'a> FSIndexEngine<'a> {
    pub fn new(tree: Option<&'a SizeTree>) -> Self {
        Self { tree }
    }
}

impl<'a> FileIndexQuery for FSIndexEngine<'a> {
    fn query_files(&self, filter: &QueryFilter, live: &AtomicBool) -> Vec<IndexedFile> {
        let mut results = Vec::new();

        // 1. 优先尝试从 SizeTree 内存索引直接秒级提取 (< 2ms)
        if let Some(t) = self.tree {
            let mut cache = HashMap::new();
            for root in &filter.roots {
                if !live.load(Ordering::Relaxed) {
                    break;
                }
                if let Some(root_node) = t.find_node_by_path(root) {
                    let files = t.collect_subtree_files(
                        root_node,
                        filter.max_depth,
                        filter.min_size,
                        filter.max_size,
                    );
                    for (node_idx, size, tree_mtime) in files {
                        if !live.load(Ordering::Relaxed) {
                            break;
                        }
                        let path_str = t.path_of_with(node_idx, &mut cache);
                        let path = PathBuf::from(path_str);
                        if is_protected(&path) {
                            continue;
                        }
                        if filter.matches_file(&path, size) {
                            let mtime = if tree_mtime > 0 {
                                tree_mtime
                            } else {
                                std::fs::metadata(&path)
                                    .ok()
                                    .and_then(|m| m.modified().ok())
                                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                                    .map(|d| d.as_secs())
                                    .unwrap_or(0)
                            };

                            results.push(IndexedFile {
                                path,
                                size,
                                mtime,
                                is_dir: false,
                            });
                        }
                    }
                }
            }
            if !results.is_empty() {
                crate::log!(
                    "[FSQuery] ⚡ 命中内存索引 (SizeTree): 检索 {} 个根路径, 匹配 {} 个文件",
                    filter.roots.len(),
                    results.len()
                );
                return results;
            }
        }

        // 2. 回退：无索引或未在索引中找到时，透明回退到多线程 LiveWalker
        for root in &filter.roots {
            if !live.load(Ordering::Relaxed) {
                break;
            }
            if !root.exists() {
                continue;
            }

            let walker = WalkDir::new(root)
                .max_depth(filter.max_depth)
                .follow_links(false)
                .into_iter();

            for entry in walker.filter_entry(|e| !is_ignored_scan_dir(e.path())) {
                if !live.load(Ordering::Relaxed) {
                    break;
                }
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                let is_dir = entry.file_type().is_dir();
                if is_dir && !filter.include_dirs {
                    continue;
                }
                if !is_dir && !entry.file_type().is_file() {
                    continue;
                }

                let path = entry.path().to_path_buf();
                if is_protected(&path) {
                    continue;
                }

                let metadata = match entry.metadata() {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                let size = metadata.len();
                if filter.matches_file(&path, size) {
                    let mtime = metadata
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);

                    results.push(IndexedFile {
                        path,
                        size,
                        mtime,
                        is_dir,
                    });
                }
            }
        }

        crate::log!(
            "[FSQuery] 🔍 实时文件系统遍历: 扫描 {} 个根路径, 检索到 {} 个匹配文件",
            filter.roots.len(),
            results.len()
        );

        results
    }

    fn measure_path(&self, path: &Path, live: &AtomicBool) -> Option<(u64, u64, u64)> {
        // 1. 有树时走 O(1) 表查（带卷/挂载点守卫，避免 UNC 等跨卷路径误命中）。
        //    表查通道没有子树最新 mtime，last_modified 返回 0，与 MFT 通道口径一致。
        if let Some(t) = self.tree {
            if let Some((size, files)) = crate::core::scanner::measure_via_tree(t, path) {
                return Some((size, files, 0));
            }
        }

        // 2. 回退：scanner::measure_target 并行遍历（allocated 口径 / 跳 symlink / 并行）
        crate::core::scanner::measure_target(path, live)
    }

    fn query_large_files(
        &self,
        roots: &[PathBuf],
        min_size: u64,
        limit: usize,
        live: &AtomicBool,
    ) -> Vec<IndexedFile> {
        let filter = QueryFilter::new(roots.to_vec())
            .max_depth(20)
            .size_range(min_size, u64::MAX);

        let mut files = self.query_files(&filter, live);
        // 用 select_nth_unstable_by_key 做 partial sort：O(n) 划分 + O(limit log limit) 排序，
        // 比全量 sort 的 O(n log n) 快很多（n=几十万时差距明显）。
        if files.len() > limit {
            files.select_nth_unstable_by_key(limit - 1, |f| std::cmp::Reverse(f.size));
            files.truncate(limit);
            files.sort_by_key(|f| std::cmp::Reverse(f.size));
        } else {
            files.sort_by_key(|f| std::cmp::Reverse(f.size));
        }
        files
    }

    fn query_duplicate_buckets(
        &self,
        roots: &[PathBuf],
        min_size: u64,
        live: &AtomicBool,
    ) -> HashMap<u64, Vec<(PathBuf, u64)>> {
        let filter = QueryFilter::new(roots.to_vec())
            .max_depth(20)
            .size_range(min_size, u64::MAX);

        let files = self.query_files(&filter, live);
        let mut buckets: HashMap<u64, Vec<(PathBuf, u64)>> = HashMap::new();

        for file in files {
            buckets
                .entry(file.size)
                .or_default()
                .push((file.path, file.mtime));
        }

        // 过滤掉只有单个文件的桶
        buckets.retain(|_, list| list.len() >= 2);
        buckets
    }
}

fn is_ignored_scan_dir(p: &Path) -> bool {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.starts_with('.') {
        return true;
    }
    let s = name.to_lowercase();
    matches!(
        s.as_str(),
        "node_modules"
            | "target"
            | "dist"
            | "build"
            | "out"
            | "bin"
            | "obj"
            | "pkg"
            | "vendor"
            | "pods"
            | "deriveddata"
            | "bower_components"
            | "venv"
            | "env"
            | "__pycache__"
            | "library"
            | "appdata"
            | "application data"
            | "application support"
            | "local settings"
            | "cache"
            | "caches"
            | "temp"
            | "tmp"
            | "logs"
            | "gems"
            | "site-packages"
            | "docs"
            | "doc"
            | "documentation"
            | "manual"
            | "sdk"
            | "javadoc"
            | "$recycle.bin"
            | "system volume information"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_filter_matches() {
        let filter = QueryFilter::default()
            .size_range(100, 1000)
            .extensions(&["png", "jpg"]);

        assert!(filter.matches_file(Path::new("test.png"), 500));
        assert!(filter.matches_file(Path::new("photo.JPG"), 100));
        assert!(!filter.matches_file(Path::new("test.txt"), 500));
        assert!(!filter.matches_file(Path::new("test.png"), 50));
        assert!(!filter.matches_file(Path::new("test.png"), 2000));
    }

    #[test]
    fn test_fs_index_engine_live_walk() {
        let temp_dir = std::env::temp_dir().join("qc_fs_query_test");
        let _ = std::fs::create_dir_all(&temp_dir);
        let file_a = temp_dir.join("file_a.txt");
        let file_b = temp_dir.join("file_b.txt");
        let _ = std::fs::write(&file_a, "hello world 123");
        let _ = std::fs::write(&file_b, "hello world 123");

        let live = AtomicBool::new(true);
        let engine = FSIndexEngine::new(None);

        let filter = QueryFilter::new(vec![temp_dir.clone()]);
        let files = engine.query_files(&filter, &live);
        assert!(files.len() >= 2);

        let measure = engine.measure_path(&temp_dir, &live);
        assert!(measure.is_some());
        let (sz, count, newest) = measure.unwrap();
        assert!(sz >= 30);
        assert!(count >= 2);
        assert!(newest > 0, "应有最新 mtime");

        let buckets = engine.query_duplicate_buckets(&[temp_dir.clone()], 1, &live);
        assert!(!buckets.is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
