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
            // 发现式扫描命中的都是 DevBuild（`removes_directory() == true`，
            // 见 `categories::mod`），删除时验的是根身份，所以这里值得
            // 多付一次 stat 把身份取到——树上的聚合体积/计数不含
            // Metadata，没法像 scanner::scan_fixed_inner 那样顺手拿。
            let identity = crate::core::model::capture_identity(&path)?;
            Some(ScanItem {
                label: item_label(marker, &path),
                path,
                size,
                file_count: tree.file_count_of(idx),
                category: marker.category,
                last_modified: 0,
                recommended: false,
                busy: None,
                identity: Some(identity),
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

/// 进程内整盘索引只加载一次。垃圾扫描、文件搜索、磁盘透镜会同时
/// 调进来，以前各自 deserialize 1600 万节点，峰值直接 ×3。
static INDEX_BUILD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
struct CachedRootIndex {
    mount: std::path::PathBuf,
    scan: std::sync::Arc<crate::core::disk::ScanResult>,
    last_event_id: u64,
}

static CACHED_ROOT_INDEX: std::sync::Mutex<Option<CachedRootIndex>> = std::sync::Mutex::new(None);

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
    let _build_guard = INDEX_BUILD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let cached = CACHED_ROOT_INDEX
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .filter(|cached| cached.mount == root)
        .map(|cached| (cached.scan.clone(), cached.last_event_id));
    let (scan, last_event_id) = match cached {
        Some((scan, last_event_id)) => {
            crate::log!(
                "校验进程内 {} 索引缓存：{} 条记录，事件水位 {}",
                label,
                scan.records_read,
                last_event_id
            );
            refresh_cached_macos_index(
                root,
                label,
                scan,
                last_event_id,
                live,
                RefreshBudget::Interactive,
            )?
        }
        None => load_or_build_macos_index_for_uncached(root, label, live)?,
    };
    if live.load(Ordering::Relaxed) {
        *CACHED_ROOT_INDEX.lock().unwrap_or_else(|e| e.into_inner()) = Some(CachedRootIndex {
            mount: root.to_path_buf(),
            scan: scan.clone(),
            last_event_id,
        });
        spawn_prune_orphan_indexes(&scan.volume);
        spawn_watermark_ticker();
    }
    Some(scan)
}

/// 空转期间定时把事件水位往前推，别让它旧到掉进悬崖。
///
/// 索引只在用户打开磁盘透镜 / 搜索 / 垃圾扫描时才刷新（见本模块三个调用
/// 方），进程空转几小时水位就在原地不动。实测（本机，`/`，用
/// `cargo run --example fseprobe --features fseprobe`）回放耗时对事件 ID
/// 间隔近似线性，约 **2 秒 / 百万事件 ID**：
///
/// | 空转 | 事件 ID 差 | 回放耗时 | 结果 |
/// |---|---|---|---|
/// | 0 | 0 | 12.8ms | 正常 |
/// | 25 分钟 | 1.48M | 2.84s | 正常增量 |
/// | 1 小时 | 3.48M | 6.68s | 正常增量 |
/// | 2 小时 | 6.98M | 14.4s | 正常增量 |
/// | 2.9 小时 | 20.7M | 撞上 30s 超时 | 退化成 ~57s 整盘重建 |
///
/// 一次实测的 96.9 秒磁盘透镜就是最后那行：空转近 3 小时，回放超时，转全量。
/// 15 分钟一轮把间隔钉在 ~2 秒量级，离 30s 超时和内核历史丢弃都还很远。
#[cfg(not(windows))]
const WATERMARK_TICK: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// 启动水位推进线程，每进程一次。
///
/// 只在**已经有一份索引**之后启动——没有索引时无水位可推，更不该由后台
/// 线程去触发首次全量扫描。因此挂在 `load_or_build_macos_index_for` 成功
/// 之后，和 `spawn_prune_orphan_indexes` 同一个位置。
#[cfg(not(windows))]
fn spawn_watermark_ticker() {
    static STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    std::thread::spawn(|| loop {
        std::thread::sleep(WATERMARK_TICK);
        advance_cached_watermark();
    });
}

/// 推进一次进程内缓存索引的水位。
///
/// 三条自我约束，缺一条这个线程就会变成用户的负担而不是帮助：
///
/// 1. **拿不到构建锁就跳过本轮**。用户发起的扫描正在跑时不去抢，等下一轮。
/// 2. **没有缓存索引就跳过**。首次全量扫描只能由用户的动作触发。
/// 3. **`RefreshBudget::Background`**：增量走不通就放弃，不在后台跑整盘重建。
///
/// 反过来，本轮真的在跑时，用户此刻发起扫描会等这次回放结束——这不是退化：
/// 那份回放的钱他自己也要付，而且付完还要再加一次整盘重建。
#[cfg(not(windows))]
fn advance_cached_watermark() {
    let Ok(_build_guard) = INDEX_BUILD_LOCK.try_lock() else {
        crate::log!("水位推进：构建锁被占用（用户扫描进行中），跳过本轮");
        return;
    };
    let cached = CACHED_ROOT_INDEX
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|c| (c.mount.clone(), c.scan.clone(), c.last_event_id));
    let Some((mount, scan, last_event_id)) = cached else {
        return;
    };

    let live = AtomicBool::new(true);
    let t0 = std::time::Instant::now();
    let Some((scan, advanced)) = refresh_cached_macos_index(
        &mount,
        "整盘",
        scan,
        last_event_id,
        &live,
        RefreshBudget::Background,
    ) else {
        return;
    };
    if advanced == last_event_id {
        return;
    }
    crate::log!(
        "水位推进：{} → {}（{} 条记录，耗时 {:?}）",
        last_event_id,
        advanced,
        scan.records_read,
        t0.elapsed()
    );
    let mut slot = CACHED_ROOT_INDEX.lock().unwrap_or_else(|e| e.into_inner());
    // 期间用户可能换了卷或重建过索引，只在还是同一份时才写回。
    if slot
        .as_ref()
        .is_some_and(|c| c.mount == mount && c.last_event_id == last_event_id)
    {
        *slot = Some(CachedRootIndex {
            mount,
            scan,
            last_event_id: advanced,
        });
    }
}

/// 每进程回收一次过期索引，放后台线程，不挡扫描。
///
/// 挑在这里触发是因为此刻刚确定了「现役索引是哪一份」——回收判定正需要它
/// 作为基准。只跑一次：这是纯粹的磁盘清理，重复扫缓存目录没有意义。
#[cfg(not(windows))]
fn spawn_prune_orphan_indexes(keep: &crate::core::disk::VolumeId) {
    static PRUNED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if PRUNED.set(()).is_err() {
        return;
    }
    let keep = keep.clone();
    std::thread::spawn(move || {
        crate::platform::macos::cache::prune_orphan_indexes(&keep);
    });
}

/// 树被就地修改（删除、增量更新）后，刷新进程内缓存。
#[cfg(not(windows))]
pub fn remember_macos_root_index(scan: std::sync::Arc<crate::core::disk::ScanResult>) {
    let mount = scan.volume.mount_point().to_path_buf();
    let mut cached = CACHED_ROOT_INDEX.lock().unwrap_or_else(|e| e.into_inner());
    let last_event_id = cached
        .as_ref()
        .filter(|entry| entry.mount == mount)
        .map(|entry| entry.last_event_id)
        .unwrap_or(0);
    *cached = Some(CachedRootIndex {
        mount,
        scan,
        last_event_id,
    });
}

#[cfg(not(windows))]
fn load_or_build_macos_index_for_uncached(
    root: &std::path::Path,
    label: &str,
    live: &AtomicBool,
) -> Option<(std::sync::Arc<crate::core::disk::ScanResult>, u64)> {
    if !live.load(Ordering::Relaxed) {
        return None;
    }

    let t0 = std::time::Instant::now();
    let volume = crate::core::disk::VolumeId::from_mount_point(root.to_path_buf());
    if let Some(loaded) = crate::platform::macos::cache::load_index(&volume) {
        crate::log!(
            "加载 {} 索引：{} 条记录，上次事件 ID {}，耗时 {:?}",
            label,
            loaded.scan.records_read,
            loaded.last_event_id,
            t0.elapsed()
        );
        return refresh_cached_macos_index(
            root,
            label,
            std::sync::Arc::new(loaded.scan),
            loaded.last_event_id,
            live,
            RefreshBudget::Interactive,
        );
    }

    crate::log!("未找到 {} 索引，执行首次全量扫描", label);
    let (scan, checkpoint) = match full_macos_scan(root, &volume, live) {
        Ok(result) => result,
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
    Some((std::sync::Arc::new(scan), checkpoint))
}

/// 整盘重建的预算：谁在要这份索引，决定增量走不通时能不能就地重扫。
#[cfg(not(windows))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RefreshBudget {
    /// 用户正在等结果。增量走不通就当场整盘重建——慢，但必须给出答案。
    Interactive,
    /// 后台定时推进。增量走不通就放弃本轮，绝不背着用户跑一次 57s 的整盘
    /// 重建：那笔账本来就该记在用户真正打开界面的那一次上。
    Background,
}

/// 校验并刷新一份进程内缓存。即使没有路径变化，也要持久化推进后的水位。
#[cfg(not(windows))]
fn refresh_cached_macos_index(
    root: &Path,
    label: &str,
    scan: std::sync::Arc<crate::core::disk::ScanResult>,
    last_event_id: u64,
    live: &AtomicBool,
    budget: RefreshBudget,
) -> Option<(std::sync::Arc<crate::core::disk::ScanResult>, u64)> {
    let changes = crate::platform::macos::fsevents::changes_since(root, last_event_id);
    apply_replayed_changes(root, label, scan, last_event_id, live, budget, changes)
}

/// 拿到回放结果之后的决策部分。
///
/// 和 [`refresh_cached_macos_index`] 分开，是为了让「预算」这条判断可测：
/// `changes_since` 要真实的 FSEvents 流，四条支路里哪条把后台线程放进了整盘
/// 重建，靠跑真流是复现不出来的，只能把 `Changes` 直接喂进来。
#[cfg(not(windows))]
pub(super) fn apply_replayed_changes(
    root: &Path,
    label: &str,
    scan: std::sync::Arc<crate::core::disk::ScanResult>,
    last_event_id: u64,
    live: &AtomicBool,
    budget: RefreshBudget,
    changes: Option<crate::platform::macos::fsevents::Changes>,
) -> Option<(std::sync::Arc<crate::core::disk::ScanResult>, u64)> {
    let volume = scan.volume.clone();
    // 四条「增量走不通」的支路原本各自展开一遍整盘重建，收敛到这里：加预算
    // 判定时只有一个地方要改，也就不会漏掉某一条支路把后台线程放进全量扫描。
    let rebuild = |reason: &str| -> Option<(std::sync::Arc<crate::core::disk::ScanResult>, u64)> {
        if budget == RefreshBudget::Background {
            crate::log!(
                "{} 后台推进放弃本轮（{}），整盘重建留给下次交互",
                label,
                reason
            );
            return None;
        }
        crate::log!("{} {}，整盘重建", label, reason);
        let (scan, checkpoint) = full_macos_scan(root, &volume, live).ok()?;
        Some((std::sync::Arc::new(scan), checkpoint))
    };

    match changes {
        Some(changes)
            if !changes.requires_full_scan
                && changes.paths.is_empty()
                && changes.must_rescan.is_empty() =>
        {
            if changes.last_event_id > last_event_id {
                spawn_save_index(volume, scan.clone(), changes.last_event_id);
            }
            Some((scan, changes.last_event_id))
        }
        Some(changes) if !changes.requires_full_scan => {
            let owned = scan.as_ref().clone();
            match refresh_macos_index(&volume, owned, &changes, live) {
                Some(refreshed) => {
                    let refreshed = std::sync::Arc::new(refreshed);
                    spawn_save_index(volume, refreshed.clone(), changes.last_event_id);
                    Some((refreshed, changes.last_event_id))
                }
                None => rebuild("索引增量更新失败"),
            }
        }
        Some(changes) => rebuild(&format!(
            "索引需要全量重建：原因={:?}",
            changes.full_scan_reason
        )),
        None => rebuild("索引水位不可回放"),
    }
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
///
/// 大扫描（根卷整盘）走 [`walk::scan_root_persisted`]：原始条目溢写
/// 临时文件、构建阶段直接产出 v7 映射并落盘——扫描期间不再同时保留
/// RawEntry 数组和最终完整树。小扫描或流式构建失败时回退到
/// `save_index` 的常规路径。
#[cfg(not(windows))]
pub(super) fn full_macos_scan(
    root: &Path,
    volume: &crate::core::disk::VolumeId,
    live: &AtomicBool,
) -> Result<(crate::core::disk::ScanResult, u64), crate::core::disk::ScanError> {
    use crate::platform::macos::cache;
    use crate::platform::macos::walk;
    let checkpoint = crate::platform::macos::fsevents::current_event_id();
    let persisted = cache::index_path_for(volume)
        .map(|index_path| {
            walk::scan_root_persisted(root, volume.clone(), live, &index_path, checkpoint)
        })
        .transpose();
    match persisted {
        Ok(Some((mut scan, true))) => {
            cache::note_saved_index(volume, checkpoint);
            cache::remove_stale_delta(volume);
            // delta 已随全量重写作废；totals 以树为准再校一遍
            refresh_scan_totals(&mut scan);
            return Ok((scan, checkpoint));
        }
        Ok(Some((scan, false))) => {
            // 流式构建回退成堆树：照常 save_index + 换 mmap 主体
            cache::save_index(volume, &scan, checkpoint);
            let mut scan = scan;
            if let Some(tree) = cache::mapped_tree(volume) {
                scan.tree = tree;
                refresh_scan_totals(&mut scan);
            }
            return Ok((scan, checkpoint));
        }
        Ok(None) => {}
        Err(e) => return Err(e),
    }
    let mut scan = walk::scan_root(root, volume.clone(), live)?;
    cache::save_index(volume, &scan, checkpoint);
    if let Some(tree) = cache::mapped_tree(volume) {
        scan.tree = tree;
        refresh_scan_totals(&mut scan);
    }
    Ok((scan, checkpoint))
}

/// 用 FSEvents 变更路径重扫局部子树，避免每次小改动都重扫整个用户目录。
///
/// 直接在 `SizeTree` 上就地操作：删除旧子树、追加新子树、重建 CSR 索引。
/// 不再做「先把整棵树物化成全量 PathBuf 再重建」的往返（旧路径靠
/// from_snapshot 一类转换函数），避免为更新一个 `node_modules` 目录
/// 而把 6.6M 节点全部转成路径再重建。
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

    // 卷根自己被打上 MustScanSubDirs：整棵树都要重扫，等价于全量，直接回退。
    // 除此之外的子树重扫路径和普通变更路径同等对待——下面的元数据判定会把
    // 目录归进 roots（整棵重扫），文件归进就地更新，已消失的归进删除。
    if changes.must_rescan.iter().any(|path| path == mount) {
        crate::log!(
            "refresh_macos_index: 卷根 {} 被标记为需重扫子树，等价全量，回退",
            mount.display()
        );
        return None;
    }

    let mut changed_paths: Vec<PathBuf> = changes
        .paths
        .iter()
        .chain(changes.must_rescan.iter())
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

    // 祖先折叠：字典序下 `PathBuf` 按分量比较，祖先必定紧邻排在它全部后代
    // 之前，且两者之间不会插进无关路径。于是只需和「上一个保留下来的根」比
    // 一次，就能判定当前路径是否已被覆盖——O(n log n)，取代原先对 `covered`
    // 全表扫描的 O(n²)。`Path::starts_with` 按分量比较，`/a/b` 不会误吞
    // `/a-x`。
    roots.sort();
    roots.dedup();
    let mut kept: Vec<PathBuf> = Vec::with_capacity(roots.len());
    for path in roots {
        if kept.last().is_some_and(|last| path.starts_with(last)) {
            continue;
        }
        kept.push(path);
    }
    let roots = kept;
    crate::log!(
        "refresh_macos_index: {} 个原始路径 → 去重后 {} 个独立变更根",
        changes.paths.len() + changes.must_rescan.len(),
        roots.len()
    );

    // 局部重扫什么时候不划算？取决于要重扫多少**内容**，不是有多少个根。
    //
    // 原先卡的是根的个数（>512 放弃）。日志里四次触发全是 517/554/661/866
    // ——全都贴着门槛，且绝大多数根是几十上百条记录的小目录：并行重扫它们
    // 只要毫秒级，却换来一次 50~120 秒的整盘重建。现在改成按旧树里这些子树
    // 的记录数估算成本，只有当重扫量逼近整棵树时（全量还顺带压实索引、重置
    // 水位，更划算）才放弃。
    //
    // 树里查不到的根是新建目录，估不出体积，只能按 0 计入；`MAX_ROOTS` 因此
    // 保留一个宽松的兜底上限，防止极端情况下并行调度本身成为瓶颈。
    const MAX_ROOTS: usize = 20_000;
    const REBUILD_RATIO: f64 = 0.20;
    /// 比例低于这个绝对量时一律走增量。占比只在树足够大时才说明问题——
    /// 一棵 4 条记录的测试树里重扫 1 条就是 25%，但那 1 条是微秒级的活，
    /// 换成全量反而更贵。
    const ALWAYS_INCREMENTAL_RECORDS: u64 = 50_000;
    let total_records = scan.tree.file_count_of(scan.tree.root());
    let mut estimated: u64 = 0;
    let mut unknown_roots = 0usize;
    for root in &roots {
        match scan.tree.find_node_by_path(root) {
            Some(node) => estimated = estimated.saturating_add(scan.tree.file_count_of(node)),
            None => unknown_roots += 1,
        }
    }
    let ratio = if total_records == 0 {
        1.0
    } else {
        estimated as f64 / total_records as f64
    };
    crate::log!(
        "refresh_macos_index: 重扫成本估算 {} / {} 条记录（{:.1}%），新建根 {} 个",
        estimated,
        total_records,
        ratio * 100.0,
        unknown_roots
    );
    if roots.len() > MAX_ROOTS {
        crate::log!(
            "refresh_macos_index: 独立变更根 {} 个 > {}，放弃增量",
            roots.len(),
            MAX_ROOTS
        );
        return None;
    }
    if estimated > ALWAYS_INCREMENTAL_RECORDS && ratio > REBUILD_RATIO {
        crate::log!(
            "refresh_macos_index: 重扫量占全树 {:.1}% > {:.0}%，全量更划算，放弃增量",
            ratio * 100.0,
            REBUILD_RATIO * 100.0
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
                // 父目录不在树中：多为 SIP/权限保护目录（如
                // /private/var/db/searchparty），walk 本就读不进去、索引里
                // 天然没有，这类事件永远放不进树。跳过这一个子树、继续
                // 合并其余——实测曾因这一条路径放弃整个增量、回退 80 秒
                // 全量扫描，代价完全不成比例。
                crate::log!(
                    "refresh_macos_index: 父目录 {} 不在树中，跳过子树 {}（多为 SIP/权限保护目录）",
                    parent_path.display(),
                    sr.root.display()
                );
                continue;
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
    let file_count = scan.tree.file_count_of(scan.tree.root());
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
            None => {
                // 名字没命中，看 child 的子项里有没有 CACHEDIR.TAG（树内
                // 查名，无 IO）；有再读盘验签名——验签名的文件 IO 只发生
                // 在树内已见信号时，避免给遍历热点路径加磁盘读。
                let tagged = tree.child_indices(child).iter().any(|&grand| {
                    tree.valid(grand)
                        && !tree.is_dir(grand)
                        && tree.entry_name(grand).eq_ignore_ascii_case("cachedir.tag")
                });
                if tagged {
                    let mut cache: std::collections::HashMap<u32, String> =
                        std::collections::HashMap::new();
                    let path = std::path::PathBuf::from(tree.path_of_with(child, &mut cache));
                    if super::has_cachedir_tag(&path) {
                        out.push((child, &super::CACHEDIR_MARKER));
                        continue;
                    }
                }
                collect_tree(tree, child, depth + 1, max_depth, live, out)
            }
        }
    }
}
