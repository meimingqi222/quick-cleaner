//! 带超时地跑一个外部命令，并完整收集它的 stdout/stderr。
//!
//! 存在的理由只有一个：`Command::output()` **没有超时参数**。一个卡住的
//! 子进程（Spotlight 正在重建索引时的 `mdfind`、挂在网络卷上的 `lsof`）
//! 会让调用线程无限期等下去，而这些调用都在「用户点了按钮正在等结果」的
//! 路径上。
//!
//! 命令必须在进程树约束建立后才运行，超时终止整棵树。输出管道以非阻塞
//! 方式轮询，父进程退出但后代仍持有管道时，仍受同一截止时间约束。

#[path = "process_tree.rs"]
mod process_tree;

use std::ffi::OsStr;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 一次子进程运行的完整结果。
///
/// 注意 `ok` 只表示**退出码为 0**，不表示「结果可信」。`lsof +D` 实测无论
/// 空结果还是命中都可能返回 1，调用方必须结合退出码、stdout 和 stderr
/// 判断结果能否使用。
pub struct ProcRun {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// 正常退出时的退出码；被信号终止时为 `None`。
    ///
    /// `ok` 无法区分“命令明确返回非零”和“进程被 SIGKILL/SIGTERM 杀死”。
    /// lsof 只接受正常的 exit 0/1 进入输出解析，因此调用方需要保留这个
    /// 区别，不能让被信号杀死的空输出落进放行分支。
    pub exit_code: Option<i32>,
    pub ok: bool,
}

/// 跑 `program`，最多等 `timeout`。
///
/// 返回 `None` 的三种情况调用方**都应该按「测不出」处理，而不是按「没结果」**：
/// 进程起不来、超时被杀、`try_wait` 自己报错。这三种都意味着「我们不知道
/// 答案」，把它当成空结果就等于在没有依据的情况下放行。
pub fn run_with_timeout<S: AsRef<OsStr>>(
    program: impl AsRef<OsStr>,
    args: &[S],
    timeout: Duration,
) -> Option<ProcRun> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Background owner commands must not allocate a console when launched by the GUI.
        command.creation_flags(winapi::um::winbase::CREATE_NO_WINDOW);
    }
    run_command(command, timeout)
}

/// 跑一条**用户工具链命令**（npm / pnpm / pipx / conda 这类），最多等 `timeout`。
///
/// 与 [`run_with_timeout`] 只差一件事：程序名先经
/// [`platform::tool_command`](crate::platform::tool_command) 解析。Windows 上
/// 这类命令常常只有 `.cmd` 垫片而没有 `.exe`，`CreateProcess` 起不了批处理——
/// 直接 `Command::new("npm")` 会失败，于是 npm / pnpm / pipx 三条通道整体
/// 失效（清单查不到、卸载也跑不起来），而失败表现是「什么都查不到」而不是
/// 报错。
///
/// 返回 `None` 的语义与 `run_with_timeout` 一致：命令不存在、超时被杀、
/// `try_wait` 报错，都按「测不出」处理。工具链这边额外多一种：解析出的垫片
/// 需要经 `cmd.exe` 而参数里出现 cmd 元字符时，同样返回 `None` 拒绝执行。
pub fn run_tool_with_timeout(program: &str, args: &[&str], timeout: Duration) -> Option<ProcRun> {
    let mut command = crate::platform::tool_command(program, args)?;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(winapi::um::winbase::CREATE_NO_WINDOW);
    }
    run_command(command, timeout)
}

/// 进程退出和输出 EOF 都必须在同一截止时间内完成。
fn run_command(mut command: Command, timeout: Duration) -> Option<ProcRun> {
    let deadline = Instant::now().checked_add(timeout)?;
    let (mut child, tree) = process_tree::spawn(&mut command)?;
    let result = (|| {
        let mut out = child.stdout.take()?;
        let mut err = child.stderr.take()?;
        process_tree::prepare_pipe(&out).ok()?;
        process_tree::prepare_pipe(&err).ok()?;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut status = None;
        let mut out_eof = false;
        let mut err_eof = false;
        loop {
            if Instant::now() >= deadline {
                return None;
            }
            let mut bytes = 0;
            if !out_eof {
                let (eof, count) = process_tree::drain_pipe(&mut out, &mut stdout).ok()?;
                out_eof = eof;
                bytes += count;
            }
            if !err_eof {
                let (eof, count) = process_tree::drain_pipe(&mut err, &mut stderr).ok()?;
                err_eof = eof;
                bytes += count;
            }
            if status.is_none() {
                status = child.try_wait().ok()?;
            }
            if let Some(status) = status {
                if out_eof && err_eof {
                    return Some(ProcRun {
                        stdout,
                        stderr,
                        exit_code: status.code(),
                        ok: status.success(),
                    });
                }
            }
            if bytes == 0 {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    })();
    drop(tree);
    let _ = child.kill();
    let _ = child.wait();
    result
}

/// 跑一个闭包，最多等 `timeout`。超时返回 `None`，调用方按失败处理。
///
/// 用于包一层可能卡住的删除 syscall（冻结的 NFS/SMB、被僵死进程占着的
/// 文件）。超时后**不能**取消已经在内核里的 `unlink`——操作系统没有这个
/// 接口——只是让批次继续，不再等这一条。超时线程被 `forget`，syscall
/// 返回后它自己退出；调用方不得假设超时后目标一定还在。
///
/// 只应用在「用户可见的一条目标」上，不要包每一个文件：一次缓存清理
/// 动辄十万个 inode，为每个 `unlink` 起线程会先把自己打崩。
pub fn call_with_timeout<T: Send + 'static>(
    timeout: Duration,
    f: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let handle = std::thread::Builder::new()
        .name("qc-timeout".into())
        .spawn(move || {
            let _ = tx.send(f());
        })
        .ok()?;
    match rx.recv_timeout(timeout) {
        Ok(value) => {
            let _ = handle.join();
            Some(value)
        }
        Err(_) => {
            std::mem::forget(handle);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn review_timeout_kills_shim_descendants_and_closes_pipes() {
        let dir = crate::core::testing::fixture("qc_review_process_timeout");
        let shim = dir.join("probe.cmd");
        let marker = dir.join("finished.txt");
        let ready = dir.join("started.txt");
        std::fs::write(&shim, format!("@echo off\r\npowershell.exe -NoProfile -NonInteractive -Command \"[IO.File]::WriteAllText('{}', 'started'); Start-Sleep -Seconds 4; [IO.File]::WriteAllText('{}', 'still running')\"\r\n", ready.display(), marker.display())).unwrap();
        let start = Instant::now();
        let run = run_tool_with_timeout(&shim.to_string_lossy(), &[], Duration::from_secs(2));
        let elapsed = start.elapsed();
        std::thread::sleep(Duration::from_secs(4));
        let mutated = marker.exists();
        let started = ready.exists();
        let _ = std::fs::remove_dir_all(dir);
        assert!(run.is_none());
        assert!(started, "test must actually launch the descendant");
        assert!(elapsed < Duration::from_secs(3), "elapsed: {elapsed:?}");
        assert!(
            !mutated,
            "descendant must stop modifying files after timeout"
        );
    }

    #[cfg(windows)]
    #[test]
    fn review_exited_parent_does_not_bypass_the_pipe_deadline() {
        let dir = crate::core::testing::fixture("qc_review_exited_parent");
        let shim = dir.join("probe.cmd");
        let script = dir.join("child.ps1");
        let ready = dir.join("started.txt");
        let marker = dir.join("finished.txt");
        std::fs::write(&script, format!("[IO.File]::WriteAllText('{}', 'started'); Start-Sleep -Seconds 4; [IO.File]::WriteAllText('{}', 'still running')", ready.display(), marker.display())).unwrap();
        std::fs::write(&shim, format!("@echo off\r\nstart /b \"\" powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"{}\"\r\nexit /b 0\r\n", script.display())).unwrap();
        let start = Instant::now();
        let run = run_tool_with_timeout(&shim.to_string_lossy(), &[], Duration::from_secs(2));
        let elapsed = start.elapsed();
        std::thread::sleep(Duration::from_secs(4));
        let started = ready.exists();
        let mutated = marker.exists();
        let _ = std::fs::remove_dir_all(dir);
        assert!(started, "test must actually launch the descendant");
        assert!(run.is_none(), "EOF is required as well as parent exit");
        assert!(elapsed < Duration::from_secs(3), "elapsed: {elapsed:?}");
        assert!(!mutated);
    }

    #[cfg(windows)]
    #[test]
    fn review_large_stdout_and_stderr_are_drained_without_deadlock() {
        let script = "$s = 'x' * 100; for ($i=0; $i -lt 5000; $i++) { [Console]::Out.WriteLine($s); [Console]::Error.WriteLine($s) }; exit 7";
        let run = run_with_timeout(
            "powershell.exe",
            &["-NoProfile", "-NonInteractive", "-Command", script],
            Duration::from_secs(30),
        )
        .expect("both pipes must be drained");
        assert_eq!(run.exit_code, Some(7));
        assert_eq!(run.stdout.len(), 510_000);
        assert_eq!(run.stderr.len(), 510_000);
    }

    /// 探测起不来时的定位信息。共享 runner 在重负载下见过 `CreateProcess`
    /// 返回「找不到指定的文件」（同一镜像的相邻一轮同用例是绿的）；不重试、
    /// 不跳过——探测没跑就不能算过——但把 PATH 与解析结果带进 panic，下次红
    /// 了能一眼分出环境抖动与真实回归（例如 powershell 真被移出镜像）。
    #[cfg(windows)]
    fn powershell_resolution() -> String {
        let path = std::env::var("PATH").unwrap_or_else(|_| "<未设置>".into());
        let resolved = std::process::Command::new("where.exe")
            .arg("powershell.exe")
            .output()
            .map(|out| {
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                format!("{stdout} {stderr}").trim().to_string()
            })
            .unwrap_or_else(|error| format!("where.exe 自身也起不来：{error}"));
        format!("powershell.exe 解析结果：{resolved}；PATH={path}")
    }

    #[cfg(windows)]
    #[test]
    fn windows_background_command_has_no_console_and_keeps_output_and_exit_code() {
        let script = "Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class ConsoleProbe { [DllImport(\"kernel32.dll\")] public static extern IntPtr GetConsoleWindow(); }'; [Console]::Out.WriteLine([ConsoleProbe]::GetConsoleWindow().ToInt64()); [Console]::Error.WriteLine('stderr-probe'); exit 7";
        let run = match run_with_timeout(
            "powershell.exe",
            &["-NoProfile", "-NonInteractive", "-Command", script],
            Duration::from_secs(30),
        ) {
            Some(run) => run,
            None => panic!(
                "Windows console probe must run; {}",
                powershell_resolution()
            ),
        };
        assert_eq!(
            String::from_utf8_lossy(&run.stdout).trim(),
            "0",
            "child must have no console window"
        );
        assert!(String::from_utf8_lossy(&run.stderr).contains("stderr-probe"));
        assert_eq!(run.exit_code, Some(7));
        assert!(!run.ok);
    }

    #[cfg(unix)]
    #[test]
    fn collects_stdout_and_exit_status() {
        let run = run_with_timeout("/bin/echo", &["hello"], Duration::from_secs(5))
            .expect("echo 必须能跑起来");
        assert!(run.ok);
        assert_eq!(run.exit_code, Some(0));
        assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "hello");
    }

    #[cfg(unix)]
    #[test]
    fn nonzero_exit_is_reported_but_not_an_error() {
        let run = run_with_timeout("/bin/sh", &["-c", "exit 3"], Duration::from_secs(5))
            .expect("sh 必须能跑起来");
        assert!(!run.ok, "退出码非零要如实反映在 ok 上");
        assert_eq!(run.exit_code, Some(3));
    }

    #[cfg(unix)]
    #[test]
    fn signal_termination_has_no_exit_code() {
        let run = run_with_timeout("/bin/sh", &["-c", "kill -TERM $$"], Duration::from_secs(5))
            .expect("进程确实启动并被信号终止，不是执行器失败");
        assert!(!run.ok);
        assert_eq!(run.exit_code, None);
    }

    #[test]
    fn call_with_timeout_returns_value_before_deadline() {
        let got = call_with_timeout(Duration::from_secs(2), || 7);
        assert_eq!(got, Some(7));
    }

    #[test]
    fn call_with_timeout_returns_none_after_deadline() {
        let start = Instant::now();
        let got = call_with_timeout(Duration::from_millis(80), || {
            std::thread::sleep(Duration::from_secs(30));
            1
        });
        assert!(got.is_none());
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "超时后应立刻返回，实际耗时 {:?}",
            start.elapsed()
        );
    }

    /// 超时必须返回 `None`（= 测不出），而不是返回一个 `ok: false` 的空结果
    /// ——后者会被调用方误读成「命令正常跑完、什么都没找到」。
    #[cfg(unix)]
    #[test]
    fn timeout_returns_none() {
        let start = Instant::now();
        let run = run_with_timeout("/bin/sleep", &["30"], Duration::from_millis(200));
        assert!(run.is_none(), "超时必须是 None");
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "超时后应立刻返回，实际耗时 {:?}",
            start.elapsed()
        );
    }

    /// 子进程输出远超管道缓冲区时不能死锁——这正是要单独起读线程的原因。
    #[cfg(unix)]
    #[test]
    fn large_output_does_not_deadlock() {
        let run = run_with_timeout(
            "/bin/sh",
            &[
                "-c",
                "for i in $(seq 1 20000); do echo aaaaaaaaaaaaaaaaaaaa; done",
            ],
            Duration::from_secs(20),
        )
        .expect("不该超时——超时就说明卡在管道上了");
        assert!(run.ok);
        assert!(run.stdout.len() > 400_000, "输出应远超管道缓冲区");
    }

    #[test]
    fn missing_program_is_none() {
        let run = run_with_timeout(
            "/nonexistent/definitely-not-a-real-binary",
            &["x"],
            Duration::from_secs(5),
        );
        assert!(run.is_none());
    }
}
