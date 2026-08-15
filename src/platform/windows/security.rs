//! Windows 权限与安全令牌管理

use std::ptr;

use winapi::shared::minwindef::{BOOL, DWORD};
use winapi::um::handleapi::CloseHandle;
use winapi::um::processthreadsapi::{GetCurrentProcess, OpenProcessToken};
use winapi::um::securitybaseapi::GetTokenInformation;
use winapi::um::winbase::LocalFree;
use winapi::um::winnt::{
    TokenElevation, TokenUser, HANDLE, LPWSTR, PSID, TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER,
};

#[link(name = "advapi32")]
extern "system" {
    fn ConvertSidToStringSidW(Sid: PSID, StringSid: *mut LPWSTR) -> BOOL;
}

/// 检查当前进程是否拥有管理员提权身份
pub fn is_elevated() -> bool {
    unsafe {
        let mut token: HANDLE = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut size: DWORD = std::mem::size_of::<TOKEN_ELEVATION>() as DWORD;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut _,
            size,
            &mut size,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

/// 解析当前登录用户的 Windows SID 字符串（例如 S-1-5-21-...）
pub fn current_user_sid() -> Option<String> {
    unsafe {
        let mut token: HANDLE = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }

        let mut size: DWORD = 0;
        let _ = GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut size);
        if size == 0 {
            CloseHandle(token);
            return None;
        }

        let mut buf: Vec<u8> = vec![0u8; size as usize];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            buf.as_mut_ptr() as *mut _,
            size,
            &mut size,
        );
        CloseHandle(token);
        if ok == 0 {
            return None;
        }

        let tu = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut wide: LPWSTR = ptr::null_mut();
        if ConvertSidToStringSidW(tu.User.Sid, &mut wide) == 0 || wide.is_null() {
            return None;
        }
        let mut len = 0usize;
        while *wide.add(len) != 0 {
            len += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(wide, len));
        LocalFree(wide as *mut _);
        Some(sid)
    }
}
