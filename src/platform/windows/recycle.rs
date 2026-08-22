//! Windows 回收站管理与孤儿残留清理

use crate::core::cleaner::{delete_tree, CleanFailure, CleanProgress, CleanReport};
use crate::platform::windows::security::current_user_sid;
use crate::platform::windows::user_env::real_user_sid;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;

use winapi::um::shellapi::{
    SHEmptyRecycleBinW, SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI,
    FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW,
};

/// 把一个路径送进回收站，而不是直接抹掉。
///
/// 用 `SHFileOperationW` + `FOF_ALLOWUNDO`：这是唯一能生成 `$I`/`$R` 索引对、
/// 让文件在资源管理器里「还原」得回去的途径。直接把文件移到 `$Recycle.Bin`
/// 目录下是不行的，Shell 认不出来。
///
/// # 注意
///
/// 回收站**不释放磁盘空间**——文件还占着原来的簇，只是换了个位置登记。
/// 所以这条路径默认是关的，见 `Settings::delete_to_recycle_bin`。
///
/// # 返回
///
/// 成功返回 `true`。以下情况会失败，调用方应当回退到永久删除或如实报错：
///
/// - 目标卷没开回收站（可移动介质、网络盘）；
/// - 单个文件比回收站配额还大——此时 Shell 会**直接永久删除**，我们用
///   `FOF_WANTNUKEWARNING` 的反面（不加它）配合静默标志，让它安静地做掉；
/// - 路径超过 `MAX_PATH`：这个 API 是老式 ANSI 时代的产物，长路径不支持。
pub fn move_to_trash(path: &Path) -> Result<(), String> {
    // pFrom 要求「双 NUL 结尾」的字符串列表：每项一个 NUL，整体再加一个。
    let mut from: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    from.push(0);

    let mut op = SHFILEOPSTRUCTW {
        hwnd: ptr::null_mut(),
        wFunc: FO_DELETE as u32,
        pFrom: from.as_ptr(),
        pTo: ptr::null(),
        fFlags: FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT,
        fAnyOperationsAborted: 0,
        hNameMappings: ptr::null_mut(),
        lpszProgressTitle: ptr::null(),
    };

    // SAFETY: from 是本地 Vec，双 NUL 结尾，活到调用结束；op 里其余指针
    // 要么为 null（文档允许），要么指向 from。SHFileOperationW 只读它们。
    let rc = unsafe { SHFileOperationW(&mut op) };

    if rc != 0 {
        return Err(format!("SHFileOperationW 返回 0x{rc:08X}"));
    }
    if op.fAnyOperationsAborted != 0 {
        return Err("回收站操作被中止".into());
    }
    Ok(())
}

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
        // SAFETY: 两个 null 分别表示「无父窗口」和「所有驱动器」，都是
        // SHEmptyRecycleBinW 文档明确允许的取值。
        let hr = unsafe { SHEmptyRecycleBinW(ptr::null_mut(), ptr::null(), flags) };
        // S_OK = 0；回收站本来就是空的时候返回 E_UNEXPECTED(0x8000FFFF)，同样视为成功
        if hr == 0 {
            report.ok += 1;
        } else if hr as u32 == 0x8000_FFFF {
            report.skipped += 1;
        } else {
            // 回收站是 shell 命名空间里的对象，不是文件路径
            report.failed.push(CleanFailure::Id("回收站".into()));
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
