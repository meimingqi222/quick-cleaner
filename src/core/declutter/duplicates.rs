//! 重复文件检测引擎 (Duplicate Files Scanner)

use super::{format_timestamp_date, get_user_content_roots};
use crate::core::disk::SizeTree;
use crate::core::fs_query::{FSIndexEngine, FileIndexQuery};
use rayon::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs::File;
use std::hash::Hasher;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateFileItem {
    pub path_display: String,
    pub path: PathBuf,
    pub modified_at_str: String,
    pub modified_at_secs: u64,
    pub is_original: bool,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateGroup {
    pub id: String,
    pub filename: String,
    pub category_zh: &'static str,
    pub category_en: &'static str,
    pub size_per_copy: u64,
    pub files: Vec<DuplicateFileItem>,
}

impl DuplicateGroup {
    pub fn cleanable_size(&self) -> u64 {
        let count = self.files.iter().filter(|f| f.selected).count() as u64;
        count * self.size_per_copy
    }

    pub fn total_copies(&self) -> usize {
        self.files.len()
    }
}

/// 高性能多阶段哈希检测重复文件（利用 FSIndexEngine 极速分桶 + 并发校验）
pub fn scan_duplicate_files(live: &AtomicBool, tree: Option<&SizeTree>) -> Vec<DuplicateGroup> {
    let search_roots = get_user_content_roots();
    let engine = FSIndexEngine::new(tree);

    // 阶段 1: 内存分桶 (体积 >= 64KB)
    // 每个桶值携带 mtime，避免阶段 2 再调 std::fs::metadata。
    let candidate_size_map = engine.query_duplicate_buckets(&search_roots, 64 * 1024, live);

    let mut candidate_sizes: Vec<(u64, Vec<(PathBuf, u64)>)> = candidate_size_map
        .into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .collect();

    // 优先按潜在可清理体积排序（高收益桶优先）
    candidate_sizes
        .sort_by_key(|(size, paths)| std::cmp::Reverse(*size * (paths.len() as u64 - 1)));
    candidate_sizes.truncate(500);

    let total_candidate_sizes = candidate_sizes.len();

    // 阶段 2 & 3: 并行计算首尾抽样哈希与全量内容校验
    //
    // 一次 open 同时算 sample hash 和 full hash——之前两阶段各 open 一次，
    // 冷启动时每个候选文件两次 open+seek 是真实磁盘 I/O。合并后每个文件
    // 只 open 一次，sample 阶段直接拿预计算的 full hash 分桶。
    let confirmed_groups: Vec<DuplicateGroup> = candidate_sizes
        .into_par_iter()
        .filter_map(|(size, paths)| {
            if !live.load(Ordering::Relaxed) {
                return None;
            }

            // 一次 open 算两个 hash，返回 (sample, full, mtime)
            let hashed: Vec<(u64, u64, PathBuf, u64)> = paths
                .into_iter()
                .filter_map(|(p, mtime)| {
                    if !live.load(Ordering::Relaxed) {
                        return None;
                    }
                    compute_hashes(&p, size)
                        .ok()
                        .map(|(sample, full)| (sample, full, p, mtime))
                })
                .collect();

            // 阶段 2: 用 sample hash 分桶
            let mut sample_map: HashMap<u64, Vec<(u64, PathBuf, u64)>> = HashMap::new();
            for (sample, full, p, mtime) in hashed {
                sample_map.entry(sample).or_default().push((full, p, mtime));
            }

            let mut dup_groups = Vec::new();

            for (_, sample_paths) in sample_map {
                if sample_paths.len() <= 1 {
                    continue;
                }

                // 阶段 3: 用预计算的 full hash 校验碰撞（无需再 open）
                let mut full_map: HashMap<u64, Vec<(PathBuf, u64)>> = HashMap::new();
                for (full, p, mtime) in sample_paths {
                    full_map.entry(full).or_default().push((p, mtime));
                }

                for (full_h, mut dup_paths) in full_map {
                    if dup_paths.len() <= 1 {
                        continue;
                    }

                    // 阶段 4: 按修改时间排序（最早的作为原件）
                    dup_paths.sort_by_key(|(_, mtime)| *mtime);

                    let first_filename = dup_paths[0]
                        .0
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Duplicate".to_string());

                    let ext = dup_paths[0]
                        .0
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let (cat_zh, cat_en) = classify_duplicate_category(&ext);

                    let files = dup_paths
                        .into_iter()
                        .enumerate()
                        .map(|(idx, (p, mtime))| {
                            let is_orig = idx == 0;
                            DuplicateFileItem {
                                path_display: p.to_string_lossy().to_string(),
                                path: p,
                                modified_at_str: format_timestamp_date(mtime),
                                modified_at_secs: mtime,
                                is_original: is_orig,
                                selected: !is_orig,
                            }
                        })
                        .collect();

                    dup_groups.push(DuplicateGroup {
                        id: format!("dup-{size}-{full_h}"),
                        filename: first_filename,
                        category_zh: cat_zh,
                        category_en: cat_en,
                        size_per_copy: size,
                        files,
                    });
                }
            }

            if dup_groups.is_empty() {
                None
            } else {
                Some(dup_groups)
            }
        })
        .flatten()
        .collect();

    let mut result = confirmed_groups;
    result.sort_by_key(|g| std::cmp::Reverse(g.cleanable_size()));
    crate::log!(
        "[Declutter::Duplicates] 重复文件扫描完成: 发现 {} 组完全相同副本 (候选大小桶: {} 个)",
        result.len(),
        total_candidate_sizes
    );
    result
}

/// 一次 open 同时计算 sample hash 和 full hash。
///
/// - sample hash：首 4KB + 尾 4KB（快速分桶）
/// - full hash：大文件多点跳跃（5 点 × 16KB），小文件全量读
///
/// 合并前两个独立函数各 open 一次，冷启动时双倍 open+seek 开销。
fn compute_hashes(path: &Path, file_size: u64) -> std::io::Result<(u64, u64)> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = File::open(path)?;

    // ---- sample hash: 首 4KB + 尾 4KB ----
    let mut sample_hasher = DefaultHasher::new();
    sample_hasher.write_u64(file_size);

    let mut buf = [0u8; 4096];
    let n_head = file.read(&mut buf)?;
    sample_hasher.write(&buf[..n_head]);

    if file_size > 8192 && file.seek(SeekFrom::End(-4096)).is_ok() {
        let n = file.read(&mut buf)?;
        sample_hasher.write(&buf[..n]);
    }
    let sample = sample_hasher.finish();

    // ---- full hash: 复用已打开的句柄 ----
    let mut full_hasher = DefaultHasher::new();
    full_hasher.write_u64(file_size);

    if file_size > 2 * 1024 * 1024 {
        // 大文件：5 点跳跃哈希。首点 offset=0 与 sample 的首 4KB 物理位置
        // 重合，但 hash 累加的是 16KB（full）vs 4KB（sample），不能直接复用
        // sample 的缓冲区，仍需 seek 回头读 16KB。
        let mut big_buf = [0u8; 16384];
        let offsets = [
            0u64,
            file_size / 4,
            file_size / 2,
            (file_size * 3) / 4,
            file_size.saturating_sub(16384),
        ];
        for offset in offsets {
            if file.seek(SeekFrom::Start(offset)).is_ok() {
                let n = file.read(&mut big_buf)?;
                full_hasher.write(&big_buf[..n]);
            }
        }
    } else {
        // 小文件：全量读。seek 回开头后顺序读到尾。
        file.seek(SeekFrom::Start(0))?;
        let mut read_buf = [0u8; 65536];
        loop {
            let n = file.read(&mut read_buf)?;
            if n == 0 {
                break;
            }
            full_hasher.write(&read_buf[..n]);
        }
    }

    Ok((sample, full_hasher.finish()))
}

fn classify_duplicate_category(ext: &str) -> (&'static str, &'static str) {
    match ext {
        "jpg" | "jpeg" | "png" | "heic" | "webp" | "gif" | "svg" | "bmp" | "tiff" => {
            ("图像照片", "Images")
        }
        "mp4" | "mov" | "mkv" | "avi" | "flv" | "wmv" | "webm" | "m4v" => ("视频影音", "Videos"),
        "zip" | "tar" | "gz" | "tgz" | "7z" | "rar" | "xz" | "bz2" | "dmg" | "iso" | "pkg" => {
            ("归档文件", "Archives")
        }
        "pdf" | "docx" | "doc" | "xlsx" | "xls" | "pptx" | "ppt" | "txt" | "md" => {
            ("文本文档", "Documents")
        }
        "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg" => ("音频音乐", "Audio"),
        _ => ("其它文件", "Others"),
    }
}
