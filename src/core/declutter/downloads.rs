//! 下载项扫描器 (Downloads Scanner)

use crate::core::disk::SizeTree;
use crate::core::fs_query::{FSIndexEngine, FileIndexQuery, QueryFilter};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadItem {
    pub filename: String,
    pub path: PathBuf,
    pub size: u64,
    pub modified_at: u64,
    pub downloaded_at_str: String,
    pub kind_zh: &'static str,
    pub kind_en: &'static str,
    pub selected: bool,
}

/// 扫描用户的 Downloads 文件夹（通过 FSIndexEngine 统一引擎加速）
pub fn scan_downloads_folder(live: &AtomicBool, tree: Option<&SizeTree>) -> Vec<DownloadItem> {
    let download_dir = match dirs::download_dir() {
        Some(d) if d.exists() => d,
        _ => return Vec::new(),
    };

    let engine = FSIndexEngine::new(tree);
    let filter = QueryFilter::new(vec![download_dir]).max_depth(4);
    let files = engine.query_files(&filter, live);

    let now_secs = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut items: Vec<DownloadItem> = files
        .into_iter()
        .map(|f| {
            let filename = f
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            let (kind_zh, kind_en) = classify_download_extension(&f.path);
            let time_str = format_relative_age(f.mtime, now_secs);

            DownloadItem {
                filename,
                path: f.path,
                size: f.size,
                modified_at: f.mtime,
                downloaded_at_str: time_str,
                kind_zh,
                kind_en,
                selected: false,
            }
        })
        .collect();

    // 默认按修改时间降序（最新下载在前）
    items.sort_by_key(|it| std::cmp::Reverse(it.modified_at));
    crate::log!(
        "[Declutter::Downloads] 下载项扫描完成: 筛选出 {} 个可清理项目",
        items.len()
    );
    items
}

fn classify_download_extension(path: &Path) -> (&'static str, &'static str) {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "dmg" | "pkg" | "iso" | "app" => ("安装包", "Installer"),
        "exe" | "msi" | "msix" | "bat" => ("安装包", "Installer"),
        "zip" | "tar" | "gz" | "tgz" | "7z" | "rar" | "xz" | "bz2" => ("压缩包", "Archive"),
        "pdf" | "docx" | "doc" | "xlsx" | "xls" | "pptx" | "ppt" | "txt" | "md" | "csv" => {
            ("文档", "Document")
        }
        "mp4" | "mov" | "mkv" | "avi" | "flv" | "wmv" | "webm" => ("视频", "Video"),
        "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg" => ("音频", "Audio"),
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" | "heic" | "bmp" => ("图片", "Image"),
        _ => ("其它文件", "Other"),
    }
}

fn format_relative_age(mtime: u64, now_secs: u64) -> String {
    if mtime == 0 || now_secs <= mtime {
        return "刚刚".to_string();
    }
    let diff = now_secs.saturating_sub(mtime);
    let days = diff / 86400;

    if days == 0 {
        "今天".to_string()
    } else if days == 1 {
        "昨天".to_string()
    } else if days < 30 {
        format!("{} 天前", days)
    } else if days < 365 {
        format!("{} 个月前", days / 30)
    } else {
        format!("{} 年前", days / 365)
    }
}
