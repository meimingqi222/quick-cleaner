//! 冗余整理清理执行器 (Cleaner Executor)

use crate::core::safety::is_protected;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct DeclutterCleanReport {
    pub deleted_files: u64,
    pub freed_bytes: u64,
    pub failed_files: u64,
}

/// 执行整理清理：默认安全移至废纸篓/回收站
pub fn clean_declutter_items(paths: &[PathBuf], use_trash: bool) -> DeclutterCleanReport {
    let mut report = DeclutterCleanReport::default();

    for path in paths {
        if is_protected(path) || !path.exists() {
            report.failed_files += 1;
            continue;
        }

        let size = path.metadata().map(|m| m.len()).unwrap_or(0);

        let success = if use_trash {
            move_file_to_trash(path)
        } else {
            if path.is_dir() {
                std::fs::remove_dir_all(path).is_ok()
            } else {
                std::fs::remove_file(path).is_ok()
            }
        };

        if success {
            report.deleted_files += 1;
            report.freed_bytes += size;
        } else {
            report.failed_files += 1;
        }
    }

    report
}

fn move_file_to_trash(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::platform::macos::trash::move_to_trash(path).is_ok()
    }
    #[cfg(windows)]
    {
        crate::platform::windows::recycle::move_to_recycle_bin(path).is_ok()
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        if path.is_dir() {
            std::fs::remove_dir_all(path).is_ok()
        } else {
            std::fs::remove_file(path).is_ok()
        }
    }
}
