//! 带超时地跑一个外部命令，并完整收集它的 stdout/stderr。
//!
//! 存在的理由只有一个：`Command::output()` **没有超时参数**。一个卡住的
//! 子进程（Spotlight 正在重建索引时的 `mdfind`、挂在网络卷上的 `lsof`）
//! 会让调用线程无限期等下去，而这些调用都在「用户点了按钮正在等结果」的
//! 路径上。
//!
//! 两个容易写错、这里已经处理掉的细节：
//!
//! 1. **stdout/stderr 必须各起一个线程读到底**。如果只在父线程里等
//!    `try_wait()`、最后才读管道，子进程写满管道缓冲区（macOS 上通常
//!    64KB）之后会阻塞在 write 上永远不退出，而父线程在等它退出——两边
//!    互相等，超时逻辑本身也救不回来（`kill` 之前就已经卡在 join 上了）。
//! 2. **超时后要 `kill` 再 `wait`**。只 `kill` 不 `wait` 会留下僵尸进程。
//!
//! 这份实现原本长在 `core::inuse` 里、写死了 `lsof` 的路径。抽出来是因为
//! 残留清理的 `mdfind` 反查需要同一套逻辑——本仓库对「同一份判断存两份」
//! 有过教训（见 `core::safety` 头注释），进程超时这种带死锁陷阱的代码更
//! 不该有第二份拷贝。

use std::ffi::OsStr;
use std::io::Read;
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
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let mut out_pipe = child.stdout.take()?;
    let mut err_pipe = child.stderr.take()?;
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out_pipe.read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err_pipe.read_to_end(&mut buf);
        buf
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Err(_) => {
                // try_wait 失败同样意味着已经失去对子进程状态的可靠判断。
                // 不能直接去 join 管道读取线程：子进程若仍活着，管道不会
                // 关闭，join 会把这个“带超时”函数永久卡住。
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };

    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    let status = status?;
    Some(ProcRun {
        stdout,
        stderr,
        exit_code: status.code(),
        ok: status.success(),
    })
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

    #[test]
    fn collects_stdout_and_exit_status() {
        let run = run_with_timeout("/bin/echo", &["hello"], Duration::from_secs(5))
            .expect("echo 必须能跑起来");
        assert!(run.ok);
        assert_eq!(run.exit_code, Some(0));
        assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "hello");
    }

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
    #[test]
    fn large_output_does_not_deadlock() {
        let run = run_with_timeout(
            "/bin/sh",
            &["-c", "for i in $(seq 1 20000); do echo aaaaaaaaaaaaaaaaaaaa; done"],
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
