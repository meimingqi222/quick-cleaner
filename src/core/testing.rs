//! 单元测试夹具目录。
//!
//! # 为什么名字里要带 pid
//!
//! 同时存在两个测试二进制是常态：上一次运行没退干净、CI 并发、手工重跑。
//! 固定名字的夹具会让两个进程互相 `remove_dir_all` 对方的目录，症状是
//! 「应当能读出索引 NotFound」这类看着像产品 bug 的间歇失败——进程内的
//! `Mutex` 串行锁挡不住跨进程冲突。
//!
//! # 为什么还要回收死进程留下的那一份
//!
//! 带 pid 之后每个进程都会新建一个目录，测试 panic（或提前 return）时末尾
//! 那句 `remove_dir_all` 就走不到，于是**每跑一次泄漏一代**。旧写法靠固定名
//! 让下一次运行开头顺手清掉，这个顺带效果被 pid 唯一性拿掉了，所以必须显式
//! 补回来。
//!
//! 回收只针对**同一个 tag 的前缀族**、且**属主进程已经不存在**的那些条目：不
//! 做全局 TMPDIR 扫描，也不碰还活着的进程（可能是并发跑的另一个二进制）的
//! 夹具。
#![cfg(test)]

use std::path::{Path, PathBuf};

/// 建一个属于当前进程、以 `tag` 命名的空夹具目录。
pub(crate) fn fixture(tag: &str) -> PathBuf {
    let root = std::env::temp_dir();
    reclaim_stale(&root, tag);
    let path = root.join(format!("{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap_or_else(|e| panic!("建夹具目录 {path:?} 失败：{e}"));
    path
}

/// 只要一个进程唯一的**文件**路径，不创建任何东西。
///
/// 有一批夹具是给 `fs::write` / `symlink` 当目标用的，用 [`fixture`] 会先建出
/// 一个同名目录，写文件时炸成 `IsADirectory`。
pub(crate) fn file_path(tag: &str) -> PathBuf {
    let root = std::env::temp_dir();
    reclaim_stale(&root, tag);
    root.join(format!("{tag}_{}", std::process::id()))
}

/// 删掉 `{tag}_<pid>` 形式、且那个 pid 已经不在的那些遗留夹具（目录或文件）。
fn reclaim_stale(root: &Path, tag: &str) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    let prefix = format!("{tag}_");
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(suffix) = name.strip_prefix(&prefix) else {
            continue;
        };
        // 只有「整段都是数字」才算我们留下的 pid 后缀。像
        // `qc_readonly_test_9f3a`、`qc_merge_definitely_not_here_8f21` 这种
        // 手工造的名字不匹配，也就不会被误删。
        let Ok(pid) = suffix.parse::<u32>() else {
            continue;
        };
        if pid == std::process::id() || process_is_alive(pid) {
            continue;
        }
        // 目录、普通文件、符号链接都可能是夹具。这里用 `file_type()`（不跟随
        // 链接）而不是 `metadata()`（跟随），否则一个指向别处的链接夹具会被
        // 当成它指向的东西去删。
        match entry.file_type() {
            Ok(ft) if ft.is_dir() => {
                let _ = std::fs::remove_dir_all(entry.path());
            }
            Ok(_) => {
                let _ = std::fs::remove_file(entry.path());
            }
            Err(_) => {}
        }
    }
}

/// 进程是否还在。**判不准一律当作活着**——宁可留几个空目录，也不要在
/// 共用的临时目录里删掉别人正在用的夹具。
fn process_is_alive(pid: u32) -> bool {
    // 只对 macOS 用 libc：`libc` 在 Cargo.toml 里就是按 macOS target 声明的，
    // 写成 `cfg(unix)` 会在别的 unix 上引到一个不存在的 crate。
    #[cfg(target_os = "macos")]
    {
        // signal 0 不投递信号，只回答"能不能发"。`EPERM` 也算活着——那是
        // 别人的进程，删它的夹具同样是错的。
        let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
        if rc == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(windows)]
    {
        use winapi::shared::minwindef::{DWORD, FALSE};
        use winapi::shared::winerror::ERROR_ACCESS_DENIED;
        use winapi::um::handleapi::CloseHandle;
        use winapi::um::processthreadsapi::{GetExitCodeProcess, OpenProcess};
        use winapi::um::winnt::PROCESS_QUERY_LIMITED_INFORMATION;

        // 就地写死而不是引 `winapi::um::minwinbase::STILL_ACTIVE`：那个常量是
        // `STATUS_PENDING as u32`，要连带打开 winapi 的 `ntstatus` feature。
        const STILL_ACTIVE: DWORD = 259;

        // SAFETY: 句柄只在 OpenProcess 成功（非空）时使用，用完立刻关闭；
        // 退出码写进本地变量。
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid as DWORD);
            if h.is_null() {
                // 打不开的原因要分开看：`ERROR_ACCESS_DENIED` 说明进程存在
                // 但不归我们（等价于 unix 的 `EPERM`），其余（典型是
                // `ERROR_INVALID_PARAMETER`）才是"这个 pid 不存在"。
                return std::io::Error::last_os_error().raw_os_error()
                    == Some(ERROR_ACCESS_DENIED as i32);
            }
            let mut code: DWORD = 0;
            let ok = GetExitCodeProcess(h, &mut code);
            CloseHandle(h);
            // 读不到退出码就按活着处理。注意 `STILL_ACTIVE`（259）也可能是
            // 某个进程真正的退出码，这种巧合只会让我们少回收一个目录。
            ok == 0 || code == STILL_ACTIVE
        }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = pid;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个不可能属于任何活进程的 pid：超过 macOS 的 pid 上限（99999），
    /// 且不是 4 的倍数（Windows 的 pid 恒为 4 的倍数）。
    #[cfg(any(target_os = "macos", windows))]
    const DEFINITELY_DEAD_PID: u32 = 4_000_001;

    /// 名字带 pid，所以同一次运行里重复取同一个 tag 拿到的是同一个目录，
    /// 而且它是空的、可用的。
    #[test]
    fn fixture_is_reusable_within_a_process() {
        let tag = "qc_testing_fixture_self";
        let a = fixture(tag);
        std::fs::write(a.join("marker"), b"x").unwrap();
        let b = fixture(tag);
        assert_eq!(a, b);
        assert!(
            !b.join("marker").exists(),
            "重复获取时该清成空目录，否则上一个用例的状态会漏给下一个"
        );
        let _ = std::fs::remove_dir_all(a);
    }

    /// 回收判据是「整段数字 + 进程已死」。活着的一律不碰，非数字后缀一律
    /// 不当成 pid——误删并发运行的夹具或不相关目录，比留几个垃圾严重得多。
    #[test]
    fn reclaim_leaves_live_and_foreign_names_alone() {
        let tag = "qc_testing_fixture_reclaim";
        let root = std::env::temp_dir();
        let mine = root.join(format!("{tag}_{}", std::process::id()));
        let foreign = root.join(format!("{tag}_9f3a"));
        let _ = std::fs::remove_dir_all(&mine);
        let _ = std::fs::remove_dir_all(&foreign);
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::create_dir_all(&foreign).unwrap();

        reclaim_stale(&root, tag);

        assert!(mine.exists(), "本进程自己的目录不该被回收");
        assert!(foreign.exists(), "非 pid 后缀的目录不该被回收");
        let _ = std::fs::remove_dir_all(&mine);
        let _ = std::fs::remove_dir_all(&foreign);
    }

    /// 一个早已结束的进程留下的夹具，应该被回收掉。
    ///
    /// 只在有真存活判据的平台上跑：其他平台的 [`process_is_alive`] 恒为
    /// `true`（有意的保守兜底），那里断言"应该被回收"是错的。
    #[test]
    #[cfg(any(target_os = "macos", windows))]
    fn reclaim_removes_dirs_owned_by_dead_pids() {
        let tag = "qc_testing_fixture_dead";
        let root = std::env::temp_dir();
        // 用一个超出 pid 取值范围的号：macOS 的 pid 上限是 99999，Windows 的
        // pid 是 4 的倍数。写死一个小号（比如 2）会赌"它现在恰好没被用"，
        // 而 pid 回绕之后低号是会被复用的。
        let stale = root.join(format!("{tag}_{}", DEFINITELY_DEAD_PID));
        let _ = std::fs::remove_dir_all(&stale);
        std::fs::create_dir_all(&stale).unwrap();

        reclaim_stale(&root, tag);

        assert!(!stale.exists(), "属主进程已死的夹具该被回收");
    }
}
