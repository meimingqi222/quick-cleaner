//! 开发环境与包管理存储空间测算。
//!
//! 核心原则：
//! 1. 严格区分逻辑大小（Logical Size）与预估独占可释放空间（Exclusive Reclaimable Space）。
//! 2. 硬链接文件只有在链接数 <= 1 时才计入单个环境的独占释放空间。
//! 3. 跨环境批量统计时对文件唯一身份进行去重，避免重复计算。
//!
//! # 为什么要并行
//!
//! 这一段的成本几乎全在「每个文件开一次句柄取链接数」上（`CreateFile` +
//! `GetFileInformationByHandle`），不是遍历本身：真机实测 4.3 万个文件单线程
//! 要 24 秒。所以按目录并行，总计留给并行段之后顺序去重——跨目录的硬链接
//! 只能算一次，这件事没法并行。

use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 递归深度上限。超过就不再往下走：开发产物的目录结构不该有这么深，撞上
/// 基本意味着符号链接环或者恶意构造的树。
const MAX_DEPTH: usize = 32;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AssetStorageSize {
    /// 逻辑累计大小（字节，所有文件的逻辑体积累加）
    pub logical_bytes: u64,
    /// 预估独占释放空间（字节，没有被其他硬链接共享的文件体积）
    pub exclusive_reclaimable_bytes: u64,
    /// 文件总数
    pub file_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct FileEntityId {
    volume_or_dev: u64,
    file_id_or_ino: u64,
}

#[cfg(windows)]
fn get_file_info(path: &Path) -> Option<(u32, FileEntityId)> {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use winapi::um::fileapi::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
    use winapi::um::winbase::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT};
    use winapi::um::winnt::{
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let file = OpenOptions::new()
        .read(true)
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .ok()?;

    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut info) } == 0 {
        return None;
    }

    let id = FileEntityId {
        volume_or_dev: u64::from(info.dwVolumeSerialNumber),
        file_id_or_ino: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    };
    Some((info.nNumberOfLinks, id))
}

#[cfg(unix)]
fn get_file_info(path: &Path) -> Option<(u32, FileEntityId)> {
    use std::os::unix::fs::MetadataExt;
    let md = std::fs::symlink_metadata(path).ok()?;
    let id = FileEntityId {
        volume_or_dev: md.dev(),
        file_id_or_ino: md.ino(),
    };
    Some((md.nlink() as u32, id))
}

#[cfg(not(any(windows, unix)))]
fn get_file_info(_path: &Path) -> Option<(u32, FileEntityId)> {
    None
}

/// 一个目录里每个文件的 `(长度, 链接数, 唯一身份)`。
///
/// 只在批量测算内部用：跨目录去重要拿到全部身份，而这件事必须等并行段结束
/// 之后顺序做。4 万条大约 1 MB，代价可以忽略。
type FileFacts = (u64, u32, Option<FileEntityId>);

/// 扫一个目录：目录自己的体积，以及供跨目录去重用的文件清单。
///
/// 分两段：**遍历**（`read_dir` + `symlink_metadata`，便宜、顺序做）和
/// **取链接数/身份**（每个文件一次句柄调用，贵、并行做）。真机上单线程这一
/// 段是「每个文件几百微秒」，一个 1.4 GB / 1.4 万文件的包要跑十秒——瓶颈
/// 全在这里，不在遍历。
fn scan_dir(dir: &Path) -> (AssetStorageSize, Vec<FileFacts>) {
    let entries = collect_files(dir);

    // 句柄风暴并行化。`get_file_info` 不共享任何状态，纯读。
    let files: Vec<FileFacts> = entries
        .par_iter()
        .map(|(path, len)| match get_file_info(path) {
            Some((links, id)) => (*len, links, Some(id)),
            // 拿不到链接数时保守当作独占体积：宁可高估可释放量，也不要漏报
            // （漏报会让用户以为清不掉）。
            None => (*len, 0, None),
        })
        .collect();

    let mut size = AssetStorageSize::default();
    let mut seen_ids = HashSet::new();
    for (len, links, id) in &files {
        size.logical_bytes += len;
        size.file_count += 1;
        match id {
            // 单个目录内同一个文件也只算一次独占体积。
            Some(id) if *links <= 1 && seen_ids.insert(*id) => {
                size.exclusive_reclaimable_bytes += len;
            }
            Some(_) => {}
            None => size.exclusive_reclaimable_bytes += len,
        }
    }

    (size, files)
}

/// 遍历一个目录，收集 `(路径, 长度)`。**并行**。
///
/// 遍历本身就不便宜：每次 `read_dir` / `symlink_metadata` 都是真实系统调用，
/// 真机实测约 300µs/文件——一个 1.4 万文件的包光走一遍要 4.4 秒，和取链接数
/// 是同一个数量级。只并行后半段等于只优化了一半。
///
/// 每个目录的直接子目录交给 rayon 递归，深度仍以 [`MAX_DEPTH`] 为界：不跟随
/// 符号链接或 Reparse Point，防止死循环和越界统计。
fn collect_files(dir: &Path) -> Vec<(PathBuf, u64)> {
    fn descend(dir: &Path, depth: usize) -> Vec<(PathBuf, u64)> {
        if depth > MAX_DEPTH {
            return Vec::new();
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut files = Vec::new();
        let mut subdirs = Vec::new();
        for entry in entries.flatten() {
            let entry_path = entry.path();
            let Ok(md) = std::fs::symlink_metadata(&entry_path) else {
                continue;
            };
            if md.is_symlink() {
                continue;
            }
            if md.is_dir() {
                subdirs.push(entry_path);
            } else if md.is_file() {
                files.push((entry_path, md.len()));
            }
        }
        let nested: Vec<Vec<(PathBuf, u64)>> = subdirs
            .par_iter()
            .map(|subdir| descend(subdir, depth + 1))
            .collect();
        files.extend(nested.into_iter().flatten());
        files
    }
    descend(dir, 0)
}

/// 批量测算多个目录，并给出所有目录合并后的独占可释放体积（去重）。
pub fn measure_batch_storage(dirs: &[PathBuf]) -> (AssetStorageSize, Vec<AssetStorageSize>) {
    // 逐目录并行。单目录内部不再切分：`par_iter` 已经能让各环境同时走，
    // 而再往每个文件的粒度切会让 `get_file_info` 的句柄风暴变成调度开销。
    let scans: Vec<(AssetStorageSize, Vec<FileFacts>)> =
        dirs.par_iter().map(|dir| scan_dir(dir)).collect();

    // 总计顺序去重：跨目录的硬链接只能算一次，这一步不能并行。
    let mut total = AssetStorageSize::default();
    let mut seen = HashSet::new();
    let mut per_dir = Vec::with_capacity(scans.len());
    for (size, files) in scans {
        for (len, links, id) in files {
            match id {
                Some(id) => {
                    if seen.insert(id) {
                        total.logical_bytes += len;
                        total.file_count += 1;
                        if links <= 1 {
                            total.exclusive_reclaimable_bytes += len;
                        }
                    }
                }
                None => {
                    total.logical_bytes += len;
                    total.file_count += 1;
                    total.exclusive_reclaimable_bytes += len;
                }
            }
        }
        per_dir.push(size);
    }

    (total, per_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_measure_empty_or_nonexistent() {
        let (total, per_dir) =
            measure_batch_storage(&[PathBuf::from("nonexistent_path_quick_cleaner_test")]);
        assert_eq!(total.logical_bytes, 0);
        assert_eq!(total.file_count, 0);
        assert_eq!(per_dir.len(), 1);
        assert_eq!(per_dir[0], AssetStorageSize::default());
    }

    #[test]
    fn test_measure_real_dir_and_hardlink() {
        let temp_dir = std::env::temp_dir().join("qc_dev_storage_test");
        let _ = std::fs::remove_dir_all(&temp_dir);
        let env_a = temp_dir.join("env_a");
        let env_b = temp_dir.join("env_b");
        std::fs::create_dir_all(&env_a).unwrap();
        std::fs::create_dir_all(&env_b).unwrap();

        // 1. 在 env_a 写入一个独占文件 100 字节
        let file_a = env_a.join("exclusive.txt");
        std::fs::write(&file_a, vec![0u8; 100]).unwrap();

        // 2. 在 env_a 写入一个共享源文件 200 字节，并在 env_b 创建硬链接
        let file_shared_a = env_a.join("shared.txt");
        std::fs::write(&file_shared_a, vec![1u8; 200]).unwrap();
        let file_shared_b = env_b.join("shared_link.txt");
        let has_hardlink = std::fs::hard_link(&file_shared_a, &file_shared_b).is_ok();

        let (size_a, per_dir) = measure_batch_storage(std::slice::from_ref(&env_a));
        assert_eq!(size_a.logical_bytes, 300);
        assert_eq!(size_a.file_count, 2);
        assert_eq!(per_dir.len(), 1);
        assert_eq!(per_dir[0], size_a, "单项时逐项与总计应该一致");

        if has_hardlink {
            // shared.txt 有两个硬链接，因此在单环境统计中不计入独占释放空间
            assert_eq!(per_dir[0].exclusive_reclaimable_bytes, 100);

            let (total_batch, per_dir) = measure_batch_storage(&[env_a.clone(), env_b.clone()]);
            // 批量去重后，总逻辑体积应为 100 + 200 = 300（同一文件只统计一次）
            assert_eq!(total_batch.logical_bytes, 300);
            assert_eq!(per_dir.len(), 2);
            assert_eq!(
                per_dir[0].exclusive_reclaimable_bytes, 100,
                "共享文件不计入任何一方的独占体积"
            );
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn per_dir_sizes_stay_aligned_with_the_input_order() {
        // 调用方按位置把体积贴回条目上，所以并行之后顺序必须不变。
        let root = std::env::temp_dir().join("qc_dev_storage_order");
        let _ = std::fs::remove_dir_all(&root);
        for (name, bytes) in [("a", 10usize), ("b", 20), ("c", 30), ("d", 40)] {
            let dir = root.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("f"), vec![0u8; bytes]).unwrap();
        }
        let dirs: Vec<PathBuf> = ["a", "b", "c", "d"]
            .iter()
            .map(|name| root.join(name))
            .collect();

        let (total, per_dir) = measure_batch_storage(&dirs);
        let sizes: Vec<u64> = per_dir.iter().map(|size| size.logical_bytes).collect();
        assert_eq!(sizes, vec![10, 20, 30, 40]);
        assert_eq!(total.logical_bytes, 100);
        assert_eq!(total.file_count, 4);

        let _ = std::fs::remove_dir_all(&root);
    }
}
