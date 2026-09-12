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
    // SAFETY: OpenProcessToken 写入本地的 token 变量；后续 GetTokenInformation
    // 只在打开成功后调用，写入目标是本地的 TOKEN_ELEVATION，大小如实上报。
    // token 在所有出口都会 CloseHandle。
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
    // SAFETY: GetTokenInformation 先用空缓冲问长度，再按返回的长度分配，
    // 第二次调用写入的就是这块自己分配的内存。ConvertSidToStringSidW 分配的
    // 字符串由 LocalFree 释放，不会泄漏也不会重复释放。
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

/// 符合 Windows `CommandLineToArgvW` 规范的参数转义
pub fn quote_win_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\x0b', '\"']) {
        return arg.to_string();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        if c == '\\' {
            backslashes += 1;
        } else if c == '"' {
            // 紧随双引号前的反斜杠需要翻倍，外加转义双引号本身的一记反斜杠
            for _ in 0..backslashes * 2 + 1 {
                out.push('\\');
            }
            out.push('"');
            backslashes = 0;
        } else {
            for _ in 0..backslashes {
                out.push('\\');
            }
            out.push(c);
            backslashes = 0;
        }
    }
    // 结尾若有反斜杠，在闭合双引号前也需翻倍
    for _ in 0..backslashes * 2 {
        out.push('\\');
    }
    out.push('"');
    out
}

/// 删除因 ACCESS_DENIED（os error 5）失败时的提权补救：取得所有权、摘掉
/// Deny ACE，再授予 Administrators 完全控制。
///
/// 典型场景：应用（如 WorkBuddy）在自己的日志目录上写 `Deny Delete` ACL
/// 做防删，进程早已退出，但 ACL 还在。进程占用（os error 32）不走这里——
/// 那是句柄锁，改 ACL 也解不开。
///
/// 仅在已提权时有意义；未提权直接返回 false，不空跑子进程。
/// `/t` 递归、`/c` 遇错继续：目标子树里混着系统文件时不要整批中断。
///
/// **为什么不能只 `/grant`**：Windows AccessCheck 把「命中的 Deny」当作
/// 权威，后面的 Allow 不会把它盖回去。只给 Administrators 加 FullControl
/// 的话，用户 SID 上那条 Deny Delete 仍然让删除失败——实机 WorkBuddy
/// 日志就是这种「Deny + Allow 并存」的形态。必须先 `/remove:d` 摘掉
/// Deny，再 `/grant` 兜底。
///
/// 成功只表示「ACL 改过了」，不保证文件一定能删（服务仍可能持有句柄）。
pub fn force_delete_access(path: &std::path::Path) -> bool {
    if !is_elevated() {
        return false;
    }
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    let display = path.display().to_string();
    let quiet = |cmd: &mut Command| {
        cmd.creation_flags(winapi::um::winbase::CREATE_NO_WINDOW)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    };

    // takeown 先做：Deny ACE 的宿主可能不是我们，没有所有权就改不动 DACL。
    // `/a` 落到 Administrators 组而不是当前用户，和后面 icacls 的授权主体一致。
    let takeown = Command::new("takeown")
        .args(["/f", &display, "/a", "/r", "/d", "y"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(winapi::um::winbase::CREATE_NO_WINDOW)
        .status();
    let _ = takeown;

    // 摘 Deny：当前用户 SID + Everyone。用 SID 而不是名字——中文系统上
    // 组名本地化后按名字匹配会静默失败。一个 trustee 一条 /remove:d。
    let sid = current_user_sid();
    let mut remove_deny = Command::new("icacls");
    remove_deny.arg(&display);
    if let Some(sid) = &sid {
        remove_deny.args(["/remove:d", &format!("*{sid}")]);
    }
    remove_deny.args(["/remove:d", "*S-1-1-0"]);
    remove_deny.args(["/t", "/c", "/q"]);
    quiet(&mut remove_deny);
    let removed = matches!(remove_deny.status(), Ok(s) if s.success());

    // Deny 摘掉后再补 Administrators 完全控制，兜住「摘了 Deny 但没 Allow」
    // 的半截状态。SID 而不是名字，理由同上。
    let mut icacls = Command::new("icacls");
    icacls.args([
        &display,
        "/grant",
        "*S-1-5-32-544:(OI)(CI)F",
        "/t",
        "/c",
        "/q",
    ]);
    quiet(&mut icacls);
    let granted = matches!(icacls.status(), Ok(s) if s.success());
    // 两步任一成功都值得重试删除：/remove:d 成功但 /grant 失败时，
    // 原来的 Allow（用户 FullControl）往往还在，照样删得掉。
    removed || granted
}

/// 若当前未提权，通过 Windows UAC (runas) 自重启当前进程并退出当前无权限进程。
/// 若提权成功，返回 true；若用户取消或提权失败，返回 false。
pub fn relaunch_as_admin_if_needed() -> bool {
    if is_elevated() {
        return true;
    }

    use std::os::windows::ffi::OsStrExt;
    use winapi::um::shellapi::ShellExecuteW;
    use winapi::um::winuser::SW_SHOWNORMAL;

    let Ok(exe_path) = std::env::current_exe() else {
        return false;
    };

    let exe_wide: Vec<u16> = exe_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let op_wide: Vec<u16> = std::ffi::OsStr::new("runas")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut args: Vec<String> = std::env::args().skip(1).collect();

    // 跨账户提权（OTS）支持：提权前捕获真实前台用户的目录与 SID，
    // 传递给提权子进程，防止提权后路径错位指向管理员 Profile。
    if !args.iter().any(|a| a == "--orig-user-home") {
        if let Some(home) = dirs::home_dir()
            .or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from))
        {
            args.push("--orig-user-home".into());
            args.push(home.to_string_lossy().to_string());
        }
    }
    if !args.iter().any(|a| a == "--orig-user-sid") {
        if let Some(sid) = current_user_sid() {
            args.push("--orig-user-sid".into());
            args.push(sid);
        }
    }

    let args_str = args
        .iter()
        .map(|a| quote_win_arg(a))
        .collect::<Vec<_>>()
        .join(" ");
    let args_wide: Vec<u16> = std::ffi::OsStr::new(&args_str)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: 所有传入的宽字符串都是本地 Vec 且以 NUL 结尾，活到调用结束。
    // ShellExecuteW 只读它们，不持有。
    let res = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            op_wide.as_ptr(),
            exe_wide.as_ptr(),
            if args.is_empty() {
                ptr::null()
            } else {
                args_wide.as_ptr()
            },
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    if res as usize > 32 {
        std::process::exit(0);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quote_win_arg_simple() {
        assert_eq!(quote_win_arg("hello"), "hello");
        assert_eq!(quote_win_arg("--no-elevate"), "--no-elevate");
    }

    #[test]
    fn test_quote_win_arg_with_spaces() {
        assert_eq!(quote_win_arg("hello world"), "\"hello world\"");
        assert_eq!(
            quote_win_arg("C:\\Program Files\\App"),
            "\"C:\\Program Files\\App\""
        );
    }

    #[test]
    fn test_quote_win_arg_with_quotes_and_slashes() {
        assert_eq!(quote_win_arg("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote_win_arg("a\\\"b"), "\"a\\\\\\\"b\"");
        assert_eq!(quote_win_arg("C:\\dir\\"), "C:\\dir\\");
        assert_eq!(
            quote_win_arg("C:\\dir with spaces\\"),
            "\"C:\\dir with spaces\\\\\""
        );
    }
}
