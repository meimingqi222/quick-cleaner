//! MFT 通道：在 NTFS 内存树上做 DFS，识别与称重一次完成

use super::{has_sibling, item_label, Marker, MARKERS, SKIP_DIRS};
use crate::core::scanner::ScanItem;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
pub(super) fn discover_via_mft(
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
        let scan = if prescanned.as_ref().is_some_and(|s| s.volume == vol) {
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
                recommended: false,
            });
        }
    }
    out
}

/// MFT 树上的 DFS。命中即止，与遍历通道保持完全一致的判定规则。
#[cfg(windows)]
pub(super) fn collect_mft(
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
