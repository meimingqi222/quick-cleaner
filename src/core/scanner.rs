//! 垃圾扫描与体积统计引擎

use crate::core::categories::{CategoryId, ScanTarget};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

#[derive(Clone, Debug)]
pub struct ScanItem {
    pub path: PathBuf,
    pub label: String,
    pub size: u64,
    pub file_count: u64,
    pub category: CategoryId,
    pub last_modified: u64,
}

#[derive(Clone, Debug)]
pub struct CategorySummary {
    pub category: CategoryId,
    pub total_size: u64,
    pub items: Vec<ScanItem>,
}

/// 对一批目标做并行扫描。
///
/// 并行有两层：目标之间（`par_iter`）和单棵树内部（`walk` 里对子目录再 `par_iter`）。
pub fn scan_all(targets: &[ScanTarget], live: &AtomicBool) -> Vec<CategorySummary> {
    let results: Vec<ScanItem> = targets
        .par_iter()
        .filter(|t| t.path.exists())
        .filter_map(|t| scan_dir(&t.path, &t.label, t.category, live))
        .collect();

    // 按类别聚合
    let mut out: Vec<CategorySummary> = Vec::new();
    for cat in CategoryId::ALL {
        let items: Vec<ScanItem> = results
            .iter()
            .filter(|it| it.category == cat)
            .cloned()
            .collect();
        let total: u64 = items.iter().map(|it| it.size).sum();
        out.push(CategorySummary {
            category: cat,
            total_size: total,
            items,
        });
    }
    out
}

/// 一棵子树的累计结果。
#[derive(Clone, Copy, Default)]
struct Acc {
    size: u64,
    files: u64,
    newest: u64,
}

impl Acc {
    fn merge(mut self, other: Acc) -> Acc {
        self.size += other.size;
        self.files += other.files;
        self.newest = self.newest.max(other.newest);
        self
    }
}

/// 扫描单个目录：大小 = 子树全部文件大小；文件数 = 全部文件数。
fn scan_dir(dir: &Path, label: &str, category: CategoryId, live: &AtomicBool) -> Option<ScanItem> {
    if !live.load(Ordering::Relaxed) {
        return None;
    }
    let acc = walk(dir, live);
    Some(ScanItem {
        path: dir.to_path_buf(),
        label: label.to_string(),
        size: acc.size,
        file_count: acc.files,
        category,
        last_modified: acc.newest,
    })
}

/// 递归遍历，子目录之间用 rayon 并行。
fn walk(dir: &Path, live: &AtomicBool) -> Acc {
    let mut acc = Acc::default();
    if !live.load(Ordering::Relaxed) {
        return acc;
    }

    let Ok(rd) = std::fs::read_dir(dir) else {
        return acc;
    };

    let mut subdirs: Vec<PathBuf> = Vec::new();
    for entry in rd.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            subdirs.push(entry.path());
        } else if ft.is_file() {
            // 忽略 desktop.ini（避免空目录显示噪点）
            if entry.file_name().to_string_lossy().eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            acc.files += 1;
            if let Ok(md) = entry.metadata() {
                acc.size += md.len();
                if let Ok(m) = md.modified() {
                    let t = m.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                    acc.newest = acc.newest.max(t);
                }
            }
        }
    }

    if subdirs.is_empty() {
        return acc;
    }
    let sub = subdirs
        .par_iter()
        .map(|p| walk(p, live))
        .reduce(Acc::default, Acc::merge);
    acc.merge(sub)
}
