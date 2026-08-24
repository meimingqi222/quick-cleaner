//! 进程枚举：判断卸载程序是否真的跑完了
//!
//! `Child::wait()` 在这里是靠不住的。绝大多数安装器（Inno Setup、NSIS、
//! 各家自研的）都会把自己复制到临时目录、以管理员身份重启，**然后原进程
//! 立刻退出**。于是 `wait()` 马上返回，而真正的卸载向导才刚显示出来——
//! 表现就是「卸载器界面和残留清理界面一起弹出来」。
//!
//! 可靠的判据是「跟这个软件相关的进程是否还活着」：
//!
//! - 映像路径落在安装目录内的进程；
//! - 与卸载器同名（不含扩展名）的进程——Inno 会把 `unins000.exe` 复制成
//!   临时目录下的 `unins000.tmp`，扩展名变了但主名不变。

use std::time::{Duration, Instant};

use winapi::shared::minwindef::{DWORD, FALSE, MAX_PATH};
use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
use winapi::um::processthreadsapi::{OpenProcess, TerminateProcess};
use winapi::um::tlhelp32::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use winapi::um::winbase::QueryFullProcessImageNameW;
use winapi::um::winnt::{PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE};

/// 一个正在运行的进程。
pub struct RunningProcess {
    pub pid: DWORD,
    /// 可执行文件名（含扩展名，小写）
    pub exe_name: String,
    /// 完整映像路径（小写，`/` 已换成 `\`）；取不到时为空
    pub image_path: String,
}

/// 枚举当前所有进程。
///
/// 取不到映像路径的（权限不足、系统进程）保留条目但 `image_path` 为空，
/// 仍可按 `exe_name` 匹配。
pub fn list_processes() -> Vec<RunningProcess> {
    let mut out = Vec::new();
    // SAFETY: 快照句柄只在 != INVALID_HANDLE_VALUE 时使用，并在返回前
    // CloseHandle。PROCESSENTRY32W 的 dwSize 按文档要求先填成结构体大小，
    // Process32FirstW/NextW 才会正确填充。
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }

        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as DWORD;

        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let exe_name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
                out.push(RunningProcess {
                    pid: entry.th32ProcessID,
                    image_path: image_path_of(entry.th32ProcessID),
                    exe_name,
                });
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    out
}

fn image_path_of(pid: DWORD) -> String {
    // SAFETY: 进程句柄打不开时（权限不足的系统进程）返回空句柄，此时直接
    // 返回空串，不会拿它去调 API。缓冲区是本地数组，长度如实上报。
    unsafe {
        // 用 LIMITED_INFORMATION 而不是 QUERY_INFORMATION：前者对非同权限
        // 进程也常能成功，够我们读路径了
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
        if h.is_null() {
            return String::new();
        }
        let mut buf = [0u16; MAX_PATH * 2];
        let mut size = buf.len() as DWORD;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut size);
        CloseHandle(h);
        if ok == 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..size as usize])
            .to_lowercase()
            .replace('/', "\\")
    }
}

/// 是否还有跟该软件相关的进程在运行。
pub fn has_related_process(install_dir: &str, uninstaller_stem: &str) -> bool {
    let dir_prefix = if install_dir.is_empty() {
        String::new()
    } else {
        format!("{}\\", install_dir.trim_end_matches('\\'))
    };

    list_processes().iter().any(|p| {
        if !dir_prefix.is_empty() && p.image_path.starts_with(&dir_prefix) {
            return true;
        }
        if uninstaller_stem.is_empty() {
            return false;
        }
        // Inno 把 unins000.exe 复制成 unins000.tmp，主名不变
        let stem = p
            .exe_name
            .rsplit_once('.')
            .map(|(s, _)| s)
            .unwrap_or(&p.exe_name);
        stem == uninstaller_stem
    })
}

/// 等到跟该软件相关的进程全部退出。
///
/// `grace` 是启动后的宽限期：卸载器提权重启需要一点时间，太早检查会看到
/// 「一个都没有」而误判为已完成。
///
/// 返回是否在超时前等到了结束。
pub fn wait_until_finished(
    install_dir: &str,
    uninstaller_stem: &str,
    grace: Duration,
    timeout: Duration,
) -> bool {
    // 没有任何可判据时不做无谓等待
    if install_dir.is_empty() && uninstaller_stem.is_empty() {
        return true;
    }

    // 宽限期内只要看到相关进程就立刻进入等待，不必等满
    let grace_deadline = Instant::now() + grace;
    let mut seen = false;
    while Instant::now() < grace_deadline {
        if has_related_process(install_dir, uninstaller_stem) {
            seen = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    if !seen {
        return true; // 宽限期内始终没有相关进程，认为已经结束
    }

    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !has_related_process(install_dir, uninstaller_stem) {
            // 再确认一次，避免正好卡在进程切换的缝隙里
            std::thread::sleep(Duration::from_millis(400));
            if !has_related_process(install_dir, uninstaller_stem) {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    false
}

/// 结束映像路径落在 `dirs` 任意一棵目录下的进程。
///
/// 强力清理要删安装目录时，文件被占用会整批失败。只按路径前缀杀，
/// 不按 exe 名全局匹配——`chrome.exe` 这种重名不能误伤。
///
/// 返回成功发出终止请求的个数（进程真正退出可能还要几十毫秒）。
pub fn terminate_processes_under(dirs: &[String]) -> usize {
    let my_pid = std::process::id();
    let prefixes: Vec<String> = dirs
        .iter()
        .filter(|d| d.len() > 3)
        .map(|d| {
            let d = d.trim_end_matches('\\');
            format!("{d}\\")
        })
        .collect();
    if prefixes.is_empty() {
        return 0;
    }

    let mut n = 0usize;
    for p in list_processes() {
        if p.pid == 0 || p.pid == 4 || p.pid == my_pid || p.image_path.is_empty() {
            continue;
        }
        let hit = prefixes.iter().any(|pre| {
            let dir = pre.trim_end_matches('\\');
            p.image_path == dir || p.image_path.starts_with(pre.as_str())
        });
        if !hit {
            continue;
        }
        if terminate_pid(p.pid) {
            n += 1;
        }
    }
    n
}

fn terminate_pid(pid: DWORD) -> bool {
    // SAFETY: 句柄只在 OpenProcess 成功时使用，用完关闭。TerminateProcess
    // 的退出码 1 只是标记，没有约定含义。
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, FALSE, pid);
        if h.is_null() {
            return false;
        }
        let ok = TerminateProcess(h, 1) != 0;
        CloseHandle(h);
        ok
    }
}

/// winget 对「管理员进程卸载用户范围的包」返回这个码。
pub const WINGET_ADMIN_CONTEXT_PROHIBITED: u32 = 0x8A15_007D;

/// 跑一条命令并等待退出，返回进程退出码。
///
/// `unelevated` 为真时，如果当前进程已经 UAC 提权，会用桌面用户（资源
/// 管理器）的令牌把命令降权再跑。本程序启动时会自提权，而 winget 拒绝在
/// 管理员上下文里动用户范围的包（`0x8A15007D`），不降权的话命令「执行完」
/// 软件还在。
pub fn run_cmd_and_wait(cmdline: &str, hidden: bool, unelevated: bool) -> Result<u32, String> {
    if unelevated && super::security::is_elevated() {
        run_as_desktop_user(cmdline, hidden)
    } else {
        run_as_current(cmdline, hidden)
    }
}

fn run_as_current(cmdline: &str, hidden: bool) -> Result<u32, String> {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new("cmd");
    c.raw_arg(format!("/c {cmdline}"));
    if hidden {
        c.creation_flags(winapi::um::winbase::CREATE_NO_WINDOW);
    }
    let status = c
        .status()
        .map_err(|e| format!("启动卸载程序失败: {e}"))?;
    Ok(status.code().map(|c| c as u32).unwrap_or(1))
}

fn run_as_desktop_user(cmdline: &str, hidden: bool) -> Result<u32, String> {
    enable_impersonate_privilege();
    let token = desktop_user_token()?;
    // SAFETY: token 来自 OpenProcessToken / TokenLinkedToken，非空；本函数
    // 所有出口都 CloseHandle。CreateProcessWithTokenW 的命令行缓冲必须可变
    // （API 会原地改），wide 以 NUL 结尾且活到调用返回。
    let code = unsafe { create_process_with_token(token, cmdline, hidden) };
    unsafe {
        CloseHandle(token);
    }
    code
}

/// `CreateProcessWithTokenW` 要求调用方持有 `SeImpersonatePrivilege`。
/// 管理员令牌里通常有这个特权，但可能是禁用状态，这里打开再继续。
fn enable_impersonate_privilege() {
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::processthreadsapi::{GetCurrentProcess, OpenProcessToken};
    use winapi::um::securitybaseapi::AdjustTokenPrivileges;
    use winapi::um::winbase::LookupPrivilegeValueW;
    use winapi::um::winnt::{
        LUID, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    let name: Vec<u16> = std::ffi::OsStr::new("SeImpersonatePrivilege")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: name 以 NUL 结尾；令牌句柄打开成功才用，出口关闭。
    // 打不开或调整失败都不致命——后面 CreateProcessWithTokenW 会给出明确错误。
    unsafe {
        let mut luid = LUID {
            LowPart: 0,
            HighPart: 0,
        };
        if LookupPrivilegeValueW(std::ptr::null(), name.as_ptr(), &mut luid) == 0 {
            return;
        }
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        ) == 0
        {
            return;
        }
        let mut tp: TOKEN_PRIVILEGES = std::mem::zeroed();
        tp.PrivilegeCount = 1;
        tp.Privileges[0].Luid = luid;
        tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
        AdjustTokenPrivileges(
            token,
            FALSE,
            &mut tp,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        CloseHandle(token);
    }
}

fn desktop_user_token() -> Result<winapi::um::winnt::HANDLE, String> {
    if let Some(h) = explorer_token() {
        return Ok(h);
    }
    linked_limited_token().ok_or_else(|| {
        "无法获取桌面用户令牌（资源管理器进程打不开，当前进程也没有关联的受限令牌）".to_string()
    })
}

fn session_of(pid: DWORD) -> Option<DWORD> {
    let mut session = 0;
    // SAFETY: session 是本地 DWORD，ProcessIdToSessionId 只写入这一格。
    let ok = unsafe { winapi::um::processthreadsapi::ProcessIdToSessionId(pid, &mut session) };
    if ok != 0 {
        Some(session)
    } else {
        None
    }
}

fn explorer_token() -> Option<winapi::um::winnt::HANDLE> {
    let my_session = session_of(std::process::id());
    let mut fallback: Option<DWORD> = None;
    for p in list_processes() {
        if p.exe_name != "explorer.exe" || p.pid == 0 {
            continue;
        }
        if my_session.is_some() && session_of(p.pid) == my_session {
            if let Some(h) = open_primary_token(p.pid) {
                return Some(h);
            }
        } else if fallback.is_none() {
            fallback = Some(p.pid);
        }
    }
    fallback.and_then(open_primary_token)
}

fn open_primary_token(pid: DWORD) -> Option<winapi::um::winnt::HANDLE> {
    use winapi::um::securitybaseapi::DuplicateTokenEx;
    use winapi::um::winnt::{
        SecurityImpersonation, TokenPrimary, TOKEN_ADJUST_DEFAULT, TOKEN_ADJUST_SESSIONID,
        TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_QUERY,
    };
    // SAFETY: 进程/令牌句柄只在打开成功后使用，失败路径和函数出口都关闭。
    // DuplicateTokenEx 产出的主令牌交给调用方，由调用方 CloseHandle。
    unsafe {
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
        if proc.is_null() {
            return None;
        }
        let mut existing = std::ptr::null_mut();
        let opened = winapi::um::processthreadsapi::OpenProcessToken(
            proc,
            TOKEN_DUPLICATE | TOKEN_QUERY,
            &mut existing,
        );
        CloseHandle(proc);
        if opened == 0 || existing.is_null() {
            return None;
        }
        let mut primary = std::ptr::null_mut();
        let ok = DuplicateTokenEx(
            existing,
            TOKEN_QUERY
                | TOKEN_DUPLICATE
                | TOKEN_ASSIGN_PRIMARY
                | TOKEN_ADJUST_DEFAULT
                | TOKEN_ADJUST_SESSIONID,
            std::ptr::null_mut(),
            SecurityImpersonation,
            TokenPrimary,
            &mut primary,
        );
        CloseHandle(existing);
        if ok == 0 || primary.is_null() {
            None
        } else {
            Some(primary)
        }
    }
}

fn linked_limited_token() -> Option<winapi::um::winnt::HANDLE> {
    use winapi::um::processthreadsapi::{GetCurrentProcess, OpenProcessToken};
    use winapi::um::securitybaseapi::{DuplicateTokenEx, GetTokenInformation};
    use winapi::um::winnt::{
        SecurityImpersonation, TokenLinkedToken, TokenPrimary, TOKEN_ADJUST_DEFAULT,
        TOKEN_ADJUST_SESSIONID, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_QUERY,
        TOKEN_LINKED_TOKEN,
    };
    // SAFETY: 当前进程令牌打开失败就返回；GetTokenInformation 写入本地
    // TOKEN_LINKED_TOKEN，得到的 LinkedToken 再复制成主令牌后立刻关掉。
    unsafe {
        let mut elevated = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut elevated) == 0 {
            return None;
        }
        let mut linked: TOKEN_LINKED_TOKEN = std::mem::zeroed();
        let mut size: DWORD = 0;
        let ok = GetTokenInformation(
            elevated,
            TokenLinkedToken,
            &mut linked as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_LINKED_TOKEN>() as DWORD,
            &mut size,
        );
        CloseHandle(elevated);
        if ok == 0 || linked.LinkedToken.is_null() {
            return None;
        }
        let mut primary = std::ptr::null_mut();
        let dup = DuplicateTokenEx(
            linked.LinkedToken,
            TOKEN_QUERY
                | TOKEN_DUPLICATE
                | TOKEN_ASSIGN_PRIMARY
                | TOKEN_ADJUST_DEFAULT
                | TOKEN_ADJUST_SESSIONID,
            std::ptr::null_mut(),
            SecurityImpersonation,
            TokenPrimary,
            &mut primary,
        );
        CloseHandle(linked.LinkedToken);
        if dup == 0 || primary.is_null() {
            None
        } else {
            Some(primary)
        }
    }
}

unsafe fn create_process_with_token(
    token: winapi::um::winnt::HANDLE,
    cmdline: &str,
    hidden: bool,
) -> Result<u32, String> {
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::processthreadsapi::{
        GetExitCodeProcess, PROCESS_INFORMATION, STARTUPINFOW,
    };
    use winapi::um::synchapi::WaitForSingleObject;
    use winapi::um::winbase::{
        CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, STARTF_USESHOWWINDOW, INFINITE,
    };
    use winapi::um::winuser::SW_HIDE;

    let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let cmd_exe = format!("{sys}\\System32\\cmd.exe");
    let app: Vec<u16> = std::ffi::OsStr::new(&cmd_exe)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut cl: Vec<u16> = std::ffi::OsStr::new(&format!("cmd.exe /c {cmdline}"))
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut si: STARTUPINFOW = std::mem::zeroed();
    si.cb = std::mem::size_of::<STARTUPINFOW>() as DWORD;
    if hidden {
        si.dwFlags = STARTF_USESHOWWINDOW;
        si.wShowWindow = SW_HIDE as u16;
    }
    let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
    let mut flags: DWORD = CREATE_UNICODE_ENVIRONMENT;
    if hidden {
        flags |= CREATE_NO_WINDOW;
    }

    // 用桌面用户自己的环境块：LOCALAPPDATA / PATH 都是他的。
    // 传 NULL 会继承本进程（管理员）的环境，winget 就会去管理员
    // 的包索引里找，用户范围的 crush 仍然「找不到」。
    let mut env: winapi::shared::minwindef::LPVOID = std::ptr::null_mut();
    let has_env = CreateEnvironmentBlock(&mut env, token, 0) != 0;

    // SAFETY: app / cl 以 NUL 结尾且活到调用返回；cl 按约定可变。
    // env 由 CreateEnvironmentBlock 分配，调用返回后 Destroy。
    // 成功后关闭线程句柄，进程句柄等到 Wait 之后再关。
    let ok = CreateProcessWithTokenW(
        token,
        0,
        app.as_ptr(),
        cl.as_mut_ptr(),
        flags,
        if has_env {
            env
        } else {
            std::ptr::null_mut()
        },
        std::ptr::null(),
        &mut si,
        &mut pi,
    );
    if has_env && !env.is_null() {
        DestroyEnvironmentBlock(env);
    }
    if ok == 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("降权启动卸载程序失败: {err}"));
    }
    if !pi.hThread.is_null() {
        CloseHandle(pi.hThread);
    }
    WaitForSingleObject(pi.hProcess, INFINITE);
    let mut code: DWORD = 1;
    let _ = GetExitCodeProcess(pi.hProcess, &mut code);
    CloseHandle(pi.hProcess);
    Ok(code)
}

// winapi 0.3 的 winbase 特性把 CreateProcessWithTokenW 放在可选绑定里，
// 有的 feature 组合编不出来。签名与 SDK 一致，在此手动声明。
#[link(name = "userenv")]
extern "system" {
    fn CreateEnvironmentBlock(
        lpenvironment: *mut winapi::shared::minwindef::LPVOID,
        htoken: winapi::um::winnt::HANDLE,
        binherit: winapi::shared::minwindef::BOOL,
    ) -> winapi::shared::minwindef::BOOL;
    fn DestroyEnvironmentBlock(
        lpenvironment: winapi::shared::minwindef::LPVOID,
    ) -> winapi::shared::minwindef::BOOL;
}

#[link(name = "advapi32")]
extern "system" {
    fn CreateProcessWithTokenW(
        htoken: winapi::um::winnt::HANDLE,
        dwlogonflags: DWORD,
        lpapplicationname: winapi::um::winnt::LPCWSTR,
        lpcommandline: winapi::um::winnt::LPWSTR,
        dwcreationflags: DWORD,
        lpenvironment: winapi::shared::minwindef::LPVOID,
        lpcurrentdirectory: winapi::um::winnt::LPCWSTR,
        lpstartupinfo: winapi::um::processthreadsapi::LPSTARTUPINFOW,
        lpprocessinformation: winapi::um::processthreadsapi::LPPROCESS_INFORMATION,
    ) -> winapi::shared::minwindef::BOOL;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_exit_zero_returns_zero() {
        let code = run_cmd_and_wait("exit 0", true, false).expect("run cmd");
        assert_eq!(code, 0);
    }

    #[test]
    fn unelevated_exit_zero_returns_zero() {
        let code = run_cmd_and_wait("exit 0", true, true).expect("run unelevated");
        assert_eq!(code, 0);
    }

    #[test]
    fn desktop_token_can_spawn_cmd() {
        if !crate::platform::windows::security::is_elevated() {
            return;
        }
        enable_impersonate_privilege();
        let token = desktop_user_token().expect("desktop user token");
        let result = unsafe { create_process_with_token(token, "exit 0", true) };
        unsafe {
            CloseHandle(token);
        }
        assert_eq!(result.expect("spawn with desktop token"), 0);
    }

    #[test]
    fn terminate_under_empty_dirs_is_noop() {
        assert_eq!(terminate_processes_under(&[]), 0);
        assert_eq!(terminate_processes_under(&[String::new(), "c:".into()]), 0);
    }

    #[test]
    fn enumerates_at_least_this_process() {
        let procs = list_processes();
        assert!(!procs.is_empty(), "至少应该枚举到自己");
        let me = std::process::id();
        assert!(
            procs.iter().any(|p| p.pid == me),
            "当前进程 {me} 不在枚举结果里"
        );
    }

    #[test]
    fn image_paths_are_normalised() {
        for p in list_processes().iter().filter(|p| !p.image_path.is_empty()) {
            assert!(
                !p.image_path.contains('/'),
                "路径未归一化: {}",
                p.image_path
            );
            assert_eq!(p.image_path, p.image_path.to_lowercase());
        }
    }

    /// 当前进程的 exe 一定在跑，按它的目录应该能匹配到。
    #[test]
    fn detects_a_process_in_a_known_directory() {
        let exe = std::env::current_exe().expect("拿不到当前可执行文件路径");
        let dir = exe.parent().expect("拿不到所在目录");
        let dir_lower = dir.to_string_lossy().to_lowercase().replace('/', "\\");
        assert!(
            has_related_process(&dir_lower, ""),
            "自己就跑在 {dir_lower} 下，却没被检出"
        );
    }

    #[test]
    fn no_criteria_means_no_waiting() {
        let start = Instant::now();
        assert!(wait_until_finished(
            "",
            "",
            Duration::from_secs(5),
            Duration::from_secs(5)
        ));
        assert!(start.elapsed() < Duration::from_millis(200), "不该有等待");
    }

    /// 目录里不存在任何进程时，宽限期一过就该返回，不能干等满超时。
    #[test]
    fn absent_process_returns_after_grace() {
        let start = Instant::now();
        let done = wait_until_finished(
            r"c:\definitely\not\a\real\install\dir",
            "definitely_not_a_real_process_name",
            Duration::from_millis(400),
            Duration::from_secs(30),
        );
        assert!(done);
        assert!(start.elapsed() < Duration::from_secs(3), "等待过久");
    }

    #[test]
    fn unrelated_path_is_not_matched() {
        assert!(!has_related_process(r"c:\no\such\place\at\all", ""));
    }

    /// 前缀匹配必须对齐路径分隔符，不能把 `foobar` 当成 `foo` 的子目录。
    #[test]
    fn prefix_match_respects_separator() {
        let exe = std::env::current_exe().unwrap();
        let dir = exe.parent().unwrap().to_string_lossy().to_lowercase();
        let sibling = format!("{}xyz", dir.trim_end_matches('\\'));
        assert!(!has_related_process(&sibling, ""));
    }
}
