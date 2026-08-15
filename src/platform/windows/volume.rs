//! 卷枚举与容量查询
//!
//! 这些函数以前挤在 `mft.rs` 里，但它们和 `$MFT` 解析没有关系——只是恰好
//! 都要调 Win32 卷 API。拆出来后 `mft.rs` 只负责 MFT 本身。

use std::os::windows::ffi::OsStrExt;

/// 枚举本机所有 NTFS 固定磁盘的盘符
pub fn list_ntfs_volumes() -> Vec<char> {
    use winapi::um::fileapi::{GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
    use winapi::um::winbase::DRIVE_FIXED;

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

        if unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_FIXED {
            continue;
        }

        let mut fs = [0u16; 32];
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
            out.push(letter);
        }
    }
    out
}

/// 查询指定盘符的总容量与可用容量（字节）
pub fn get_volume_space(vol: char) -> Option<(u64, u64)> {
    use std::ffi::OsStr;
    use winapi::um::fileapi::GetDiskFreeSpaceExW;
    use winapi::um::winnt::ULARGE_INTEGER;

    let path = format!("{}:\\\0", vol);
    let wide: Vec<u16> = OsStr::new(&path).encode_wide().collect();
    let mut free_avail: ULARGE_INTEGER = unsafe { std::mem::zeroed() };
    let mut total: ULARGE_INTEGER = unsafe { std::mem::zeroed() };
    let mut total_free: ULARGE_INTEGER = unsafe { std::mem::zeroed() };
    let ret = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_avail,
            &mut total,
            &mut total_free,
        )
    };
    if ret != 0 {
        Some((unsafe { *total.QuadPart() }, unsafe { *total_free.QuadPart() }))
    } else {
        None
    }
}
