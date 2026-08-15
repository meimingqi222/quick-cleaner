//! Windows 回收站管理与孤儿残留清理

use crate::core::cleaner::{delete_tree, CleanProgress, CleanReport};
use crate::platform::windows::security::current_user_sid;
use crate::platform::windows::user_env::real_user_sid;
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

/// 清理真实前台用户 SID 目录下的所有回收站条目（保留 desktop.ini）。
pub fn clean_real_user_recycle_entries(p: &CleanProgress) -> CleanReport {
    let mut report = CleanReport::default();
    let Some(sid) = real_user_sid() else {
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

/// 扫尾清掉真实用户 SID 目录下 Shell 没能删干净的残骸。
pub fn sweep_orphaned_recycle(p: &CleanProgress) -> CleanReport {
    clean_real_user_recycle_entries(p)
}

/// 清空回收站。
///
/// 当当前进程 SID 与真实前台用户 SID 一致时，先调用 Windows Shell API `SHEmptyRecycleBinW`，
/// 再扫尾清理孤儿文件。
/// 当跨账户提权（OTS）导致真实用户 SID 与进程管理员 SID 不一致时，跳过 `SHEmptyRecycleBinW`
/// （避免误清管理员自身的回收站），直接对真实用户的 `$Recycle.Bin\<SID>` 目录执行
/// 深度清理并保留 desktop.ini。
pub fn empty_recycle_bin(p: &CleanProgress) -> CleanReport {
    let mut report = CleanReport::default();
    let real_sid = real_user_sid();
    let proc_sid = current_user_sid();

    let is_same_user = match (&real_sid, &proc_sid) {
        (Some(r), Some(p)) => r == p,
        (None, None) => true,
        _ => false,
    };

    if is_same_user {
        // 同一用户环境：先通知 Windows Shell 清空回收站
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
    }

    // 无论是否同用户，都针对真实前台用户的 SID 目录执行彻底清理与扫尾
    report.merge(clean_real_user_recycle_entries(p));
    report
}

/// 判断一个扫描目标是否是回收站（需要走 SHEmptyRecycleBin 特殊路径）。
pub fn is_recycle_bin(path: &Path) -> bool {
    path.to_string_lossy()
        .to_ascii_lowercase()
        .contains("$recycle.bin")
}
