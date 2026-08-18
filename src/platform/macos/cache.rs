//! macOS 扫描结果缓存
//!
//! M4 的第一步：把扫描结果序列化到磁盘，下次启动时快速恢复。
//!
//! # 缓存策略
//!
//! - 缓存文件：`~/Library/Application Support/QuickCleaner/scan-cache.json`
//! - 存储内容：卷标识、总大小、文件数、目录数、扫描时间、顶层目录摘要
//! - 失效条件：缓存文件的修改时间与扫描时间不一致，或缓存超过 24 小时
//!
//! # 测试隔离
//!
//! `QUICKCLEANER_CACHE_DIR` 环境变量可覆盖缓存目录。测试用它指向临时目录，
//! 避免覆盖用户真实的缓存文件——`scan_volume` 在扫描完成后会无条件 `save()`，
//! 不隔离的话跑一次 `cargo test` 就把用户的 `scan-cache.json` 冲掉。
//!
//! # FSEvents 增量
//!
//! 完整索引由 `fsevents` 模块保存 FSEvents 水位。没有变化时直接复用；创建、
//! 删除和修改事件只重扫受影响子树，重命名、事件丢失或历史日志不可用时回退全量扫描。

use crate::core::disk::{ScanResult, SizeTree, TreeIndexEntry, VolumeId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// 缓存的单个目录摘要。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CachedDir {
    pub name: String,
    pub size: u64,
    pub file_count: u64,
}

/// 扫描结果的缓存条目。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanCache {
    /// 卷的挂载点路径（用作缓存 key）
    pub volume_mount: String,
    /// 卷的展示标签
    pub volume_label: String,
    /// 总占用大小
    pub total_size: u64,
    /// 文件总数
    pub file_count: u64,
    /// 目录总数
    pub dir_count: u64,
    /// 扫描耗时（毫秒）
    pub elapsed_ms: u64,
    /// 扫描完成的时间戳（Unix epoch 秒）
    pub scanned_at: u64,
    /// 顶层目录摘要（最多 100 个，按大小降序）
    pub top_dirs: Vec<CachedDir>,
}

impl ScanCache {
    /// 缓存有效期：24 小时。
    const MAX_AGE_SECS: u64 = 24 * 60 * 60;

    /// 缓存目录路径。
    ///
    /// 优先读 `QUICKCLEANER_CACHE_DIR` 环境变量（测试隔离用），否则落到
    /// `~/Library/Application Support/QuickCleaner`。环境变量指向的目录
    /// 不存在时会自动创建。
    fn cache_dir() -> Option<PathBuf> {
        if let Ok(custom) = std::env::var("QUICKCLEANER_CACHE_DIR") {
            let dir = PathBuf::from(custom);
            std::fs::create_dir_all(&dir).ok()?;
            return Some(dir);
        }
        let home = dirs::home_dir()?;
        let dir = home
            .join("Library")
            .join("Application Support")
            .join("QuickCleaner");
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir)
    }

    /// 缓存文件路径。
    fn cache_path() -> Option<PathBuf> {
        Self::cache_dir().map(|d| d.join("scan-cache.json"))
    }

    /// 保存扫描结果到缓存。
    pub fn save(&self) {
        if let Some(path) = Self::cache_path() {
            if let Ok(json) = serde_json::to_string(self) {
                let _ = std::fs::write(&path, json);
            }
        }
    }

    /// 加载缓存。如果缓存不存在、过期或损坏，返回 `None`。
    pub fn load_for(volume: &VolumeId) -> Option<Self> {
        let path = Self::cache_path()?;
        let json = std::fs::read_to_string(&path).ok()?;
        let cache: Self = serde_json::from_str(&json).ok()?;

        // 检查卷是否匹配
        if cache.volume_mount != volume.mount_point().to_string_lossy() {
            return None;
        }

        // 检查是否过期
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if now > cache.scanned_at && now - cache.scanned_at > Self::MAX_AGE_SECS {
            return None;
        }

        Some(cache)
    }

    /// 从扫描结果构建缓存条目。
    pub fn from_scan(
        volume: &VolumeId,
        scan: &crate::core::disk::ScanResult,
        top_dirs: Vec<CachedDir>,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            volume_mount: volume.mount_point().to_string_lossy().to_string(),
            volume_label: volume.display().to_string(),
            total_size: scan.total_size,
            file_count: scan.file_count,
            dir_count: scan.dir_count,
            elapsed_ms: scan.elapsed_ms,
            scanned_at: now,
            top_dirs,
        }
    }
}

/// 完整目录索引的持久化格式。
///
/// 与上面的首屏摘要缓存不同，这个索引保存所有文件和目录，供垃圾规则查询
/// 以及 FSEvents 增量更新使用。路径按卷拆分，避免外接盘互相覆盖。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistedIndex {
    pub version: u32,
    pub volume_mount: String,
    pub volume_label: String,
    pub last_event_id: u64,
    pub scanned_at: u64,
    pub entries: Vec<TreeIndexEntry>,
}

pub struct LoadedIndex {
    pub scan: ScanResult,
    pub last_event_id: u64,
}

const INDEX_VERSION: u32 = 5;

/// 串行化索引写入，并记录本进程已落盘的最高 FSEvents 水位。异步保存可能
/// 乱序完成，较旧结果绝不能在较新结果之后覆盖索引文件。
static INDEX_SAVE_WATERMARKS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<PathBuf, u64>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn index_path(volume: &VolumeId) -> Option<PathBuf> {
    let mount = volume.mount_point().to_string_lossy();
    let key: String = mount.bytes().map(|byte| format!("{byte:02x}")).collect();
    ScanCache::cache_dir().map(|dir| dir.join(format!("scan-index-{key}.bin")))
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// 从完整索引恢复运行时 `ScanResult`。
///
/// 索引文件是 lz4 压缩的 bincode，解压后反序列化为 `PersistedIndex`。
pub fn load_index(volume: &VolumeId) -> Option<LoadedIndex> {
    let path = index_path(volume)?;
    let compressed = std::fs::read(path).ok()?;
    let bytes = lz4_flex::decompress_size_prepended(&compressed).ok()?;
    let data: PersistedIndex = bincode::deserialize(&bytes).ok()?;
    if data.version != INDEX_VERSION
        || data.volume_mount != volume.mount_point().to_string_lossy()
        || data.entries.is_empty()
    {
        return None;
    }

    let file_count = data.entries.iter().filter(|entry| !entry.is_dir).count() as u64;
    let dir_count = data.entries.iter().filter(|entry| entry.is_dir).count() as u64;
    let records = data.entries.len() as u64;
    let tree = SizeTree::from_compact(volume.clone(), data.entries);
    let total_size = tree.size_of(tree.root());
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
        last_event_id: data.last_event_id,
    })
}

/// 原子保存完整目录索引，避免应用崩溃留下半个文件。
///
/// 先 bincode 序列化，再 lz4 压缩，写入临时文件后原子 rename。
/// lz4 解压速度比 zstd 快 3-5 倍，启动时加载索引从 ~1s 降到 ~300ms。
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
    let data = PersistedIndex {
        version: INDEX_VERSION,
        volume_mount: volume.mount_point().to_string_lossy().into_owned(),
        volume_label: volume.display().to_string(),
        last_event_id,
        scanned_at: now_epoch_secs(),
        entries: scan.tree.compact_entries(),
    };
    let Ok(bytes) = bincode::serialize(&data) else {
        return;
    };
    let compressed = lz4_flex::compress_prepend_size(&bytes);
    let temporary = path.with_extension("bin.tmp");
    if std::fs::write(&temporary, compressed).is_ok() && std::fs::rename(temporary, &path).is_ok() {
        watermarks.insert(path, last_event_id);
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
    let dir = std::env::temp_dir()
        .join("quick-cleaner-test-cache")
        .join(test_name);
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
    fn index_tree_round_trip_preserves_sizes() {
        let root = PathBuf::from("/tmp/qc-index-root");
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
    fn cache_round_trip() {
        let _guard = isolate_cache_dir("cache_round_trip");

        let cache = ScanCache {
            volume_mount: "/".to_string(),
            volume_label: "/".to_string(),
            total_size: 123456789,
            file_count: 1000,
            dir_count: 100,
            elapsed_ms: 5000,
            scanned_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            top_dirs: vec![CachedDir {
                name: "Library".to_string(),
                size: 50000000,
                file_count: 500,
            }],
        };
        cache.save();

        let vol = VolumeId::from_mount_point(PathBuf::from("/"));
        let loaded = ScanCache::load_for(&vol);
        assert!(loaded.is_some(), "缓存应当能加载");
        let loaded = loaded.unwrap();
        assert_eq!(loaded.total_size, cache.total_size);
        assert_eq!(loaded.file_count, cache.file_count);
        assert_eq!(loaded.top_dirs.len(), 1);
        assert_eq!(loaded.top_dirs[0].name, "Library");
    }

    #[test]
    fn older_async_save_cannot_overwrite_newer_watermark() {
        let _guard = isolate_cache_dir("index_watermark_order");
        let volume = VolumeId::from_mount_point(PathBuf::from("/tmp/qc-watermark"));
        let newer = test_scan(volume.clone(), 8192);
        let older = test_scan(volume.clone(), 4096);

        save_index(&volume, &newer, 20);
        save_index(&volume, &older, 10);

        let loaded = load_index(&volume).expect("索引应当能够重新加载");
        assert_eq!(loaded.last_event_id, 20);
        assert_eq!(loaded.scan.total_size, 8192);
    }
}
