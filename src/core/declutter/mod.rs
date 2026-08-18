//! 冗余整理核心引擎模块 (Declutter Domain Core)
//!
//! 提供下载项、大文件与旧文件、重复文件（多阶段哈希）、相似照片（dHash感知哈希）
//! 扫描与安全清理执行能力。

pub mod cleaner;
pub mod downloads;
pub mod duplicates;
pub mod large_files;
pub mod photos;

pub use cleaner::{clean_declutter_items, DeclutterCleanReport};
pub use downloads::{scan_downloads_folder, DownloadItem};
pub use duplicates::{scan_duplicate_files, DuplicateFileItem, DuplicateGroup};
pub use large_files::{scan_large_old_files, LargeFileItem};
pub use photos::{scan_similar_photos, PhotoGroup, PhotoItem};

use std::path::PathBuf;

/// 获取用户主目录下的搜索根目录（覆盖整个用户主目录）
pub(crate) fn get_user_content_roots() -> Vec<PathBuf> {
    dirs::home_dir().map(|h| vec![h]).unwrap_or_default()
}

pub(crate) fn is_photo_extension(ext: &str) -> bool {
    matches!(
        ext,
        "jpg"
            | "jpeg"
            | "png"
            | "heic"
            | "webp"
            | "tiff"
            | "bmp"
            | "raw"
            | "cr2"
            | "nef"
            | "arw"
            | "gif"
    )
}

pub(crate) fn format_timestamp_date(secs: u64) -> String {
    if secs == 0 {
        return "Unknown".to_string();
    }
    let dt = chrono::DateTime::from_timestamp(secs as i64, 0);
    if let Some(d) = dt {
        d.format("%Y-%m-%d").to_string()
    } else {
        "Unknown".to_string()
    }
}

pub(crate) fn format_age_str(days: u64) -> String {
    if days == 0 {
        "今天".to_string()
    } else if days == 1 {
        "昨天".to_string()
    } else if days < 30 {
        format!("{days} 天前")
    } else if days < 365 {
        format!("{} 个月前", days / 30)
    } else {
        format!("{} 年前", days / 365)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_photo_extension() {
        assert!(is_photo_extension("jpg"));
        assert!(is_photo_extension("png"));
        assert!(is_photo_extension("heic"));
        assert!(!is_photo_extension("txt"));
        assert!(!is_photo_extension("dmg"));
    }

    #[test]
    fn test_format_age_str() {
        assert_eq!(format_age_str(0), "今天");
        assert_eq!(format_age_str(1), "昨天");
        assert_eq!(format_age_str(10), "10 天前");
        assert_eq!(format_age_str(60), "2 个月前");
        assert_eq!(format_age_str(400), "1 年前");
    }
}
