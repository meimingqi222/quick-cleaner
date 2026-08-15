//! Windows 注册表底层安全访问与工具封装

use std::os::windows::ffi::OsStrExt;

use winapi::shared::minwindef::{DWORD, HKEY};
use winapi::shared::winerror::ERROR_SUCCESS;
use winapi::um::winreg::{RegDeleteTreeW, RegQueryValueExW};

pub fn to_wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

pub fn from_wide(w: &[u16]) -> String {
    let len = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..len])
}

pub fn read_reg_string(h_key: HKEY, value_name: &str) -> Option<String> {
    let wide_val = to_wide(value_name);
    let mut buf = [0u16; 1024];
    let mut buf_size = (buf.len() * 2) as DWORD;
    let mut val_type: DWORD = 0;

    unsafe {
        let res = RegQueryValueExW(
            h_key,
            wide_val.as_ptr(),
            std::ptr::null_mut(),
            &mut val_type,
            buf.as_mut_ptr() as *mut _,
            &mut buf_size,
        );

        if res as u32 == ERROR_SUCCESS {
            Some(from_wide(&buf[..(buf_size as usize / 2)]))
        } else {
            None
        }
    }
}

pub fn read_reg_dword(h_key: HKEY, value_name: &str) -> Option<u32> {
    let wide_val = to_wide(value_name);
    let mut val: DWORD = 0;
    let mut val_size = std::mem::size_of::<DWORD>() as DWORD;
    let mut val_type: DWORD = 0;

    unsafe {
        let res = RegQueryValueExW(
            h_key,
            wide_val.as_ptr(),
            std::ptr::null_mut(),
            &mut val_type,
            &mut val as *mut _ as *mut _,
            &mut val_size,
        );

        if res as u32 == ERROR_SUCCESS {
            Some(val)
        } else {
            None
        }
    }
}

/// 递归删除指定的注册表子树
pub fn delete_reg_tree(root: HKEY, subpath: &str) -> bool {
    let wide_path = to_wide(subpath);
    unsafe {
        let res = RegDeleteTreeW(root, wide_path.as_ptr());
        res as u32 == ERROR_SUCCESS
    }
}
