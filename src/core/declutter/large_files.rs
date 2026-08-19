//! 大文件与旧文件扫描器 (Large & Old Files Scanner)

use super::{format_age_str, get_user_content_roots};
use crate::core::disk::SizeTree;
use crate::core::fs_query::{FSIndexEngine, FileIndexQuery};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LargeFileItem {
    pub filename: String,
    pub path_display: String,
    pub path: PathBuf,
    pub size: u64,
    pub last_accessed_secs: u64,
    pub last_accessed_str: String,
    pub kind_zh: &'static str,
    pub kind_en: &'static str,
    pub icon_type: usize,
    pub selected: bool,
}

/// 扫描用户目录下的大型文件（通过 FSIndexEngine 统一引擎加速）
pub fn scan_large_old_files(
    live: &AtomicBool,
    min_size_bytes: u64,
    tree: Option<&SizeTree>,
) -> Vec<LargeFileItem> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let search_roots = get_user_content_roots();
    let engine = FSIndexEngine::new(tree);
    let files = engine.query_large_files(&search_roots, min_size_bytes, 500, live);

    let mut all_files: Vec<LargeFileItem> = files
        .into_iter()
        .map(|f| {
            let filename = f
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            let ext = f
                .path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            let (kind_zh, kind_en, icon_type) = classify_large_file_type(&ext);
            let age_days = now.saturating_sub(f.mtime) / 86400;
            let last_accessed_str = format_age_str(age_days);

            let selected = age_days >= 90;

            LargeFileItem {
                filename,
                path_display: f.path.to_string_lossy().to_string(),
                path: f.path,
                size: f.size,
                last_accessed_secs: f.mtime,
                last_accessed_str,
                kind_zh,
                kind_en,
                icon_type,
                selected,
            }
        })
        .collect();

    all_files.sort_by_key(|b| std::cmp::Reverse(b.size));
    crate::log!(
        "[Declutter::LargeFiles] 大文件扫描完成: 筛选出 {} 个大文件/旧文件",
        all_files.len()
    );
    all_files
}

pub fn classify_large_file_type(ext: &str) -> (&'static str, &'static str, usize) {
    match ext {
        "mp4" | "mov" | "mkv" | "avi" | "flv" | "wmv" | "webm" | "m4v" => ("视频影音", "Videos", 0),
        "zip" | "tar" | "gz" | "tgz" | "7z" | "rar" | "xz" | "bz2" | "dmg" | "iso" | "pkg" => {
            ("归档文件", "Archives", 1)
        }
        "pdf" | "docx" | "doc" | "xlsx" | "xls" | "pptx" | "ppt" | "txt" | "md" | "epub" => {
            ("文本文档", "Documents", 2)
        }
        _ => ("其它文件", "Others", 3),
    }
}
