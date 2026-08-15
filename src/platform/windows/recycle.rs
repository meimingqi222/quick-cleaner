//! Windows 回收站管理与孤儿残留清理

use crate::core::cleaner::{delete_tree, CleanProgress, CleanReport};
use crate::platform::windows::security::current_user_sid;
use std::path::{Path, PathBuf};
use std::ptr;

use winapi::um::shellapi::SHEmptyRecycleBinW;

/// 回收站 SID 目录里的某个条目是否该被扫尾清掉。
///
/// 该目录下除了 Shell 用来标记文件夹外观的 `desktop.ini`，其余内容
/// （`$I`/`$R` 索引与数据对、MSYS2/Cygwin 之类工具留下的点开头临时项）
/// 全都是已删除文件的残骸，一律清理。
pub fn is_recycle_junk_entry(name: &str) -> bool {
    !name.eq_ignore_ascii_case("desktop.ini")
}

/// 扫尾清掉当前用户 SID 目录下 Shell 没能删干净的残骸。
pub fn sweep_orphaned_recycle(p: &CleanProgress) -> CleanReport {
    let mut report = CleanReport::default();
    let Some(sid) = current_user_sid() else {
        return report;
    };

    for letter in 'A'..='Z' {
        let dir = PathBuf::from(format!("{letter}:\\$Recycle.Bin")).join(&sid);
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            if p.cancelled() {
                return report;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !is_recycle_junk_entry(&name) {
                continue;
            }
            let path = entry.path();
            p.note(&path);
            report.record(&path, delete_tree(&path, p));
        }
    }
    report
}

/// 清空回收站。
///
/// 分两步：先让 Shell 清掉它索引里可见的条目，再扫尾删掉孤儿数据。
pub fn empty_recycle_bin(p: &CleanProgress) -> CleanReport {
    let mut report = CleanReport::default();
    // SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND
    let flags = 0x0000_0001 | 0x0000_0002 | 0x0000_0004;
    let hr = unsafe { SHEmptyRecycleBinW(ptr::null_mut(), ptr::null(), flags) };
    // S_OK = 0；回收站本来就是空的时候返回 E_UNEXPECTED(0x8000FFFF)，同样视为成功
    if hr == 0 {
        report.ok += 1;
    } else if hr as u32 == 0x8000_FFFF {
        report.skipped += 1;
    } else {
        report.failed.push(PathBuf::from("回收站"));
    }

    report.merge(sweep_orphaned_recycle(p));
    report
}

/// 判断一个扫描目标是否是回收站（需要走 SHEmptyRecycleBin 特殊路径）。
pub fn is_recycle_bin(path: &Path) -> bool {
    path.to_string_lossy()
        .to_ascii_lowercase()
        .contains("$recycle.bin")
}
