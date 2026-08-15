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
    use winapi::shared::winerror::ERROR_MORE_DATA;

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
        } else if res as u32 == ERROR_MORE_DATA {
            // 超长字符串（如超长 UninstallString 或复杂命令行）：按系统返回的实际字节数动态扩容
            let words = (buf_size as usize).div_ceil(2);
            let mut dyn_buf = vec![0u16; words];
            let res2 = RegQueryValueExW(
                h_key,
                wide_val.as_ptr(),
                std::ptr::null_mut(),
                &mut val_type,
                dyn_buf.as_mut_ptr() as *mut _,
                &mut buf_size,
            );
            if res2 as u32 == ERROR_SUCCESS {
                Some(from_wide(&dyn_buf[..(buf_size as usize / 2)]))
            } else {
                None
            }
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

/// 枚举某个键下的所有子键名。
pub fn enum_subkeys(root: HKEY, subpath: &str, sam: DWORD) -> Vec<String> {
    use winapi::shared::minwindef::MAX_PATH;
    use winapi::shared::winerror::ERROR_NO_MORE_ITEMS;
    use winapi::um::winnt::KEY_ENUMERATE_SUB_KEYS;
    use winapi::um::winreg::{RegCloseKey, RegEnumKeyExW, RegOpenKeyExW};

    let mut out = Vec::new();
    let wide = to_wide(subpath);
    let mut h: HKEY = std::ptr::null_mut();

    unsafe {
        if RegOpenKeyExW(root, wide.as_ptr(), 0, sam | KEY_ENUMERATE_SUB_KEYS, &mut h) as u32
            != ERROR_SUCCESS
        {
            return out;
        }
        let mut idx: DWORD = 0;
        let mut buf = [0u16; MAX_PATH];
        loop {
            let mut len = buf.len() as DWORD;
            let res = RegEnumKeyExW(
                h,
                idx,
                buf.as_mut_ptr(),
                &mut len,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if res as u32 == ERROR_NO_MORE_ITEMS {
                break;
            }
            if res as u32 == ERROR_SUCCESS {
                out.push(from_wide(&buf[..len as usize]));
            } else {
                break;
            }
            idx += 1;
        }
        RegCloseKey(h);
    }
    out
}

/// 枚举某个键下的所有 (值名, 字符串形式的值)。
///
/// 非字符串类型的值会被跳过——残留判定全部基于路径文本匹配，
/// 二进制值没有参考价值。
pub fn enum_string_values(root: HKEY, subpath: &str, sam: DWORD) -> Vec<(String, String)> {
    use winapi::shared::winerror::{ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS};
    use winapi::um::winnt::{KEY_QUERY_VALUE, REG_EXPAND_SZ, REG_SZ};
    use winapi::um::winreg::{RegCloseKey, RegEnumValueW, RegOpenKeyExW};

    let mut out = Vec::new();
    let wide = to_wide(subpath);
    let mut h: HKEY = std::ptr::null_mut();

    unsafe {
        if RegOpenKeyExW(root, wide.as_ptr(), 0, sam | KEY_QUERY_VALUE, &mut h) as u32
            != ERROR_SUCCESS
        {
            return out;
        }
        let mut idx: DWORD = 0;
        // 防火墙规则的值可以很长，给足缓冲
        let mut name_buf = [0u16; 512];
        let mut data_buf = [0u16; 4096];
        loop {
            let mut name_len = name_buf.len() as DWORD;
            let mut data_len = (data_buf.len() * 2) as DWORD;
            let mut val_type: DWORD = 0;
            let res = RegEnumValueW(
                h,
                idx,
                name_buf.as_mut_ptr(),
                &mut name_len,
                std::ptr::null_mut(),
                &mut val_type,
                data_buf.as_mut_ptr() as *mut u8,
                &mut data_len,
            );
            if res as u32 == ERROR_NO_MORE_ITEMS {
                break;
            }
            if res as u32 == ERROR_SUCCESS {
                if val_type == REG_SZ || val_type == REG_EXPAND_SZ {
                    let name = from_wide(&name_buf[..name_len as usize]);
                    let data = from_wide(&data_buf[..(data_len as usize / 2).min(data_buf.len())]);
                    out.push((name, data));
                }
            } else if res as u32 == ERROR_MORE_DATA {
                // 超长注册表值（如 Windows 防火墙长规则）：动态扩容当前条目缓冲区重试，
                // 即使单项失败也绝不中断 loop，确保后续条目继续被枚举。
                let dyn_words = (data_len as usize).div_ceil(2).max(8192);
                let mut dyn_data_buf = vec![0u16; dyn_words];
                let mut dyn_data_len = (dyn_data_buf.len() * 2) as DWORD;
                name_len = name_buf.len() as DWORD;
                let retry_res = RegEnumValueW(
                    h,
                    idx,
                    name_buf.as_mut_ptr(),
                    &mut name_len,
                    std::ptr::null_mut(),
                    &mut val_type,
                    dyn_data_buf.as_mut_ptr() as *mut u8,
                    &mut dyn_data_len,
                );
                if retry_res as u32 == ERROR_SUCCESS && (val_type == REG_SZ || val_type == REG_EXPAND_SZ) {
                    let name = from_wide(&name_buf[..name_len as usize]);
                    let data = from_wide(&dyn_data_buf[..(dyn_data_len as usize / 2).min(dyn_data_buf.len())]);
                    out.push((name, data));
                }
            }
            idx += 1;
        }
        RegCloseKey(h);
    }
    out
}

/// 删除某个键下的单个值（而非整个键）。
pub fn delete_reg_value(root: HKEY, subpath: &str, value_name: &str, sam: DWORD) -> bool {
    use winapi::um::winnt::KEY_SET_VALUE;
    use winapi::um::winreg::{RegCloseKey, RegDeleteValueW, RegOpenKeyExW};

    let wide = to_wide(subpath);
    let wide_val = to_wide(value_name);
    let mut h: HKEY = std::ptr::null_mut();
    unsafe {
        if RegOpenKeyExW(root, wide.as_ptr(), 0, sam | KEY_SET_VALUE, &mut h) as u32
            != ERROR_SUCCESS
        {
            return false;
        }
        let ok = RegDeleteValueW(h, wide_val.as_ptr()) as u32 == ERROR_SUCCESS;
        RegCloseKey(h);
        ok
    }
}
