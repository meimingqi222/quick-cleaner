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

    // SAFETY: h_key 由调用方保证是打开着的有效句柄。buf 是本地数组，
    // buf_size 如实报告它的字节长度，RegQueryValueExW 不会越界写。
    // 出参 val_type / buf_size 都是本地变量的地址。
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

    // SAFETY: 同 read_reg_string——句柄由调用方保证有效，写入目标是
    // 本地的单个 DWORD，val_size 如实报告它的大小。
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

/// 递归删除指定的注册表子树。
///
/// `sam` 携带 WOW64 视图标志：32 位安装的登记项扫的是 32 位视图
/// （`KEY_WOW64_32KEY`），删除也必须落在同一个视图上，否则 64 位进程
/// 会去删 64 位视图里的同名路径——删错对象或永远报「删除失败」。实现
/// 上先按声明视图打开父键，再对叶子名调 `RegDeleteTreeW`。
///
/// 返回 `true` 只代表删除调用成功；完成核验用 [`reg_key_absent`]。
pub fn delete_reg_tree(root: HKEY, subpath: &str, sam: DWORD) -> bool {
    use winapi::um::winnt::{DELETE, KEY_ENUMERATE_SUB_KEYS, KEY_QUERY_VALUE};
    use winapi::um::winreg::{RegCloseKey, RegOpenKeyExW};

    let (parent, leaf) = match subpath.rsplit_once('\\') {
        Some((parent, leaf)) => (parent, leaf),
        None => ("", subpath),
    };
    let wide_parent = to_wide(parent);
    let mut h: HKEY = std::ptr::null_mut();
    // SAFETY: wide_parent 以 NUL 结尾且活到调用结束；空父路径返回根键
    // 本身的句柄。句柄在出口无条件关闭。
    unsafe {
        if RegOpenKeyExW(
            root,
            wide_parent.as_ptr(),
            0,
            sam | DELETE | KEY_ENUMERATE_SUB_KEYS | KEY_QUERY_VALUE,
            &mut h,
        ) as u32
            != ERROR_SUCCESS
        {
            return false;
        }
        // 叶子为空表示删的就是打开的这个键本身；RegDeleteTreeW 只把
        // NULL 指针当「删 hKey 自己」，空串不是。
        let wide_leaf = (!leaf.is_empty()).then(|| to_wide(leaf));
        let leaf_ptr = wide_leaf
            .as_ref()
            .map(|w| w.as_ptr())
            .unwrap_or(std::ptr::null());
        let ok = RegDeleteTreeW(h, leaf_ptr) as u32 == ERROR_SUCCESS;
        RegCloseKey(h);
        ok
    }
}

/// 三态核验：键是否真的不在了。
///
/// `Some(true)` 只认 `ERROR_FILE_NOT_FOUND` / `ERROR_PATH_NOT_FOUND`；
/// 打开成功说明还在（`Some(false)`）；其他失败（权限不足等）原因不明，
/// 是 Unknown（`None`），不能当作删除成功。
pub fn reg_key_absent(root: HKEY, subpath: &str, sam: DWORD) -> Option<bool> {
    use winapi::shared::winerror::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
    use winapi::um::winnt::KEY_QUERY_VALUE;
    use winapi::um::winreg::{RegCloseKey, RegOpenKeyExW};

    let wide = to_wide(subpath);
    let mut h: HKEY = std::ptr::null_mut();
    // SAFETY: wide 以 NUL 结尾且活到调用结束；句柄只在打开成功时使用，
    // 出口无条件关闭。
    unsafe {
        let res = RegOpenKeyExW(root, wide.as_ptr(), 0, sam | KEY_QUERY_VALUE, &mut h) as u32;
        if !h.is_null() {
            RegCloseKey(h);
        }
        match res {
            ERROR_SUCCESS => Some(false),
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Some(true),
            _ => None,
        }
    }
}

/// 三态核验：某个值是否真的不在了。键不在也算值不在（整棵树已删除）；
/// 其余语义与 [`reg_key_absent`] 相同。
pub fn reg_value_absent(root: HKEY, subpath: &str, value_name: &str, sam: DWORD) -> Option<bool> {
    use winapi::shared::winerror::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
    use winapi::um::winnt::KEY_QUERY_VALUE;
    use winapi::um::winreg::{RegCloseKey, RegOpenKeyExW};

    let wide = to_wide(subpath);
    let wide_val = to_wide(value_name);
    let mut h: HKEY = std::ptr::null_mut();
    // SAFETY: 同 reg_key_absent；wide_val 活到 RegQueryValueExW 返回。
    unsafe {
        let open = RegOpenKeyExW(root, wide.as_ptr(), 0, sam | KEY_QUERY_VALUE, &mut h) as u32;
        if open != ERROR_SUCCESS {
            return match open {
                ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Some(true),
                _ => None,
            };
        }
        let mut val_type: DWORD = 0;
        let mut buf_size: DWORD = 0;
        let res = RegQueryValueExW(
            h,
            wide_val.as_ptr(),
            std::ptr::null_mut(),
            &mut val_type,
            std::ptr::null_mut(),
            &mut buf_size,
        ) as u32;
        RegCloseKey(h);
        match res {
            ERROR_SUCCESS => Some(false),
            ERROR_FILE_NOT_FOUND => Some(true),
            _ => None,
        }
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

    // SAFETY: wide 以 NUL 结尾且活到调用结束。h 是本地变量，只在
    // RegOpenKeyExW 返回 ERROR_SUCCESS 时才被当作有效句柄使用，
    // 并在函数出口无条件 RegCloseKey。
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

    // SAFETY: 同 enum_subkeys。name_buf / data_buf 是本地数组，每轮循环都
    // 把长度重置成它们的真实容量后再传进去，RegEnumValueW 不会越界写。
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
                if retry_res as u32 == ERROR_SUCCESS
                    && (val_type == REG_SZ || val_type == REG_EXPAND_SZ)
                {
                    let name = from_wide(&name_buf[..name_len as usize]);
                    let data = from_wide(
                        &dyn_data_buf[..(dyn_data_len as usize / 2).min(dyn_data_buf.len())],
                    );
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
    // SAFETY: 两个 wide 串都以 NUL 结尾且活到调用结束；句柄只在打开成功
    // 后使用，并在返回前关闭。
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

/// 测试夹具：在 HKCU 的 QuickCleanerTest 路径下自建一个带 marker 值的键。
/// 自建自清，绝不触碰用户资源；同 crate 的测试共用。
#[cfg(test)]
pub(crate) fn create_fixture_key(subpath: &str) -> bool {
    use winapi::shared::winerror::ERROR_SUCCESS;
    use winapi::um::winnt::{KEY_CREATE_SUB_KEY, KEY_SET_VALUE, REG_SZ};
    use winapi::um::winreg::{RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY_CURRENT_USER};

    let wide = to_wide(subpath);
    let mut h: HKEY = std::ptr::null_mut();
    // SAFETY: wide 以 NUL 结尾且活到调用结束；句柄只在创建成功后使用，
    // 并在返回前关闭。
    unsafe {
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide.as_ptr(),
            0,
            std::ptr::null_mut(),
            0,
            KEY_SET_VALUE | KEY_CREATE_SUB_KEY,
            std::ptr::null_mut(),
            &mut h,
            std::ptr::null_mut(),
        ) as u32
            != ERROR_SUCCESS
        {
            return false;
        }
        let value = to_wide("marker");
        let data: Vec<u16> = "x".encode_utf16().chain(std::iter::once(0)).collect();
        let set = RegSetValueExW(
            h,
            value.as_ptr(),
            0,
            REG_SZ,
            data.as_ptr() as *const u8,
            (data.len() * 2) as u32,
        );
        RegCloseKey(h);
        set as u32 == ERROR_SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winapi::um::winnt::KEY_READ;
    use winapi::um::winreg::{RegCloseKey, RegOpenKeyExW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    /// 每台 Windows 上都有的键，拿来做真实读取的靶子。
    const CURVER: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

    /// 打开一个只读句柄，测完自己关。
    struct Key(HKEY);

    impl Key {
        fn open(subpath: &str) -> Option<Self> {
            let wide = to_wide(subpath);
            let mut h: HKEY = std::ptr::null_mut();
            // SAFETY: wide 以 NUL 结尾且活到调用结束；h 只在返回 ERROR_SUCCESS
            // 时被当作有效句柄，并由 Drop 关闭。
            let ok = unsafe {
                RegOpenKeyExW(HKEY_LOCAL_MACHINE, wide.as_ptr(), 0, KEY_READ, &mut h) as u32
                    == ERROR_SUCCESS
            };
            ok.then_some(Key(h))
        }
    }

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: 句柄来自成功的 RegOpenKeyExW，Key 不可 Clone，只关一次。
            unsafe { RegCloseKey(self.0) };
        }
    }

    #[test]
    fn wide_round_trips_including_cjk() {
        for s in [
            "",
            "Software",
            r"C:\Program Files (x86)",
            "中文路径",
            "emoji 🚀",
        ] {
            let w = to_wide(s);
            assert_eq!(*w.last().unwrap(), 0, "必须以 NUL 结尾");
            assert_eq!(from_wide(&w), s);
        }
    }

    /// `from_wide` 要在第一个 NUL 处截断，而不是把整个缓冲都吃进去。
    /// 注册表 API 回填的缓冲区尾部全是零，不截断的话字符串会拖一串 \0。
    #[test]
    fn from_wide_stops_at_the_first_nul() {
        let mut buf = to_wide("abc");
        buf.extend_from_slice(&[0u16; 8]);
        assert_eq!(from_wide(&buf), "abc");
        // 完全没有 NUL 时按整段解释
        assert_eq!(from_wide(&[0x41, 0x42]), "AB");
        assert_eq!(from_wide(&[]), "");
    }

    #[test]
    fn reads_a_real_string_value() {
        let Some(k) = Key::open(CURVER) else {
            return; // 非 Windows 或权限受限的环境直接跳过
        };
        let name = read_reg_string(k.0, "ProductName").expect("ProductName 应该读得到");
        assert!(!name.is_empty());
        assert!(!name.contains('\0'), "尾部 NUL 没有被截掉：{name:?}");
    }

    #[test]
    fn reads_a_real_dword_value() {
        let Some(k) = Key::open(CURVER) else { return };
        // CurrentMajorVersionNumber 从 Win10 起存在
        if let Some(v) = read_reg_dword(k.0, "CurrentMajorVersionNumber") {
            assert!(v >= 6, "主版本号看起来不对：{v}");
        }
    }

    /// 值不存在时必须返回 None，不能 panic、也不能返回垃圾。
    #[test]
    fn missing_values_return_none() {
        let Some(k) = Key::open(CURVER) else { return };
        assert!(read_reg_string(k.0, "绝不存在的值名 9f3a").is_none());
        assert!(read_reg_dword(k.0, "绝不存在的值名 9f3a").is_none());
    }

    /// 键不存在时枚举返回空表，同样不能 panic。
    #[test]
    fn enumerating_a_missing_key_is_empty() {
        assert!(enum_subkeys(HKEY_LOCAL_MACHINE, r"SOFTWARE\绝不存在 9f3a", 0).is_empty());
        assert!(enum_string_values(HKEY_LOCAL_MACHINE, r"SOFTWARE\绝不存在 9f3a", 0).is_empty());
    }

    #[test]
    fn enumerates_real_subkeys_and_values() {
        let subkeys = enum_subkeys(HKEY_LOCAL_MACHINE, "SOFTWARE", 0);
        if subkeys.is_empty() {
            return; // 非 Windows 环境
        }
        assert!(
            subkeys.iter().any(|k| k.eq_ignore_ascii_case("Microsoft")),
            "SOFTWARE 下应当有 Microsoft"
        );
        assert!(subkeys.iter().all(|k| !k.contains('\0')));

        let values = enum_string_values(HKEY_LOCAL_MACHINE, CURVER, 0);
        assert!(
            values.iter().any(|(n, _)| n == "ProductName"),
            "CurrentVersion 下应当有 ProductName"
        );
        assert!(values
            .iter()
            .all(|(n, v)| !n.contains('\0') && !v.contains('\0')));
    }

    /// 空子路径表示「就是这个根键本身」，不能因此炸掉。
    #[test]
    fn empty_subpath_targets_the_root_itself() {
        let _ = enum_subkeys(HKEY_LOCAL_MACHINE, "", 0);
        let _ = enum_string_values(HKEY_LOCAL_MACHINE, "", 0);
    }

    /// 删除不存在的键要老实返回 false，而不是报告成功。
    ///
    /// 只删一个必定不存在的路径——绝不能让测试真的动到注册表。
    #[test]
    fn deleting_a_missing_tree_reports_failure() {
        assert!(!delete_reg_tree(
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\QuickCleanerTestKeyThatMustNotExist9f3a",
            0
        ));
        // 三态核验对同一个路径必须给出「确认不在」，而不是 Unknown。
        assert_eq!(
            reg_key_absent(
                HKEY_LOCAL_MACHINE,
                r"SOFTWARE\QuickCleanerTestKeyThatMustNotExist9f3a",
                0
            ),
            Some(true)
        );
    }

    /// 删除 + 三态核验的真实往返：夹具键建在 HKCU 自己的测试路径下
    /// （自建自清，不是用户资源），删掉后必须核验为「确认不在」。
    #[test]
    fn delete_then_verify_reports_verified_absence() {
        let base = format!("Software\\QuickCleanerTest\\{}", std::process::id());
        let with_child = format!("{base}\\child");
        assert!(create_fixture_key(&with_child), "自建夹具键失败");

        // 键还在：核验必须给出「还在」，不能当作已删除。
        assert_eq!(
            reg_key_absent(HKEY_CURRENT_USER, &with_child, 0),
            Some(false)
        );
        // 值还在：同理。
        assert_eq!(
            reg_value_absent(HKEY_CURRENT_USER, &with_child, "marker", 0),
            Some(false)
        );

        // 删值 + 核验：确认消失才算完成。
        assert!(super::delete_reg_value(
            HKEY_CURRENT_USER,
            &with_child,
            "marker",
            0
        ));
        assert_eq!(
            reg_value_absent(HKEY_CURRENT_USER, &with_child, "marker", 0),
            Some(true)
        );

        // 删树 + 核验：确认消失才算完成。
        assert!(delete_reg_tree(HKEY_CURRENT_USER, &base, 0));
        assert_eq!(reg_key_absent(HKEY_CURRENT_USER, &base, 0), Some(true));
    }
}
