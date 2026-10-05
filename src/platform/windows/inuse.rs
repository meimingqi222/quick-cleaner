use crate::core::inuse::{Busy, SpotCheck};
use std::collections::HashMap;
use std::path::PathBuf;

/// Raw sharing evidence for the SQLite stale-file exception. Never call safety predicates here.
pub fn is_open(path: &std::path::Path) -> Option<bool> {
    let md = std::fs::symlink_metadata(path).ok()?;
    if md.file_type().is_symlink() {
        return None;
    }
    if !md.is_dir() {
        return file_is_open(path);
    }
    // Bound the synchronous raw probe; unreadable or oversized trees cannot prove vacancy.
    for (count, entry) in walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .enumerate()
    {
        if count >= 4096 {
            return None;
        }
        let entry = entry.ok()?;
        if entry.file_type().is_file() {
            if file_is_open(entry.path())? {
                return Some(true);
            }
        } else if entry.file_type().is_symlink() {
            return None;
        }
    }
    Some(false)
}

fn file_is_open(path: &std::path::Path) -> Option<bool> {
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::fileapi::{CreateFileW, OPEN_EXISTING};
    use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
    use winapi::um::winnt::{FILE_ATTRIBUTE_NORMAL, GENERIC_READ};
    let path: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // Share mode zero conflicts with any existing read/write handle, including shared SQLite handles.
    // This is evidence at one instant, not a lock held across deletion; the cleaner rechecks failures.
    unsafe {
        let handle = CreateFileW(
            path.as_ptr(),
            GENERIC_READ,
            0,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        );
        if handle != INVALID_HANDLE_VALUE {
            CloseHandle(handle);
            return Some(false);
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(32 | 33) => Some(true),
            _ => None,
        }
    }
}

/// Windows 暂不实现句柄检测；不能把“未实现”伪装成检测失败。
pub fn detect_inuse(_paths: &[PathBuf]) -> HashMap<PathBuf, Busy> {
    HashMap::new()
}

/// 删除前只保留跨平台活 SQLite 文件闸门，不假装实现 Windows 句柄检测。
pub fn spot_check_inuse(paths: &[PathBuf]) -> HashMap<PathBuf, SpotCheck> {
    crate::platform::spot_check_without_handle_probe(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_sqlite_raw_probe_is_nonrecursive_and_detects_shared_handles() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = crate::core::testing::fixture("qc_raw_sqlite_probe");
        let db = root.join("Cache.db");
        let wal = root.join("Cache.db-wal");
        std::fs::write(&db, b"db").unwrap();
        std::fs::write(&wal, b"wal").unwrap();
        for path in [&db, &wal, &root] {
            crate::core::testing::backdate(path, 7200);
        }
        assert_eq!(crate::core::inuse::is_open(&db), Some(false));
        assert!(!crate::core::safety::is_live_database(&db));
        let handle = std::fs::File::options()
            .read(true)
            .share_mode(
                winapi::um::winnt::FILE_SHARE_READ
                    | winapi::um::winnt::FILE_SHARE_WRITE
                    | winapi::um::winnt::FILE_SHARE_DELETE,
            )
            .open(&db)
            .unwrap();
        assert_eq!(crate::core::inuse::is_open(&db), Some(true));
        assert_eq!(crate::core::inuse::is_open(&root), Some(true));
        assert!(crate::core::safety::is_live_database(&db));
        drop(handle);
        assert!(!crate::core::safety::is_live_database(&db));
        assert_eq!(is_open(&root.join("missing.db")), None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
