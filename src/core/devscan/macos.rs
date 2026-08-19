//! macOS 索引通道：getattrlistbulk 全量扫描 + FSEvents 增量更新

use super::{has_sibling, item_label, Marker, MARKERS, SKIP_DIRS};
use crate::core::scanner::ScanItem;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// macOS 并行遍历器通道：用 `walk::scan_root` 构建 `SizeTree`，然后在树上 DFS。
///
/// 对应 Windows 侧的 `discover_via_mft`。区别是 macOS 没有 NTFS $MFT，
/// 用并行 `getattrlistbulk` 遍历器代替。不需要提权（TCC 不拦第三方目录）。
#[cfg(not(windows))]
pub(super) fn discover_via_macos_tree(
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
pub(super) fn discover_via_macos_tree_arc(
    live: &AtomicBool,
    prescanned: Option<std::sync::Arc<crate::core::disk::ScanResult>>,
) -> Vec<ScanItem> {
    discover_via_macos_tree(live, prescanned)
}

/// 在 SizeTree 上 DFS 匹配 marker，构建 ScanItem 列表。
#[cfg(not(windows))]
pub(super) fn collect_tree_and_build_items(
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
                recommended: false,
            })
        })
        .collect()
}

/// 加载或构建 macOS 用户目录索引。
///
/// 这是 macOS 扫描的核心入口，被 `scan_fixed`（查表）和 `scan_discovered`（DFS）
/// 共同复用。
#[cfg(not(windows))]
pub(super) fn load_or_build_macos_index(
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
pub(super) fn load_or_build_macos_index_for(
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
                            // 注：metadata_failed 的情况已在上面跳过，
                            // 走到这里说明是变更根 >512 或父节点缺失
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
pub(super) fn spawn_save_index(
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
pub(super) fn full_macos_scan(
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
pub(super) fn refresh_macos_index(
    volume: &crate::core::disk::VolumeId,
    mut scan: crate::core::disk::ScanResult,
    changes: &crate::platform::macos::fsevents::Changes,
    live: &AtomicBool,
) -> Option<crate::core::disk::ScanResult> {
    use crate::platform::macos::walk;

    let mount = volume.mount_point();
    let mut changed_paths: Vec<PathBuf> = changes
        .paths
        .iter()
        .filter(|path| path.starts_with(mount))
        .cloned()
        .collect();
    changed_paths.sort();
    changed_paths.dedup();

    // FSEvents 在事件量过大时会合并事件，合并后的根路径事件会带
    // MustScanSubDirs / UserDropped / KernelDropped 等 flag——这些 flag
    // 已经在 fsevents.rs 里检测并标记为 requires_full_scan=true，调用方
    // 不会进入本函数。能走到这里的根路径事件只是根目录自身的元数据变化
    // （权限、修改时间等），对文件树结构没有影响，可以安全跳过，继续
    // 处理其余子目录变更。
    if changed_paths.iter().any(|path| path == mount) {
        let before = changed_paths.len();
        changed_paths.retain(|path| path != mount);
        crate::log!(
            "refresh_macos_index: 过滤根路径自身事件 {}（{} → {} 条变更）",
            mount.display(),
            before,
            changed_paths.len()
        );
    }

    enum FileChange {
        Upsert(PathBuf, u64, u64), // (path, size, mtime)
        Remove(PathBuf),
    }

    let mut roots = Vec::new();
    let mut file_changes = Vec::new();
    let mut metadata_failed = 0usize;
    for path in changed_paths {
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => roots.push(path),
            Ok(metadata) => {
                use std::os::unix::fs::MetadataExt;
                let size = metadata.blocks().saturating_mul(512);
                let mtime = metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                if scan
                    .tree
                    .find_node_by_path(path.parent().unwrap_or(mount))
                    .is_none()
                {
                    roots.push(path.parent().unwrap_or(mount).to_path_buf());
                } else {
                    file_changes.push(FileChange::Upsert(path, size, mtime));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                file_changes.push(FileChange::Remove(path));
            }
            Err(_) => {
                // 权限或瞬时 I/O 错误不能被解释成“文件已删除”，保留旧值。
                metadata_failed += 1;
            }
        }
    }

    // 权限或瞬时 I/O 失败时跳过该路径，保留旧值。不再因为个别路径失败
    // 就放弃整轮增量——12 万个变更路径中 1 个失败就回退全量扫描（100+ 秒）
    // 完全不合理。被跳过的路径会在下次全量扫描或用户手动「重新分析」时修正。
    if metadata_failed > 0 {
        crate::log!(
            "refresh_macos_index: {} 个路径读取元数据失败，跳过这些路径继续增量",
            metadata_failed
        );
    }

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
    // 太多彼此独立的变化目录时，重扫局部区域反而比一次完整扫描更慢。
    if roots.len() > 512 {
        crate::log!(
            "refresh_macos_index: 独立变更根 {} 个 > 512，放弃增量",
            roots.len()
        );
        return None;
    }

    // 被目录重扫覆盖的文件无需先就地修改，否则新追加的文件节点不在旧 CSR
    // 子数组里，随后移除父子树时无法一并标记，会留下孤立节点。
    let mut files_updated = 0usize;
    let mut paths_removed = 0usize;
    for change in file_changes {
        let path = match &change {
            FileChange::Upsert(path, _, _) | FileChange::Remove(path) => path,
        };
        if roots.iter().any(|root| path.starts_with(root)) {
            continue;
        }
        match change {
            FileChange::Upsert(path, size, mtime) => {
                if scan.tree.upsert_file_with_mtime(&path, size, mtime) {
                    files_updated += 1;
                }
            }
            FileChange::Remove(path) => {
                if let Some(node) = scan.tree.find_node_by_path(&path) {
                    scan.tree.remove_subtree_inplace(node);
                    paths_removed += 1;
                }
            }
        }
    }
    crate::log!(
        "refresh_macos_index: 文件就地更新 {}，删除 {}，元数据失败 {}，目录重扫 {}",
        files_updated,
        paths_removed,
        metadata_failed,
        roots.len()
    );

    if roots.is_empty() {
        scan.tree.rebuild_child_arrays();
        refresh_scan_totals(&mut scan);
        return Some(scan);
    }

    // 并行扫描所有独立变更根，然后串行追加到树。
    // 之前串行扫描 65 个根要 5.7s（Notion 2s + Edge Cache 2s + Telegram 600ms），
    // 并行后墙钟时间等于最慢的那个子树。
    //
    // 每个 FSEvents 变更根都必须重扫。之前为了速度跳过小目录、超大目录和
    // iCloud Drive，但“保留旧数据”会让已删除文件永久留在索引里，也会漏掉
    // 新文件。磁盘透镜展示的是文件系统事实，不能用已知错误换取刷新速度。
    use rayon::prelude::*;
    let t_par = std::time::Instant::now();
    struct SubtreeResult {
        root: PathBuf,
        scan: Option<crate::core::disk::ScanResult>,
    }
    let scan_results: Vec<SubtreeResult> = roots
        .par_iter()
        .filter_map(|root| {
            if !live.load(Ordering::Relaxed) {
                return None;
            }
            if !root.exists() {
                return Some(SubtreeResult {
                    root: root.clone(),
                    scan: None,
                });
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
                        scan: Some(s),
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
    let expected = roots.len();
    if scan_results.len() < expected {
        crate::log!(
            "refresh_macos_index: {}/{} 子树扫描失败，放弃增量",
            expected - scan_results.len(),
            expected
        );
        return None;
    }
    crate::log!(
        "refresh_macos_index: 并行扫描 {} 个子树，总耗时 {:?}",
        scan_results.len(),
        t_par.elapsed()
    );

    // 串行追加到树——append_subtree 会修改树结构，不能并行
    for sr in scan_results {
        let Some(subtree) = sr.scan else {
            if let Some(old_node) = scan.tree.find_node_by_path(&sr.root) {
                scan.tree.remove_subtree_inplace(old_node);
            }
            continue;
        };
        let root_name = sr
            .root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let parent_path = sr.root.parent().unwrap_or(mount);
        let parent_idx = match scan.tree.find_node_by_path(parent_path) {
            Some(parent) => parent,
            None => {
                crate::log!(
                    "refresh_macos_index: 父目录 {} 不在树中，放弃增量更新 {}",
                    parent_path.display(),
                    sr.root.display()
                );
                return None;
            }
        };

        // 在树中定位旧节点并就地移除
        if let Some(old_node) = scan.tree.find_node_by_path(&sr.root) {
            scan.tree.remove_subtree_inplace(old_node);
        }
        scan.tree
            .append_subtree(parent_idx, &subtree.tree, &root_name);
    }

    // 重建 CSR 子节点索引（一次 O(n) 整数操作，无 PathBuf 分配）
    scan.tree.rebuild_child_arrays();

    refresh_scan_totals(&mut scan);
    Some(scan)
}

#[cfg(not(windows))]
pub(super) fn refresh_scan_totals(scan: &mut crate::core::disk::ScanResult) {
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
}

/// macOS SizeTree 上的 DFS，与遍历通道 `collect` 保持完全一致的判定规则。
#[cfg(not(windows))]
pub(super) fn collect_tree(
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
