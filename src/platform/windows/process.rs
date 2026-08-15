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
use winapi::um::processthreadsapi::OpenProcess;
use winapi::um::tlhelp32::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use winapi::um::winbase::QueryFullProcessImageNameW;
use winapi::um::winnt::PROCESS_QUERY_LIMITED_INFORMATION;

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

#[cfg(test)]
mod tests {
    use super::*;

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
            assert!(!p.image_path.contains('/'), "路径未归一化: {}", p.image_path);
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
