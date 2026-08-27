//! 遍历通道：从代码根目录出发做有界 DFS

use super::{code_roots, has_sibling, item_label, Hit, MARKERS, SKIP_DIRS};
use crate::core::scanner::{measure_dir, ScanItem};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) fn discover_via_walk(live: &AtomicBool) -> Vec<ScanItem> {
    discover_via_walk_roots(&code_roots(), live)
}

/// 只遍历指定的代码根目录，供 macOS 的主目录浅扫复用。
pub(super) fn discover_via_walk_roots(
    roots: &[(PathBuf, usize)],
    live: &AtomicBool,
) -> Vec<ScanItem> {
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
                recommended: false,
                busy: None,
            }
        })
        .filter(|item| item.size > 0)
        .collect()
}

pub(super) fn collect(
    dir: &Path,
    depth: usize,
    max_depth: usize,
    live: &AtomicBool,
    out: &mut Vec<Hit>,
) {
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
