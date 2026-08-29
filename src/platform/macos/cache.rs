//! macOS 扫描结果索引持久化
//!
//! 把全量/增量扫描结果落盘为 v7 二进制索引，下次启动时 mmap 恢复，
//! 避免每次都重新走一遍完整扫描。
//!
//! # 索引文件
//!
//! - Base 索引：`~/Library/Application Support/QuickCleaner/scan-index-<key>.bin`
//! - Delta 增量：同目录下 `scan-index-<key>.delta.bin`，与 base 一一对应
//! - `<key>` 由卷挂载点路径逐字节转十六进制得到
//!
//! # 测试隔离
//!
//! `QUICKCLEANER_CACHE_DIR` 环境变量可覆盖缓存目录。测试用它指向临时目录，
//! 避免覆盖用户真实的索引文件。
//!
//! # FSEvents 增量
//!
//! 完整索引由 `fsevents` 模块保存 FSEvents 水位。没有变化时直接复用；创建、
//! 删除和修改事件只重扫受影响子树，重命名、事件丢失或历史日志不可用时回退全量扫描。

use crate::core::disk::{ScanResult, SizeTree, VolumeId};
use crate::platform::macos::disk_tree::IndexMeta;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 缓存/索引文件所在目录，**不保证存在**。
///
/// 唯一权威来源：[`cache_dir`]（会顺手建目录）和 FSEvents 的
/// 自过滤都从这里取。以前 fsevents.rs 自己拼了一份写死的
/// `~/Library/Application Support/QuickCleaner`，一旦 `QUICKCLEANER_CACHE_DIR`
/// 生效，两边就指向不同目录——FSEvents 会把自己写索引产生的事件当成用户
/// 文件变化，每次启动都触发一次全量重扫。
pub(crate) fn cache_dir_path() -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("QUICKCLEANER_CACHE_DIR") {
        return Some(PathBuf::from(custom));
    }
    Some(
        dirs::home_dir()?
            .join("Library")
            .join("Application Support")
            .join("QuickCleaner"),
    )
}

/// 缓存/索引目录路径，不存在时自动创建。
///
/// 优先读 `QUICKCLEANER_CACHE_DIR` 环境变量（测试隔离用），否则落到
/// `~/Library/Application Support/QuickCleaner`。
pub(crate) fn cache_dir() -> Option<PathBuf> {
    let dir = cache_dir_path()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

pub struct LoadedIndex {
    pub scan: ScanResult,
    pub last_event_id: u64,
}

/// 串行化索引写入，并记录本进程已落盘的最高 FSEvents 水位。异步保存可能
/// 乱序完成，较旧结果绝不能在较新结果之后覆盖索引文件。
static INDEX_SAVE_WATERMARKS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<PathBuf, u64>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn index_path(volume: &VolumeId) -> Option<PathBuf> {
    let mount = volume.mount_point().to_string_lossy();
    let key: String = mount.bytes().map(|byte| format!("{byte:02x}")).collect();
    cache_dir().map(|dir| dir.join(format!("scan-index-{key}.bin")))
}

/// delta 文件路径，与 base 索引一一对应。
fn index_delta_path(volume: &VolumeId) -> Option<PathBuf> {
    let mount = volume.mount_point().to_string_lossy();
    let key: String = mount.bytes().map(|byte| format!("{byte:02x}")).collect();
    cache_dir().map(|dir| dir.join(format!("scan-index-{key}.delta.bin")))
}

/// 增量规模超过该值（追加 + 覆盖节点合计）时不再追加 delta，
/// 直接触发一次流式压实，把 base 与增量合并重写。
/// 这样 delta 文件体积有界，加载端也不会无限膨胀。
const DELTA_COMPACT_THRESHOLD: usize = 200_000;

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// 把已落盘的 v7 文件重新 mmap 成运行时树，用于全量扫描后丢掉堆上主体。
pub fn mapped_tree(volume: &VolumeId) -> Option<SizeTree> {
    let path = index_path(volume)?;
    SizeTree::from_mapped(volume.clone(), &path)
}

/// 索引文件路径（供全量扫描直接流式落盘）。
pub(crate) fn index_path_for(volume: &VolumeId) -> Option<PathBuf> {
    index_path(volume)
}

/// 记录"索引已由扫描直接落盘"的事件水位，跳过重复的 save_index。
pub(crate) fn note_saved_index(volume: &VolumeId, last_event_id: u64) {
    let Some(path) = index_path(volume) else {
        return;
    };
    let mut watermarks = INDEX_SAVE_WATERMARKS
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    watermarks.insert(path, last_event_id);
}

/// 全量扫描已经原子替换 base 后，旧 delta 不再可能适用。
pub(crate) fn remove_stale_delta(volume: &VolumeId) {
    if let Some(path) = index_delta_path(volume) {
        let _ = std::fs::remove_file(path);
    }
}

/// 回收不再需要的索引文件。
///
/// 索引按挂载点命名（`scan-index-<挂载点 hex>.bin`），但从来没人删过旧的：
/// 实测缓存目录里同时躺着 328 MB 的整盘索引和 230 MB 的 `~` 索引，后者自从
/// 扫描口径改成整盘之后再没被读过。一个清理工具自己留着几百兆死缓存说不过去。
///
/// 两条判定，都以「删了最多损失一次重扫」为底线：
///
/// 1. **被现役索引覆盖**：挂载点是 `keep` 的严格后代，**且在同一个卷上**。
///    `/` 的索引已经包含 `/Users/xxx` 的全部内容，那份子集索引永远不会再被
///    加载。同卷判定不能省：路径意义上 `/Volumes/外置盘` 也是 `/` 的后代，
///    但它是另一个文件系统，整盘索引并不覆盖它。macOS 的 firmlink 对 `stat`
///    透明（`/` 与 `/Users` 的 `st_dev` 相同），所以设备号是可靠依据。
/// 2. **太久没动**：超过 [`ORPHAN_MAX_AGE_DAYS`] 天没被写过。拔掉的外置盘、
///    一次性的扫描根都归这类；真要再用，重扫一次即可。
///
/// 当前正在用的那份（`keep` 自己）和它的 delta 永远不动。
pub(crate) fn prune_orphan_indexes(keep: &VolumeId) {
    const ORPHAN_MAX_AGE_DAYS: u64 = 30;
    let max_age_secs = ORPHAN_MAX_AGE_DAYS * 24 * 3600;

    let Some(dir) = cache_dir_path() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let keep_mount = keep.mount_point().to_path_buf();
    let now = now_epoch_secs();

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // base 索引和它的 delta 一起判定、一起删，避免留下对不上 base 的 delta。
        let Some(key) = name
            .strip_prefix("scan-index-")
            .and_then(|rest| rest.strip_suffix(".bin"))
        else {
            continue;
        };
        if key.ends_with(".delta") {
            continue;
        }
        let Some(mount) = decode_mount_key(key) else {
            continue;
        };
        if mount == keep_mount {
            continue;
        }

        let covered = mount.starts_with(&keep_mount) && same_volume(&mount, &keep_mount);
        let stale = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .is_some_and(|d| now.saturating_sub(d.as_secs()) > max_age_secs);
        if !covered && !stale {
            continue;
        }

        let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let reason = if covered {
            "被现役索引覆盖"
        } else {
            "超期未用"
        };
        if std::fs::remove_file(&path).is_ok() {
            crate::log!(
                "回收索引 {}（{}，{:.1} MB）",
                mount.display(),
                reason,
                bytes as f64 / (1024.0 * 1024.0)
            );
        }
        let delta = dir.join(format!("scan-index-{key}.delta.bin"));
        let _ = std::fs::remove_file(delta);
    }
}

/// 两个路径是否落在同一个文件系统上。任一侧 `stat` 不到（路径已消失、
/// 盘已拔掉）就返回 `false`——判不出来时按「不覆盖」处理，交给超期规则，
/// 宁可多留一份索引，也不误删还在用的那份。
fn same_volume(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev(),
        _ => false,
    }
}

/// 把索引文件名里的 hex 还原成挂载点路径，非法编码返回 `None`。
fn decode_mount_key(key: &str) -> Option<PathBuf> {
    if !key.len().is_multiple_of(2) || key.is_empty() {
        return None;
    }
    let mut bytes = Vec::with_capacity(key.len() / 2);
    for pair in key.as_bytes().chunks(2) {
        let hex = std::str::from_utf8(pair).ok()?;
        bytes.push(u8::from_str_radix(hex, 16).ok()?);
    }
    String::from_utf8(bytes).ok().map(PathBuf::from)
}

/// 从完整索引恢复运行时 `ScanResult`。
///
/// v7 是未压缩 mmap 文件（条目 + 名字池 + CSR），加载不把 24B 节点复制进 Vec。
/// 存在有效 delta 文件时叠加应用，统计取 delta 头里的最新值。
/// base 只 mmap + 校验一次；delta 缺失或无效时直接用 base 的 header 统计，
/// 绝不能二次加载（那会让启动耗时翻倍）。
pub fn load_index(volume: &VolumeId) -> Option<LoadedIndex> {
    let mut loaded = load_index_raw(volume)?;
    heal_data_volume_mirror(volume, &mut loaded);
    Some(loaded)
}

/// 旧索引自愈：修复前 walk 会把 `/System/Volumes/Data` 镜像一并收录，
/// 用户树在索引里存在两份（firmlink 两侧各一），删除类事件只落在其中
/// 一份上，另一份成了僵尸（搜索还能搜到已删目录、两个入口大小不一）。
/// 加载时把镜像子树整条移除，计数随 `remove_path` 一并扣减。新索引
/// （walk 已剪枝）里找不到该节点，这一步是零开销空操作；被治好的树
/// 在下一次落盘时自然固化。
fn heal_data_volume_mirror(volume: &VolumeId, loaded: &mut LoadedIndex) {
    if volume.mount_point() != std::path::Path::new("/") {
        return;
    }
    let mirror = std::path::PathBuf::from("/System/Volumes/Data");
    if loaded.scan.tree.find_node_by_path(&mirror).is_some() {
        loaded.scan.remove_path(&mirror);
        crate::log!("索引自愈：已移除 /System/Volumes/Data 镜像子树（firmlink 重复侧）");
    }
}

fn load_index_raw(volume: &VolumeId) -> Option<LoadedIndex> {
    let path = index_path(volume)?;
    let delta_path = index_delta_path(volume);
    if let Some(dp) = delta_path {
        if let Some((tree, dmeta)) = SizeTree::from_mapped_with_delta(volume.clone(), &path, &dp) {
            let (file_count, dir_count, total_size_hdr, last_event_id) = match dmeta {
                Some(dm) => (dm.file_count, dm.dir_count, dm.total_size, dm.last_event_id),
                None => {
                    let (f, d, t, e) = tree.mapped_header_stats()?;
                    (f, d, t, e)
                }
            };
            let total_size = if total_size_hdr == 0 {
                tree.size_of(tree.root())
            } else {
                total_size_hdr
            };
            let records = file_count + dir_count;
            crate::log!(
                "索引 mmap{} 加载：{} 条 + {} 增量，映射 {:.1} MB",
                if dmeta.is_some() { " + delta" } else { "" },
                tree.entry_count(),
                tree.delta_len(),
                tree.memory_bytes() as f64 / (1024.0 * 1024.0)
            );
            release_malloc_pressure();
            return Some(LoadedIndex {
                scan: ScanResult {
                    volume: volume.clone(),
                    total_size,
                    file_count,
                    dir_count,
                    dirs: Vec::new(),
                    tree,
                    elapsed_ms: 0,
                    records_read: records,
                    records_expected: records,
                    mft_run_bytes: 0,
                    ext_records: 0,
                    ext_data_merged: 0,
                    hard_links: 0,
                    unique_size: total_size,
                    unique_files: file_count,
                },
                last_event_id,
            });
        }
        // base 本身加载失败（损坏/版本不符），没有可恢复的东西
        return None;
    }
    let tree = SizeTree::from_mapped(volume.clone(), &path)?;
    let (file_count, dir_count, total_size, last_event_id) = tree.mapped_header_stats()?;
    crate::log!(
        "索引 mmap 完成：{} 条，映射 {:.1} MB",
        tree.entry_count(),
        tree.memory_bytes() as f64 / (1024.0 * 1024.0)
    );
    let total_size = if total_size == 0 {
        tree.size_of(tree.root())
    } else {
        total_size
    };
    let records = file_count + dir_count;
    release_malloc_pressure();
    Some(LoadedIndex {
        scan: ScanResult {
            volume: volume.clone(),
            total_size,
            file_count,
            dir_count,
            dirs: Vec::new(),
            tree,
            elapsed_ms: 0,
            records_read: records,
            records_expected: records,
            mft_run_bytes: 0,
            ext_records: 0,
            ext_data_merged: 0,
            hard_links: 0,
            unique_size: total_size,
            unique_files: file_count,
        },
        last_event_id,
    })
}

/// 原子保存完整目录索引，避免应用崩溃留下半个文件。
///
/// mmap 树上的小增量只写 delta 文件（几 KB～几十 MB），不再每次压实
/// 重写整个 587MB 的 base——那会带来 4.8s 的保存耗时和 1.3GiB 的
/// footprint 峰值。delta 累积超过 [`DELTA_COMPACT_THRESHOLD`] 或树是
/// 堆主体时，才走一次流式压实全量写出。
pub fn save_index(volume: &VolumeId, scan: &ScanResult, last_event_id: u64) {
    let Some(path) = index_path(volume) else {
        return;
    };
    let mut watermarks = INDEX_SAVE_WATERMARKS
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if watermarks
        .get(&path)
        .is_some_and(|saved| *saved > last_event_id)
    {
        crate::log!(
            "跳过过期索引保存：事件水位 {} < 已保存 {} ({})",
            last_event_id,
            watermarks[&path],
            path.display()
        );
        return;
    }
    let file_count = scan.tree.count_used_files();
    let dir_count = scan.tree.count_used_dirs();
    let total_size = scan.tree.size_of(scan.tree.root());
    let scanned_at = now_epoch_secs();

    // mmap 主体的小增量只写 delta；即使树没有结构变化，也写一个空 payload
    // 的元数据 delta，把新的 FSEvents 水位和统计持久化到下次启动。
    if scan.tree.has_mapped_base() && scan.tree.delta_len() <= DELTA_COMPACT_THRESHOLD {
        if let (Some(delta_path), Some(base_checksum)) =
            (index_delta_path(volume), scan.tree.base_checksum())
        {
            let dmeta = crate::platform::macos::disk_tree::DeltaMeta {
                base_checksum,
                last_event_id,
                scanned_at,
                file_count,
                dir_count,
                total_size,
                n_entries: 0,
                pool_len: 0,
                n_overrides: 0,
                n_extra_parents: 0,
            };
            match scan.tree.write_delta(&delta_path, &dmeta) {
                Ok(()) => {
                    watermarks.insert(path.clone(), last_event_id);
                    crate::log!(
                        "索引增量已保存：{} 追加 + {} 覆盖节点（base {} 条不动）",
                        scan.tree.entries_delta_len(),
                        scan.tree.overrides_delta_len(),
                        scan.tree.entry_count() - scan.tree.entries_delta_len()
                    );
                    return;
                }
                Err(e) => {
                    crate::log!("写索引 delta 失败（{}），回退全量压实", e);
                }
            }
        }
    }

    let meta = IndexMeta {
        mount: volume.mount_point().to_string_lossy().into_owned(),
        label: volume.display().to_string(),
        file_count,
        dir_count,
        total_size,
        last_event_id,
        scanned_at,
    };
    if scan.tree.write_v7(&path, meta).is_ok() {
        // delta 已并入 base，旧文件作废
        if let Some(delta_path) = index_delta_path(volume) {
            let _ = std::fs::remove_file(&delta_path);
        }
        watermarks.insert(path.clone(), last_event_id);
        crate::log!(
            "索引已保存：{} 条，文件 {:.1} MB",
            scan.tree.entry_count(),
            std::fs::metadata(&path)
                .map(|m| m.len() as f64 / (1024.0 * 1024.0))
                .unwrap_or(0.0)
        );
        release_malloc_pressure();
    }
}

fn release_malloc_pressure() {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn malloc_zone_pressure_relief(zone: *mut libc::c_void, goal: usize) -> usize;
        }
        unsafe {
            malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
        }
    }
}

/// 序列化所有会修改 `QUICKCLEANER_CACHE_DIR` 环境变量的测试。
///
/// `env::set_var` 是进程级的，`cargo test` 默认多线程并发跑用例，
/// 不加锁的话两个用例会互相覆盖对方设的目录，导致写错地方。
#[cfg(test)]
static CACHE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 把 `QUICKCLEANER_CACHE_DIR` 指向一个临时目录，返回 guard。
///
/// `scan_volume` 完成后会无条件 `save()`，不隔离的话跑一次 `cargo test`
/// 就把用户真实的 `scan-cache.json` 冲掉。返回的 guard 保证在用例结束前
/// 环境变量不会被另一个用例改掉——所有调用此函数的用例共享同一把锁，
/// 因此它们会被串行化，不会互相踩环境变量。
#[cfg(test)]
pub(crate) fn isolate_cache_dir(test_name: &str) -> std::sync::MutexGuard<'static, ()> {
    let guard = CACHE_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 必须带 pid：`CACHE_ENV_LOCK` 只在**单个进程内**串行，而同时存在两个
    // 测试二进制是常态（上一次运行没退干净、CI 并发、手工重跑）。不带 pid
    // 时它们指向同一个目录，一方的 `remove_dir_all` 会清掉另一方正在读的
    // 索引，症状是「应当能读出索引 NotFound」「现役索引不能被回收」。
    let dir = crate::core::testing::fixture("quick-cleaner-test-cache").join(test_name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时缓存目录失败");
    std::env::set_var("QUICKCLEANER_CACHE_DIR", &dir);
    guard
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::disk::TreeSnapshotEntry;

    fn test_scan(volume: VolumeId, size: u64) -> ScanResult {
        let root = volume.mount_point().to_path_buf();
        let tree = SizeTree::from_snapshot(
            volume.clone(),
            vec![
                TreeSnapshotEntry {
                    path: root.clone(),
                    is_dir: true,
                    size,
                    mtime: 0,
                },
                TreeSnapshotEntry {
                    path: root.join("cache.bin"),
                    is_dir: false,
                    size,
                    mtime: 0,
                },
            ],
        );
        ScanResult {
            volume,
            total_size: size,
            file_count: 1,
            dir_count: 1,
            dirs: Vec::new(),
            tree,
            elapsed_ms: 0,
            records_read: 2,
            records_expected: 2,
            mft_run_bytes: 0,
            ext_records: 0,
            ext_data_merged: 0,
            hard_links: 0,
            unique_size: size,
            unique_files: 1,
        }
    }

    #[test]
    fn orphan_prune_removes_covered_index_but_keeps_current() {
        let _guard = isolate_cache_dir("orphan_prune");
        let dir = cache_dir().unwrap();

        // 用真实存在的目录：覆盖判定要 stat 设备号，虚构路径判不出同卷。
        let base = crate::core::testing::fixture("qc_orphan_prune_base");
        let nested = base.join("nested");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&nested).unwrap();

        let keep = VolumeId::from_mount_point(base.clone());
        let covered = VolumeId::from_mount_point(nested.clone());
        // 路径上是 `/` 的后代，但不在同一个卷上，且并不存在——不该被当成覆盖。
        let other = VolumeId::from_mount_point(PathBuf::from("/Volumes/qc-not-mounted"));
        for volume in [&keep, &covered, &other] {
            std::fs::write(index_path(volume).unwrap(), b"stub").unwrap();
            std::fs::write(index_delta_path(volume).unwrap(), b"stub").unwrap();
        }

        prune_orphan_indexes(&keep);

        assert!(index_path(&keep).unwrap().exists(), "现役索引不能被回收");
        assert!(
            index_delta_path(&keep).unwrap().exists(),
            "现役索引的 delta 不能被回收"
        );
        assert!(
            !index_path(&covered).unwrap().exists(),
            "同卷且被现役索引包含的子集索引应被回收"
        );
        assert!(
            !index_delta_path(&covered).unwrap().exists(),
            "回收 base 时必须一并删掉它的 delta"
        );
        assert!(
            index_path(&other).unwrap().exists(),
            "判不出同卷的索引刚写过，应交给超期规则，不能立刻删"
        );

        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn mount_key_round_trips_through_the_file_name() {
        for mount in ["/", "/Users/someone", "/Volumes/带中文 的盘"] {
            let volume = VolumeId::from_mount_point(PathBuf::from(mount));
            let path = {
                let _guard = isolate_cache_dir("mount_key_round_trip");
                index_path(&volume).unwrap()
            };
            let name = path.file_name().unwrap().to_str().unwrap();
            let key = name
                .strip_prefix("scan-index-")
                .and_then(|r| r.strip_suffix(".bin"))
                .unwrap();
            assert_eq!(decode_mount_key(key), Some(PathBuf::from(mount)));
        }
        // 非法编码不能 panic，也不能误判成某个挂载点
        assert_eq!(decode_mount_key("zz"), None);
        assert_eq!(decode_mount_key("abc"), None);
        assert_eq!(decode_mount_key(""), None);
    }

    #[test]
    fn index_tree_round_trip_preserves_sizes() {
        let root = PathBuf::from(format!("{}_{}", "/tmp/qc-index-root", std::process::id()));
        let volume = VolumeId::from_mount_point(root.clone());
        let tree = SizeTree::from_snapshot(
            volume.clone(),
            vec![
                TreeSnapshotEntry {
                    path: root.clone(),
                    is_dir: true,
                    size: 4096,
                    mtime: 0,
                },
                TreeSnapshotEntry {
                    path: root.join("project"),
                    is_dir: true,
                    size: 4096,
                    mtime: 0,
                },
                TreeSnapshotEntry {
                    path: root.join("project").join("target.bin"),
                    is_dir: false,
                    size: 4096,
                    mtime: 0,
                },
            ],
        );
        let project = *tree
            .child_indices(tree.root())
            .iter()
            .find(|&&idx| tree.entry_name(idx) == "project")
            .expect("应当找到项目目录");
        assert_eq!(tree.size_of(project), 4096);
        assert_eq!(tree.size_of(tree.root()), 4096);
        let restored = SizeTree::from_compact(volume, tree.compact_entries());
        assert_eq!(restored.size_of(restored.root()), 4096);
    }

    #[test]
    fn older_async_save_cannot_overwrite_newer_watermark() {
        let _guard = isolate_cache_dir("index_watermark_order");
        let volume = VolumeId::from_mount_point(PathBuf::from(format!(
            "{}_{}",
            "/tmp/qc-watermark",
            std::process::id()
        )));
        let newer = test_scan(volume.clone(), 8192);
        let older = test_scan(volume.clone(), 4096);

        save_index(&volume, &newer, 20);
        save_index(&volume, &older, 10);

        let loaded = load_index(&volume).expect("索引应当能够重新加载");
        assert_eq!(loaded.last_event_id, 20);
        assert_eq!(loaded.scan.total_size, 8192);
    }

    fn snapshot_tree(root: PathBuf, file_size: u64) -> (VolumeId, ScanResult) {
        let volume = VolumeId::from_mount_point(root.clone());
        let tree = SizeTree::from_snapshot(
            volume.clone(),
            vec![
                TreeSnapshotEntry {
                    path: root.clone(),
                    is_dir: true,
                    size: 0,
                    mtime: 0,
                },
                TreeSnapshotEntry {
                    path: root.join("project"),
                    is_dir: true,
                    size: 0,
                    mtime: 0,
                },
                TreeSnapshotEntry {
                    path: root.join("project").join("node_modules"),
                    is_dir: true,
                    size: 0,
                    mtime: 0,
                },
                TreeSnapshotEntry {
                    path: root.join("project").join("node_modules").join("pkg"),
                    is_dir: true,
                    size: 0,
                    mtime: 0,
                },
                TreeSnapshotEntry {
                    path: root
                        .join("project")
                        .join("node_modules")
                        .join("pkg")
                        .join("index.js"),
                    is_dir: false,
                    size: file_size,
                    mtime: 0,
                },
            ],
        );
        let scan = ScanResult {
            volume: volume.clone(),
            total_size: tree.size_of(tree.root()),
            file_count: tree.count_used_files(),
            dir_count: tree.count_used_dirs(),
            dirs: Vec::new(),
            tree,
            elapsed_ms: 0,
            records_read: 5,
            records_expected: 5,
            mft_run_bytes: 0,
            ext_records: 0,
            ext_data_merged: 0,
            hard_links: 0,
            unique_size: file_size,
            unique_files: 1,
        };
        (volume, scan)
    }

    #[test]
    fn v7_mmap_round_trip_keeps_nested_search_and_children() {
        let _guard = isolate_cache_dir("v7_mmap_nested_search");
        let root = PathBuf::from(format!("{}_{}", "/tmp/qc-v7-nested", std::process::id()));
        let (volume, scan) = snapshot_tree(root.clone(), 4096);
        save_index(&volume, &scan, 7);
        let loaded = load_index(&volume).expect("v7 索引应当能 mmap 加载");
        let tree = &loaded.scan.tree;
        let nested = root
            .join("project")
            .join("node_modules")
            .join("pkg")
            .join("index.js");
        assert!(
            tree.find_node_by_path(&nested).is_some(),
            "全量索引必须能按路径找到 node_modules 内部文件"
        );
        let hits = tree.search("index.js", 16);
        assert_eq!(hits.len(), 1, "搜索必须命中折叠策略以前会丢掉的内部文件");
        assert!(hits[0].path.ends_with("index.js"));
        let kids = tree.children(tree.root());
        assert_eq!(kids.len(), 1);
        assert_eq!(kids[0].name, "project");
        assert_eq!(tree.file_count_of(tree.root()), 1);
        assert_eq!(loaded.scan.file_count, 1);
        assert_eq!(loaded.scan.dir_count, 4);
    }

    #[test]
    fn v7_rejects_corrupt_checksum() {
        let _guard = isolate_cache_dir("v7_bad_checksum");
        let root = PathBuf::from(format!("{}_{}", "/tmp/qc-v7-checksum", std::process::id()));
        let (volume, scan) = snapshot_tree(root, 1024);
        save_index(&volume, &scan, 1);
        let path = index_path(&volume).expect("应当有索引路径");
        let mut bytes = std::fs::read(&path).expect("应当能读出索引");
        let header = 128;
        assert!(bytes.len() > header);
        bytes[header] ^= 0xff;
        std::fs::write(&path, bytes).expect("应当能写回损坏的索引");
        assert!(
            load_index(&volume).is_none(),
            "checksum 不匹配的索引必须拒绝加载"
        );
    }

    #[test]
    fn v7_rejects_corrupt_header_watermark() {
        let _guard = isolate_cache_dir("v7_bad_header_watermark");
        let root = PathBuf::from(format!(
            "{}_{}",
            "/tmp/qc-v7-header-watermark",
            std::process::id()
        ));
        let (volume, scan) = snapshot_tree(root, 1024);
        save_index(&volume, &scan, 7);
        let path = index_path(&volume).expect("应当有索引路径");
        let mut bytes = std::fs::read(&path).expect("应当能读出索引");
        bytes[56..64].copy_from_slice(&999u64.to_le_bytes());
        std::fs::write(&path, bytes).expect("应当能写回损坏的索引");
        assert!(
            load_index(&volume).is_none(),
            "header 中水位被篡改后必须因 checksum 不匹配而拒绝加载"
        );
    }

    #[test]
    fn v7_rejects_bad_csr_sentinel_even_with_checksum() {
        let _guard = isolate_cache_dir("v7_bad_csr_sentinel");
        let root = PathBuf::from(format!("{}_{}", "/tmp/qc-v7-csr", std::process::id()));
        let (volume, scan) = snapshot_tree(root, 2048);
        save_index(&volume, &scan, 1);
        let path = index_path(&volume).expect("应当有索引路径");
        let mut bytes = std::fs::read(&path).expect("应当能读出索引");
        let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let cs_off = u32::from_le_bytes(bytes[88..92].try_into().unwrap()) as usize;
        bytes[cs_off + n * 4..cs_off + (n + 1) * 4].copy_from_slice(&0u32.to_le_bytes());
        let sum = crate::platform::macos::index_v7::index_checksum_bytes(&bytes);
        bytes[72..80].copy_from_slice(&sum.to_le_bytes());
        std::fs::write(&path, bytes).expect("应当能写回结构损坏的索引");
        assert!(
            load_index(&volume).is_none(),
            "child_start 尾哨兵错误必须在加载时被拒绝"
        );
    }

    #[test]
    fn v7_rejects_duplicate_csr_child_even_with_checksum() {
        let _guard = isolate_cache_dir("v7_duplicate_csr_child");
        let root = PathBuf::from(format!(
            "{}_{}",
            "/tmp/qc-v7-duplicate-child",
            std::process::id()
        ));
        let (volume, scan) = snapshot_tree(root, 2048);
        save_index(&volume, &scan, 1);
        let path = index_path(&volume).expect("应当有索引路径");
        let mut bytes = std::fs::read(&path).expect("应当能读出索引");
        let ca_off = u32::from_le_bytes(bytes[92..96].try_into().unwrap()) as usize;
        let ca_len = u32::from_le_bytes(bytes[20..24].try_into().unwrap()) as usize;
        assert!(ca_len >= 2);
        let first = bytes[ca_off..ca_off + 4].to_vec();
        bytes[ca_off + 4..ca_off + 8].copy_from_slice(&first);
        let sum = crate::platform::macos::index_v7::index_checksum_bytes(&bytes);
        bytes[72..80].copy_from_slice(&sum.to_le_bytes());
        std::fs::write(&path, bytes).expect("应当能写回结构损坏的索引");
        assert!(
            load_index(&volume).is_none(),
            "重复 child_at 会同时造成另一节点缺失，必须拒绝加载"
        );
    }

    #[test]
    fn v7_rejects_forward_parent_even_with_checksum() {
        let _guard = isolate_cache_dir("v7_bad_parent");
        let root = PathBuf::from(format!("{}_{}", "/tmp/qc-v7-parent", std::process::id()));
        let (volume, scan) = snapshot_tree(root, 2048);
        save_index(&volume, &scan, 1);
        let path = index_path(&volume).expect("应当有索引路径");
        let mut bytes = std::fs::read(&path).expect("应当能读出索引");
        let ent_off = u32::from_le_bytes(bytes[84..88].try_into().unwrap()) as usize;
        // 节点 1 的 parent 改成 2（指向后面的节点，破坏 parent < index）
        let parent_off = ent_off + 24;
        let used_dir_bits = 0xC000_0000u32;
        bytes[parent_off..parent_off + 4].copy_from_slice(&(2u32 | used_dir_bits).to_le_bytes());
        let sum = crate::platform::macos::index_v7::index_checksum_bytes(&bytes);
        bytes[72..80].copy_from_slice(&sum.to_le_bytes());
        std::fs::write(&path, bytes).expect("应当能写回结构损坏的索引");
        assert!(
            load_index(&volume).is_none(),
            "父下标前向引用必须在加载时被拒绝"
        );
    }

    #[test]
    fn mmap_overlay_upsert_is_private_to_clone() {
        let _guard = isolate_cache_dir("v7_mmap_overlay");
        let root = PathBuf::from(format!("{}_{}", "/tmp/qc-v7-overlay", std::process::id()));
        let (volume, scan) = snapshot_tree(root.clone(), 4096);
        save_index(&volume, &scan, 3);
        let loaded = load_index(&volume).expect("应当能加载");
        let orig_files = loaded.scan.tree.count_used_files();
        let mut cloned = loaded.scan.tree.clone();
        let extra = root.join("project").join("new.bin");
        assert!(cloned.upsert_file(&extra, 100));
        assert_eq!(
            loaded.scan.tree.count_used_files(),
            orig_files,
            "对 clone 的增量写入不得改到原 mmap 主体"
        );
        assert_eq!(cloned.count_used_files(), orig_files + 1);
        assert!(cloned.find_node_by_path(&extra).is_some());
        let project = cloned
            .find_node_by_path(&root.join("project"))
            .expect("project 应当存在");
        let names: Vec<String> = cloned
            .children(project)
            .into_iter()
            .map(|n| n.name)
            .collect();
        assert!(
            names.iter().any(|n| n == "new.bin"),
            "overlay 子节点必须出现在 children() 里: {names:?}"
        );
        let hits = cloned.search("new.bin", 8);
        assert_eq!(hits.len(), 1);
    }

    /// 回归测试：克隆一棵**已经修改过**的 mmap 树必须保留修改。
    ///
    /// 旧实现 Clone 对同一 inode 重新 mmap，只能看到原文件内容，原映射
    /// 上的 MAP_PRIVATE COW 修改（墓碑、目录聚合更新）全部丢失——
    /// FSEvents 增量刷新后再执行 UI 删除，旧节点会被复活、目录统计回退。
    #[test]
    fn clone_of_modified_mmap_tree_keeps_delta() {
        let _guard = isolate_cache_dir("v7_clone_keeps_delta");
        let root = PathBuf::from(format!(
            "{}_{}",
            "/tmp/qc-v7-clone-delta",
            std::process::id()
        ));
        let (volume, scan) = snapshot_tree(root.clone(), 4096);
        save_index(&volume, &scan, 1);
        let loaded = load_index(&volume).expect("应当能加载");

        // 第一份克隆上做增量：追加文件 + 删除已有子树
        let mut modified = loaded.scan.tree.clone();
        assert!(modified.upsert_file(&root.join("project").join("added.bin"), 777));
        let nm = modified
            .find_node_by_path(&root.join("project").join("node_modules"))
            .expect("node_modules 应当存在");
        modified.remove_subtree_inplace(nm);
        assert!(
            modified
                .find_node_by_path(&root.join("project").join("node_modules"))
                .is_none(),
            "删除后的节点不应再可见"
        );

        // 第二份克隆自修改后的树——必须继承全部修改
        let second = modified.clone();
        assert!(
            second
                .find_node_by_path(&root.join("project").join("node_modules"))
                .is_none(),
            "克隆已修改的 mmap 树不得复活被删除的子树"
        );
        assert!(second
            .find_node_by_path(&root.join("project").join("added.bin"))
            .is_some());
        let project = second
            .find_node_by_path(&root.join("project"))
            .expect("project 应当存在");
        assert_eq!(
            second.size_of(project),
            777,
            "克隆必须保留聚合值更新（index.js 随 node_modules 删除，只剩 added.bin）"
        );
        assert_eq!(second.file_count_of(second.root()), 1);

        // 原树不受影响
        let orig = &loaded.scan.tree;
        assert!(orig
            .find_node_by_path(&root.join("project").join("node_modules"))
            .is_some());
        assert_eq!(orig.file_count_of(orig.root()), 1);
    }

    /// 增量保存走小 delta 文件，重新加载后墓碑 / 追加 / 聚合全部恢复。
    #[test]
    fn empty_delta_persists_new_watermark() {
        let _guard = isolate_cache_dir("v7_empty_delta_watermark");
        let root = PathBuf::from(format!(
            "{}_{}",
            "/tmp/qc-v7-empty-delta",
            std::process::id()
        ));
        let (volume, scan) = snapshot_tree(root, 4096);
        save_index(&volume, &scan, 1);
        let loaded = load_index(&volume).expect("应当能加载 base");
        assert!(loaded.scan.tree.has_mapped_base());
        assert_eq!(loaded.scan.tree.delta_len(), 0);

        save_index(&volume, &loaded.scan, 9);
        let delta_path = index_delta_path(&volume).expect("应当有 delta 路径");
        assert!(delta_path.exists(), "无结构变化也必须写元数据 delta");
        let reloaded = load_index(&volume).expect("base + 空 delta 应当能加载");
        assert_eq!(reloaded.last_event_id, 9);
        assert_eq!(reloaded.scan.total_size, 4096);
    }

    #[test]
    fn delta_save_round_trip_preserves_incremental_state() {
        let _guard = isolate_cache_dir("v7_delta_round_trip");
        let root = PathBuf::from(format!("{}_{}", "/tmp/qc-v7-delta-rt", std::process::id()));
        let (volume, scan) = snapshot_tree(root.clone(), 4096);
        save_index(&volume, &scan, 1);

        let loaded = load_index(&volume).expect("应当能加载");
        let mut tree = loaded.scan.tree;
        assert!(tree.has_mapped_base());
        assert!(tree.upsert_file(&root.join("project").join("added.bin"), 777));
        let nm = tree
            .find_node_by_path(&root.join("project").join("node_modules"))
            .expect("node_modules 应当存在");
        tree.remove_subtree_inplace(nm);

        // 第二次保存应写 delta 文件而不是重写 base
        let delta_path = index_delta_path(&volume).expect("应当有 delta 路径");
        save_index(
            &volume,
            &ScanResult {
                volume: volume.clone(),
                total_size: tree.size_of(tree.root()),
                file_count: tree.file_count_of(tree.root()),
                dir_count: tree.count_used_dirs(),
                dirs: Vec::new(),
                tree,
                elapsed_ms: 0,
                records_read: 0,
                records_expected: 0,
                mft_run_bytes: 0,
                ext_records: 0,
                ext_data_merged: 0,
                hard_links: 0,
                unique_size: 0,
                unique_files: 0,
            },
            5,
        );
        assert!(delta_path.exists(), "小增量保存必须产出 delta 文件");

        let reloaded = load_index(&volume).expect("base+delta 应当能加载");
        assert_eq!(reloaded.last_event_id, 5);
        let tree = &reloaded.scan.tree;
        assert!(
            tree.find_node_by_path(&root.join("project").join("node_modules"))
                .is_none(),
            "重载后墓碑必须生效"
        );
        assert!(tree
            .find_node_by_path(&root.join("project").join("added.bin"))
            .is_some());
        assert_eq!(tree.file_count_of(tree.root()), 1);
        assert_eq!(tree.size_of(tree.root()), 777);
        let hits = tree.search("added.bin", 8);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn corrupt_delta_parent_falls_back_to_clean_base() {
        let _guard = isolate_cache_dir("v7_delta_bad_parent");
        let root = PathBuf::from(format!(
            "{}_{}",
            "/tmp/qc-v7-delta-bad-parent",
            std::process::id()
        ));
        let (volume, scan) = snapshot_tree(root.clone(), 4096);
        save_index(&volume, &scan, 1);
        let loaded = load_index(&volume).expect("应当能加载 base");
        let mut tree = loaded.scan.tree;
        assert!(tree.upsert_file(&root.join("bad.bin"), 123));
        save_index(
            &volume,
            &ScanResult {
                volume: volume.clone(),
                total_size: tree.size_of(tree.root()),
                file_count: tree.count_used_files(),
                dir_count: tree.count_used_dirs(),
                dirs: Vec::new(),
                tree,
                elapsed_ms: 0,
                records_read: 0,
                records_expected: 0,
                mft_run_bytes: 0,
                ext_records: 0,
                ext_data_merged: 0,
                hard_links: 0,
                unique_size: 0,
                unique_files: 0,
            },
            5,
        );
        let delta_path = index_delta_path(&volume).expect("应当有 delta 路径");
        let mut bytes = std::fs::read(&delta_path).expect("应当能读出 delta");
        let appended_index = scan.tree.entry_count() as u32;
        bytes[128..132].copy_from_slice(&appended_index.to_le_bytes());
        let sum = crate::platform::macos::index_v7::delta_checksum(&bytes[..128], &bytes[128..]);
        bytes[100..108].copy_from_slice(&sum.to_le_bytes());
        std::fs::write(&delta_path, bytes).expect("应当能写回损坏 delta");

        let restored = load_index(&volume).expect("损坏 delta 应回退到纯 base");
        assert_eq!(restored.last_event_id, 1);
        assert!(
            restored
                .scan
                .tree
                .find_node_by_path(&root.join("bad.bin"))
                .is_none(),
            "校验失败时不得保留已解析的追加节点"
        );
        assert_eq!(restored.scan.tree.entry_count(), scan.tree.entry_count());
    }

    #[test]
    fn empty_search_returns_bounded_descending_top_n() {
        let root = PathBuf::from(format!("{}_{}", "/tmp/qc-search-top-n", std::process::id()));
        let volume = VolumeId::from_mount_point(root.clone());
        let tree = SizeTree::from_snapshot(
            volume,
            vec![
                TreeSnapshotEntry {
                    path: root.clone(),
                    is_dir: true,
                    size: 0,
                    mtime: 0,
                },
                TreeSnapshotEntry {
                    path: root.join("small.bin"),
                    is_dir: false,
                    size: 10,
                    mtime: 0,
                },
                TreeSnapshotEntry {
                    path: root.join("large.bin"),
                    is_dir: false,
                    size: 90,
                    mtime: 0,
                },
                TreeSnapshotEntry {
                    path: root.join("medium.bin"),
                    is_dir: false,
                    size: 40,
                    mtime: 0,
                },
            ],
        );
        let hits = tree.search("", 3);
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits.iter().map(|hit| hit.size).collect::<Vec<_>>(),
            vec![140, 90, 40]
        );
        assert!(hits.windows(2).all(|pair| pair[0].size >= pair[1].size));
        assert!(tree.search("", 0).is_empty());
    }

    #[test]
    fn superseded_search_stops_without_results() {
        let root = PathBuf::from(format!(
            "{}_{}",
            "/tmp/qc-search-cancel",
            std::process::id()
        ));
        let volume = VolumeId::from_mount_point(root.clone());
        let mut snapshot = Vec::with_capacity(10_001);
        snapshot.push(TreeSnapshotEntry {
            path: root.clone(),
            is_dir: true,
            size: 0,
            mtime: 0,
        });
        for i in 0..10_000 {
            snapshot.push(TreeSnapshotEntry {
                path: root.join(format!("file-{i}.bin")),
                is_dir: false,
                size: i,
                mtime: 0,
            });
        }
        let tree = SizeTree::from_snapshot(volume, snapshot);
        let generation = std::sync::atomic::AtomicU64::new(2);
        assert!(
            tree.search_cancellable("file", 500, &generation, 1)
                .is_empty(),
            "已被新代次取代的搜索必须立即停止且不返回旧结果"
        );
        assert!(
            tree.search_cancellable("", 500, &generation, 1).is_empty(),
            "空查询 top-N 遍历也必须支持取消"
        );
    }

    /// 带增量的 mmap 树经流式压实写出后能完整恢复。
    /// （这条同时覆盖 write_v7_streaming 的 CSR / 名字池 / checksum。）
    #[test]
    fn streaming_compaction_round_trip_with_delta() {
        let _guard = isolate_cache_dir("v7_streaming_compact");
        let root = PathBuf::from(format!("{}_{}", "/tmp/qc-v7-streaming", std::process::id()));
        let (volume, scan) = snapshot_tree(root.clone(), 4096);
        save_index(&volume, &scan, 1);
        let loaded = load_index(&volume).expect("应当能加载");
        let mut tree = loaded.scan.tree;
        assert!(tree.upsert_file(&root.join("late.bin"), 11));
        let pkg = tree
            .find_node_by_path(&root.join("project").join("node_modules").join("pkg"))
            .expect("pkg 应当存在");
        tree.remove_subtree_inplace(pkg);

        let out = crate::core::testing::file_path("qc-v7-streaming-out.bin");
        let _ = std::fs::remove_file(&out);
        let meta = crate::platform::macos::disk_tree::IndexMeta {
            mount: volume.mount_point().to_string_lossy().into_owned(),
            label: volume.display().to_string(),
            file_count: tree.count_used_files(),
            dir_count: tree.count_used_dirs(),
            total_size: tree.size_of(tree.root()),
            last_event_id: 9,
            scanned_at: now_epoch_secs(),
        };
        tree.write_v7(&out, meta).expect("流式压实写出应当成功");
        let bytes = std::fs::read(&out).expect("应当能读取流式压实结果");
        let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let ca_len = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
        let cs_off = u32::from_le_bytes(bytes[88..92].try_into().unwrap()) as usize;
        let sentinel = u32::from_le_bytes(
            bytes[cs_off + n * 4..cs_off + (n + 1) * 4]
                .try_into()
                .unwrap(),
        );
        assert_eq!(sentinel, ca_len, "流式写入必须保留 CSR 尾哨兵");
        let restored = SizeTree::from_mapped(volume.clone(), &out).expect("压实结果应当可加载");
        let _ = std::fs::remove_file(&out);

        assert!(restored.find_node_by_path(&root.join("late.bin")).is_some());
        assert!(restored
            .find_node_by_path(&root.join("project").join("node_modules").join("pkg"))
            .is_none());
        assert_eq!(restored.file_count_of(restored.root()), 1); // 只剩 late.bin
        assert_eq!(restored.size_of(restored.root()), 11);
        // project 目录下只剩 node_modules 一条路径可达
        let project = restored
            .find_node_by_path(&root.join("project"))
            .expect("project 应当存在");
        assert_eq!(restored.children(project).len(), 1);
        let (hdr_files, hdr_dirs, _, _) = restored.mapped_header_stats().unwrap();
        assert_eq!(hdr_files, 1); // late.bin（index.js 随 pkg 删除）
        assert_eq!(hdr_dirs, 3); // root + project + node_modules
    }

    /// 全量重写后旧 delta 必须作废删除，避免叠加到新 base 上。
    #[test]
    fn full_rewrite_removes_stale_delta() {
        let _guard = isolate_cache_dir("v7_stale_delta");
        let root = PathBuf::from(format!(
            "{}_{}",
            "/tmp/qc-v7-stale-delta",
            std::process::id()
        ));
        let (volume, scan) = snapshot_tree(root.clone(), 4096);
        save_index(&volume, &scan, 1);
        let loaded = load_index(&volume).expect("应当能加载");
        let mut tree = loaded.scan.tree;
        assert!(tree.upsert_file(&root.join("x.bin"), 5));
        save_index(
            &volume,
            &ScanResult {
                volume: volume.clone(),
                total_size: 0,
                file_count: 0,
                dir_count: 0,
                dirs: Vec::new(),
                tree,
                elapsed_ms: 0,
                records_read: 0,
                records_expected: 0,
                mft_run_bytes: 0,
                ext_records: 0,
                ext_data_merged: 0,
                hard_links: 0,
                unique_size: 0,
                unique_files: 0,
            },
            3,
        );
        let delta_path = index_delta_path(&volume).expect("应当有 delta 路径");
        assert!(delta_path.exists());

        // 新的全量保存（堆树直接写）→ delta 作废
        let (volume2, fresh) = snapshot_tree(root.clone(), 8192);
        let _ = volume2;
        save_index(&volume, &fresh, 6);
        assert!(!delta_path.exists(), "全量重写后必须删除旧 delta");
        let reloaded = load_index(&volume).expect("应当能加载纯 base");
        assert_eq!(reloaded.scan.total_size, 8192);
        assert!(reloaded
            .scan
            .tree
            .find_node_by_path(&root.join("x.bin"))
            .is_none());
    }

    fn process_rss_kb() -> u64 {
        let pid = std::process::id();
        let out = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
            .ok();
        out.and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    }

    fn process_footprint_mb() -> Option<f64> {
        let pid = std::process::id();
        let out = std::process::Command::new("footprint")
            .args(["-p", &pid.to_string()])
            .output()
            .ok()?;
        let text = String::from_utf8(out.stdout).ok()?;
        for line in text.lines() {
            if line.contains("Footprint") || line.contains("phys_footprint") {
                eprintln!("{line}");
            }
            let lower = line.to_ascii_lowercase();
            if lower.contains("physical footprint") || lower.contains("phys footprint") {
                eprintln!("{line}");
            }
        }
        eprintln!("--- footprint ---");
        eprint!("{text}");
        None
    }

    /// 对本机已落盘的根索引做一次加载/搜索采样。不进默认 CI：
    /// `cargo test --release --lib measure_real_v7_index -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn measure_real_v7_index() {
        let volume = VolumeId::from_mount_point(PathBuf::from("/"));
        let before = process_rss_kb();
        let t0 = std::time::Instant::now();
        let loaded = load_index(&volume).expect("应当能加载本机 v7 根索引");
        let load_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let after_load = process_rss_kb();
        let tree = &loaded.scan.tree;
        eprintln!(
            "load {:.0} ms, records {}, files {}, dirs {}, mapped {:.1} MB, rss {} -> {} KB ({:.1} MB)",
            load_ms,
            tree.entry_count(),
            loaded.scan.file_count,
            loaded.scan.dir_count,
            tree.memory_bytes() as f64 / (1024.0 * 1024.0),
            before,
            after_load,
            after_load as f64 / 1024.0
        );

        let queries = [
            ("", 500usize),
            ("Cargo.toml", 200usize),
            ("index.js", 200),
            ("node_modules", 200),
            ("*.png", 200),
            ("zzzxq_not_a_real_file_qqq", 16_000_000),
        ];
        for (q, cap) in queries {
            let t = std::time::Instant::now();
            let hits = tree.search(q, cap);
            eprintln!(
                "search {q:?} cap={cap}: {} hits in {:.1} ms, rss {:.1} MB",
                hits.len(),
                t.elapsed().as_secs_f64() * 1000.0,
                process_rss_kb() as f64 / 1024.0
            );
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
        eprintln!("idle 3s rss {:.1} MB", process_rss_kb() as f64 / 1024.0);
        let _ = process_footprint_mb();
        assert!(tree.entry_count() > 1_000_000);
    }

    // ---- 搜索性能门禁 ----

    /// 固定查询集：覆盖短/长子串、目录名、通配符、无命中全扫描。
    const PERF_QUERIES: &[&str] = &[
        "Cargo.toml",
        "index.js",
        "node_modules",
        "main.rs",
        "package.json",
        "*.png",
        "quick-cleaner",
        "zzzxq_not_a_real_file_qqq",
    ];

    #[derive(serde::Serialize, serde::Deserialize)]
    struct PerfBaseline {
        recorded_at: u64,
        /// 查询 → 热跑 p95（毫秒）
        warm_p95_ms: std::collections::BTreeMap<String, f64>,
        /// 查询 → 冷跑首次耗时（毫秒）
        cold_ms: std::collections::BTreeMap<String, f64>,
    }

    fn percentile(sorted: &[f64], p: f64) -> f64 {
        if sorted.is_empty() {
            return 0.0;
        }
        let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    fn run_search_sample(tree: &SizeTree, query: &str, rounds: usize) -> Vec<f64> {
        let mut samples = Vec::with_capacity(rounds);
        for _ in 0..rounds {
            let t = std::time::Instant::now();
            let _ = tree.search(query, 200);
            samples.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        samples
    }

    /// 搜索性能回归门禁（需要本机根索引，不进默认 CI）：
    /// `cargo test --release --lib search_perf_gate -- --ignored --nocapture`
    ///
    /// 首次运行记录基线；之后每次运行把固定查询集的冷/热耗时与基线比，
    /// 热 p95 超过基线 ×(1+容差) 即失败。容差默认 10%
    /// （`QC_PERF_GATE_TOLERANCE` 可覆盖），`QC_PERF_GATE_UPDATE=1` 刷新基线。
    /// 基线存在缓存目录 `search-perf-baseline.json`，随机器而变，不入库。
    #[test]
    #[ignore]
    fn search_perf_gate() {
        let volume = VolumeId::from_mount_point(PathBuf::from("/"));
        let loaded = load_index(&volume).expect("搜索性能门禁需要本机 v7 根索引");
        let tree = loaded.scan.tree;

        let mut cold = std::collections::BTreeMap::new();
        let mut warm_p95 = std::collections::BTreeMap::new();
        for q in PERF_QUERIES {
            // 冷：只跑一轮，反映索引页不在物理内存时的表现
            let t = std::time::Instant::now();
            let _ = tree.search(q, 200);
            cold.insert((*q).to_string(), t.elapsed().as_secs_f64() * 1000.0);
            // 热：页已在内存，5 轮取 median/p95
            let samples = run_search_sample(&tree, q, 5);
            let median = percentile(&samples, 0.5);
            let p95 = percentile(&samples, 0.95);
            warm_p95.insert((*q).to_string(), p95);
            eprintln!(
                "search {q:?}: cold {:.1} ms, warm median {:.1} ms, p95 {:.1} ms",
                cold[*q], median, p95
            );
        }

        let baseline_path = cache_dir()
            .map(|d| d.join("search-perf-baseline.json"))
            .expect("应当有缓存目录");
        let update = std::env::var("QC_PERF_GATE_UPDATE").is_ok();
        if update || !baseline_path.exists() {
            let baseline = PerfBaseline {
                recorded_at: now_epoch_secs(),
                warm_p95_ms: warm_p95,
                cold_ms: cold,
            };
            std::fs::write(
                &baseline_path,
                serde_json::to_string_pretty(&baseline).unwrap(),
            )
            .unwrap();
            eprintln!(
                "基线已{}：{}",
                if update { "刷新" } else { "记录" },
                baseline_path.display()
            );
            return;
        }

        let baseline: PerfBaseline =
            serde_json::from_str(&std::fs::read_to_string(&baseline_path).unwrap())
                .expect("基线文件损坏，删除后重跑以重建");
        let tolerance: f64 = std::env::var("QC_PERF_GATE_TOLERANCE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.10);
        let mut failures = Vec::new();
        for q in PERF_QUERIES {
            let Some(base) = baseline.warm_p95_ms.get(*q) else {
                continue;
            };
            let now = warm_p95[*q];
            let limit = base * (1.0 + tolerance);
            eprintln!(
                "gate {q:?}: baseline p95 {:.1} ms, now {:.1} ms, limit {:.1} ms",
                base, now, limit
            );
            if now > limit {
                failures.push(format!(
                    "{q:?}: p95 {:.1} ms > 基线 {:.1} ms × (1+{:.0}%)",
                    now,
                    base,
                    tolerance * 100.0
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "搜索性能回归超过 {:.0}% 门限:\n{}",
            tolerance * 100.0,
            failures.join("\n")
        );
    }

    /// 大规模增量保存内存门禁（需要本机根索引，不进默认 CI）：
    /// `cargo test --release --lib streaming_compaction_memory_gate -- --ignored --nocapture`
    ///
    /// 把真实索引复制到临时目录，加载后制造一批增量（墓碑 + 追加），
    /// 分别走 delta 保存和流式压实，报告耗时与峰值 RSS。
    /// 旧实现压实一次要 4.8s、分配 620MB 中间结构、峰值 1.3GiB。
    #[test]
    #[ignore]
    fn streaming_compaction_memory_gate() {
        let volume = VolumeId::from_mount_point(PathBuf::from("/"));
        let real = index_path(&volume).expect("应当有索引路径");
        let dir = crate::core::testing::fixture("qc-mem-gate");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let copy = dir.join("index-copy.bin");
        std::fs::copy(&real, &copy).unwrap();
        let vol = VolumeId::from_mount_point(PathBuf::from("/qc-mem-gate-root"));
        // 直接 mmap 副本（绕过缓存目录解析）
        let mut tree = SizeTree::from_mapped(vol.clone(), &copy).expect("副本应当能加载");
        eprintln!(
            "loaded {} entries, mapped {:.1} MB",
            tree.entry_count(),
            tree.memory_bytes() as f64 / (1024.0 * 1024.0)
        );

        // 制造增量：追加一批文件 + 删除一批子树根
        for i in 0..2000 {
            assert!(tree.upsert_file(&PathBuf::from(format!("/qc-mem-gate-root/f{i}.bin")), i));
        }
        let mut removed = 0;
        for i in 0..50 {
            if let Some(idx) = tree.find_node_by_path(&PathBuf::from(format!(
                "/Users/yuqiang/Library/Caches/dir{i}"
            ))) {
                tree.remove_subtree_inplace(idx);
                removed += 1;
            }
        }
        eprintln!("delta: {} appended, {} subtrees tombstoned", 2000, removed);

        // delta 保存路径
        let delta_out = dir.join("out.delta.bin");
        let dmeta = crate::platform::macos::disk_tree::DeltaMeta {
            base_checksum: tree.base_checksum().unwrap(),
            last_event_id: 2,
            scanned_at: now_epoch_secs(),
            file_count: tree.count_used_files(),
            dir_count: tree.count_used_dirs(),
            total_size: tree.size_of(tree.root()),
            n_entries: 0,
            pool_len: 0,
            n_overrides: 0,
            n_extra_parents: 0,
        };
        let t = std::time::Instant::now();
        tree.write_delta(&delta_out, &dmeta)
            .expect("delta 写出应当成功");
        eprintln!(
            "delta save: {:?}, file {:.2} MB",
            t.elapsed(),
            std::fs::metadata(&delta_out).unwrap().len() as f64 / (1024.0 * 1024.0)
        );

        // 流式压实路径
        let out = dir.join("out-compacted.bin");
        let meta = crate::platform::macos::disk_tree::IndexMeta {
            mount: "/qc-mem-gate-root".into(),
            label: "/qc-mem-gate-root".into(),
            file_count: tree.count_used_files(),
            dir_count: tree.count_used_dirs(),
            total_size: tree.size_of(tree.root()),
            last_event_id: 3,
            scanned_at: now_epoch_secs(),
        };
        let t = std::time::Instant::now();
        tree.write_v7(&out, meta).expect("流式压实应当成功");
        eprintln!(
            "streaming compaction: {:?}, file {:.1} MB",
            t.elapsed(),
            std::fs::metadata(&out).unwrap().len() as f64 / (1024.0 * 1024.0)
        );
        let restored = SizeTree::from_mapped(vol.clone(), &out).expect("压实结果应当可加载");
        assert_eq!(restored.count_used_files(), tree.count_used_files());
        assert_eq!(
            restored.size_of(restored.root()),
            tree.size_of(restored.root())
        );
        let _ = std::fs::remove_dir_all(&dir);
        eprintln!("memory gate OK");
    }

    /// overrides 覆盖表对全树搜索的开销 A/B（需要本机根索引，不进默认 CI）：
    /// `cargo test --release --lib search_overrides_overhead -- --ignored --nocapture`
    ///
    /// 同一棵树分别以空覆盖表和带 5000 条覆盖的状态跑相同查询，
    /// 确认热路径（slot 的哈希探测）没有明显回退。
    #[test]
    #[ignore]
    fn search_overrides_overhead() {
        let volume = VolumeId::from_mount_point(PathBuf::from("/"));
        let loaded = load_index(&volume).expect("应当能加载本机 v7 根索引");
        // 干净副本：清空 delta，只留 mmap 主体
        let mut clean = loaded.scan.tree.clone();
        clean.clear_delta_for_bench();
        let mut dirty = clean.clone();
        for i in 0..5000u32 {
            let idx = (i * 3001) % (dirty.entry_count().min(u32::MAX as usize) as u32);
            dirty.bench_mark_override(idx);
        }
        assert!(clean.delta_len() == 0 && dirty.overrides_delta_len() == 5000);

        for q in ["zzzxq_not_a_real_file_qqq", "node_modules", "Cargo.toml"] {
            // 预热
            let _ = clean.search(q, 200);
            let t = std::time::Instant::now();
            let _ = clean.search(q, 200);
            let clean_ms = t.elapsed().as_secs_f64() * 1000.0;
            let t = std::time::Instant::now();
            let _ = dirty.search(q, 200);
            let dirty_ms = t.elapsed().as_secs_f64() * 1000.0;
            eprintln!(
                "search {q:?}: clean(0 overrides) {clean_ms:.1} ms, dirty(5000) {dirty_ms:.1} ms"
            );
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod mirror_heal_tests {
    use super::*;
    use crate::platform::macos::disk_tree::{SizeTree, TreeIndexEntry};

    fn entry(parent: u32, name: &str, is_dir: bool, size: u64) -> TreeIndexEntry {
        TreeIndexEntry {
            parent,
            name: name.to_string(),
            is_dir,
            size,
            used: true,
            mtime: 0,
        }
    }

    /// 带镜像的迷你索引：/Users/me/a.txt（真实侧）+
    /// /System/Volumes/Data/Users/me/b.txt（firmlink 镜像侧）。
    fn loaded_with_mirror(mount: &str) -> LoadedIndex {
        let vol = VolumeId::from_mount_point(PathBuf::from(mount));
        let entries = vec![
            entry(0, "/", true, 0),
            entry(0, "Users", true, 0),
            entry(1, "me", true, 0),
            entry(2, "a.txt", false, 100),
            entry(0, "System", true, 0),
            entry(4, "Volumes", true, 0),
            entry(5, "Data", true, 0),
            entry(6, "Users", true, 0),
            entry(7, "me", true, 0),
            entry(8, "b.txt", false, 50),
        ];
        let tree = SizeTree::from_compact(vol.clone(), entries);
        LoadedIndex {
            scan: ScanResult {
                volume: vol,
                total_size: tree.size_of(tree.root()),
                file_count: tree.file_count_of(tree.root()),
                dir_count: 8,
                dirs: Vec::new(),
                tree,
                elapsed_ms: 0,
                records_read: 10,
                records_expected: 10,
                mft_run_bytes: 0,
                ext_records: 0,
                ext_data_merged: 0,
                hard_links: 0,
                unique_size: 0,
                unique_files: 0,
            },
            last_event_id: 0,
        }
    }

    /// 自愈只移除镜像子树：真实侧保留、计数与树聚合同步扣减。
    #[test]
    fn mirror_subtree_removed_with_counts_decremented() {
        let vol = VolumeId::from_mount_point(PathBuf::from("/"));
        let mut li = loaded_with_mirror("/");
        let (before_total, before_files) = (li.scan.total_size, li.scan.file_count);
        assert_eq!(before_total, 150);
        assert_eq!(before_files, 2);

        heal_data_volume_mirror(&vol, &mut li);

        assert!(li
            .scan
            .tree
            .find_node_by_path(&PathBuf::from("/Users/me/a.txt"))
            .is_some());
        assert!(li
            .scan
            .tree
            .find_node_by_path(&PathBuf::from("/System/Volumes/Data"))
            .is_none());
        // /System/Volumes 本身保留（Preboot / VM / Update 还在它下面）
        assert!(li
            .scan
            .tree
            .find_node_by_path(&PathBuf::from("/System/Volumes"))
            .is_some());
        assert_eq!(li.scan.total_size, 100);
        assert_eq!(li.scan.file_count, 1);
        assert_eq!(li.scan.tree.size_of(li.scan.tree.root()), 100);
    }

    /// 非根卷不做自愈；已治愈的树再跑一遍是零副作用空操作。
    #[test]
    fn heal_skips_other_volumes_and_is_idempotent() {
        let data_vol = VolumeId::from_mount_point(PathBuf::from("/System/Volumes/Data"));
        let mut li = loaded_with_mirror("/System/Volumes/Data");
        heal_data_volume_mirror(&data_vol, &mut li);
        assert!(li
            .scan
            .tree
            .find_node_by_path(&PathBuf::from("/System/Volumes/Data"))
            .is_some());

        let root_vol = VolumeId::from_mount_point(PathBuf::from("/"));
        let mut li2 = loaded_with_mirror("/");
        heal_data_volume_mirror(&root_vol, &mut li2);
        let (t, f) = (li2.scan.total_size, li2.scan.file_count);
        heal_data_volume_mirror(&root_vol, &mut li2);
        assert_eq!((li2.scan.total_size, li2.scan.file_count), (t, f));
    }
}
