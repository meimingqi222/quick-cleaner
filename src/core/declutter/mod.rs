//! 冗余整理核心引擎模块 (Declutter Domain Core)
//!
//! 提供下载项、大文件与旧文件、重复文件（多阶段哈希）、相似照片（dHash感知哈希）
//! 的扫描能力。
//!
//! 清理执行**不在这里**：整理页选中的路径交给 `core::cleaner::clean_arbitrary`
//! 的 `Disposal::RecycleBin` 处置。这里曾经有一份平行的 `cleaner.rs`，结果是
//! trash 原语、保护检查和计数口径各演化一套，反倒比复用多出几个 bug。

pub mod downloads;
pub mod duplicates;
pub mod large_files;
pub mod photos;

pub use downloads::{scan_downloads_folder, DownloadItem};
pub use duplicates::{scan_duplicate_files, DuplicateFileItem, DuplicateGroup};
pub use large_files::{scan_large_old_files, LargeFileItem};
pub use photos::{scan_similar_photos, PhotoGroup, PhotoItem};

use crate::core::i18n::Text;
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

/// 按「距今天数」生成双语的相对时间文案（今天/昨天/N 天前/N 个月前/N 年前）。
///
/// 大文件页（按天数）与下载页（按修改时间戳）原先各自复制了一份同样的分支
/// 逻辑，且只有中文，英文界面会直接露出中文——现在统一到这一处，时间戳版本
/// （见 `downloads.rs` 的 `format_relative_age_text`）换算成天数后委托过来。
pub(crate) fn format_age_text(days: u64) -> Text {
    if days == 0 {
        Text::new("今天", "Today")
    } else if days == 1 {
        Text::new("昨天", "Yesterday")
    } else if days < 30 {
        Text::new(format!("{days} 天前"), format!("{days} days ago"))
    } else if days < 365 {
        let months = days / 30;
        let suffix = if months == 1 { "" } else { "s" };
        Text::new(
            format!("{months} 个月前"),
            format!("{months} month{suffix} ago"),
        )
    } else {
        let years = days / 365;
        let suffix = if years == 1 { "" } else { "s" };
        Text::new(format!("{years} 年前"), format!("{years} year{suffix} ago"))
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
        use crate::core::i18n::Language;

        // 中文侧：覆盖原有的几个典型天数
        assert_eq!(format_age_text(0).get(Language::Zh), "今天");
        assert_eq!(format_age_text(1).get(Language::Zh), "昨天");
        assert_eq!(format_age_text(10).get(Language::Zh), "10 天前");
        assert_eq!(format_age_text(60).get(Language::Zh), "2 个月前");
        assert_eq!(format_age_text(400).get(Language::Zh), "1 年前");

        // 英文侧 + 单复数边界：0/1/2/29/30/364/365 天
        assert_eq!(format_age_text(0).get(Language::En), "Today");
        assert_eq!(format_age_text(1).get(Language::En), "Yesterday");
        assert_eq!(format_age_text(2).get(Language::En), "2 days ago");
        assert_eq!(format_age_text(29).get(Language::En), "29 days ago");
        assert_eq!(format_age_text(30).get(Language::En), "1 month ago");
        assert_eq!(format_age_text(364).get(Language::En), "12 months ago");
        assert_eq!(format_age_text(365).get(Language::En), "1 year ago");

        // 中文侧同样的边界也过一遍单复数无关的分支切换点
        assert_eq!(format_age_text(2).get(Language::Zh), "2 天前");
        assert_eq!(format_age_text(29).get(Language::Zh), "29 天前");
        assert_eq!(format_age_text(30).get(Language::Zh), "1 个月前");
        assert_eq!(format_age_text(364).get(Language::Zh), "12 个月前");
        assert_eq!(format_age_text(365).get(Language::Zh), "1 年前");
    }
}
