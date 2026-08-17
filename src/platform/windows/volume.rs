//! 卷枚举与容量查询
//!
//! 这些函数以前挤在 `mft.rs` 里，但它们和 `$MFT` 解析没有关系——只是恰好
//! 都要调 Win32 卷 API。拆出来后 `mft.rs` 只负责 MFT 本身。

use crate::core::disk::VolumeId;
use std::os::windows::ffi::OsStrExt;

/// 枚举本机所有 NTFS 固定磁盘的盘符
pub fn list_volumes() -> Vec<VolumeId> {
    use winapi::um::fileapi::{GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
    use winapi::um::winbase::DRIVE_FIXED;

    // SAFETY: 不接收参数，返回一个位掩码。
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root: Vec<u16> = format!("{letter}:\\")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        // SAFETY: root 是本地的宽字符串且以 NUL 结尾，只被读取。
        if unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_FIXED {
            continue;
        }

        let mut fs = [0u16; 32];
        // SAFETY: root 以 NUL 结尾；两个输出缓冲都是本地数组，长度如实
        // 上报，API 不会越界写。
        let ok = unsafe {
            GetVolumeInformationW(
                root.as_ptr(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                fs.as_mut_ptr(),
                fs.len() as u32,
            )
        };
        if ok == 0 {
            continue;
        }
        let name = String::from_utf16_lossy(&fs);
        if name.trim_end_matches('\0').eq_ignore_ascii_case("NTFS") {
            out.push(VolumeId::from_drive_letter(letter));
        }
    }
    out
}

/// 查询指定盘符的总容量与可用容量（字节）
pub fn get_volume_space(vol: &VolumeId) -> Option<(u64, u64)> {
    use std::ffi::OsStr;
    use winapi::um::fileapi::GetDiskFreeSpaceExW;
    use winapi::um::winnt::ULARGE_INTEGER;

    let letter = vol.drive_letter()?;
    let path = format!("{}:\\\0", letter);
    let wide: Vec<u16> = OsStr::new(&path).encode_wide().collect();
    // SAFETY: ULARGE_INTEGER 是 POD union，全零是合法位模式。
    let mut free_avail: ULARGE_INTEGER = unsafe { std::mem::zeroed() };
    let mut total: ULARGE_INTEGER = unsafe { std::mem::zeroed() };
    let mut total_free: ULARGE_INTEGER = unsafe { std::mem::zeroed() };
    // SAFETY: wide 以 NUL 结尾；三个出参都是上面刚初始化的本地变量的地址。
    let ret =
        unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free_avail, &mut total, &mut total_free) };
    if ret != 0 {
        // SAFETY: 调用成功才走到这里，说明这两个 union 已被 API 按
        // QuadPart 那一路填好。
        Some((unsafe { *total.QuadPart() }, unsafe {
            *total_free.QuadPart()
        }))
    } else {
        None
    }
}
