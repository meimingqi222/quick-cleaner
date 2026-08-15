//! macOS 废纸篓安全清空

use crate::core::cleaner::{clean_dir_contents, CleanProgress, CleanReport};
use std::path::PathBuf;

pub fn empty_trash(p: &CleanProgress) -> CleanReport {
    if let Some(home) = dirs::home_dir() {
        let trash = home.join(".Trash");
        if trash.exists() {
            return clean_dir_contents(&trash, p);
        }
    }
    CleanReport::default()
}
