//! 以管理员身份批量删除残留
//!
//! `/Library` 下的 launchd plist 和系统级支持目录都是 `root:wheel`，普通
//! 进程删不动。这里把一次清理里所有需要提权的路径合成**一条**脚本，通过
//! `osascript ... with administrator privileges` 执行，用户只需输一次密码。
//!
//! 安全约束（三条都必须成立才会有路径进入脚本）：
//! 1. 路径通过 [`is_elevated_residual_target`] 的父目录白名单
//! 2. 路径不是符号链接（白名单同样会拒绝）
//! 3. 路径本身从不拼进 AppleScript 源码，只经 `argv` 传入，再由
//!    `quoted form of` 转义后交给 shell

use crate::core::safety::is_elevated_residual_target;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// 交给 `osascript` 的脚本。路径全部走 `argv`，脚本文本是常量，
/// 因此不存在把文件名当代码执行的可能。
///
/// `do shell script` 只调用一次：每调用一次就弹一次密码框。
const REMOVE_SCRIPT: &str = r#"on run argv
	set cmd to ""
	repeat with rawArg in argv
		set arg to rawArg as text
		if arg starts with "daemon:" then
			set p to text 8 thru -1 of arg
			set cmd to cmd & "/bin/launchctl bootout system " & quoted form of p & " >/dev/null 2>&1; /bin/rm -f " & quoted form of p & " >/dev/null 2>&1; "
		else if arg starts with "agent:" then
			set p to text 7 thru -1 of arg
			set cmd to cmd & "/bin/launchctl bootout gui/$(/usr/bin/id -u) " & quoted form of p & " >/dev/null 2>&1; /bin/rm -f " & quoted form of p & " >/dev/null 2>&1; "
		else
			set p to text 6 thru -1 of arg
			set cmd to cmd & "/bin/rm -rf " & quoted form of p & " >/dev/null 2>&1; "
		end if
	end repeat
	do shell script cmd & "exit 0" with administrator privileges
end run"#;

/// 提权删除时的处理方式。launchd 管理的东西必须先卸载再删文件，
/// 否则 `KeepAlive` 的 daemon 会被立刻拉起来，删掉 plist 也只是让它
/// 变成一个没有配置、却还在跑的孤儿进程。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ElevatedAction {
    /// `/Library/LaunchDaemons` 下的 plist：`launchctl bootout system`
    Daemon,
    /// `/Library/LaunchAgents` 下的 plist：`launchctl bootout gui/<uid>`
    Agent,
    /// 其余目录/文件：直接 `rm -rf`
    Remove,
}

impl ElevatedAction {
    /// 从路径判断该走哪条。只认 `/Library` 下那两个 launchd 目录——
    /// 用户级 `~/Library/LaunchAgents` 不需要提权，走普通删除即可。
    pub fn for_path(path: &Path) -> Self {
        let parent = path
            .parent()
            .map(|p| p.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if parent == "/library/launchdaemons" {
            ElevatedAction::Daemon
        } else if parent == "/library/launchagents" {
            ElevatedAction::Agent
        } else {
            ElevatedAction::Remove
        }
    }

    fn tag(self) -> &'static str {
        match self {
            ElevatedAction::Daemon => "daemon:",
            ElevatedAction::Agent => "agent:",
            ElevatedAction::Remove => "path:",
        }
    }
}

fn effective_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// 该路径是否需要提权才能删。
///
/// 判据是「父目录可写吗」而不是「文件属于谁」：`rm` 要改的是父目录的项，
/// 目标文件自身的权限位不决定能不能删它。
pub fn needs_elevation(path: &Path) -> bool {
    // `access(2)` 按 **real** UID 判权限。有效身份已经是 root 时（`euid==0`），
    // 用户目录可以直接删，不必再走 osascript。`/Library` 白名单路径不能靠
    // 这个短路进 `clean_path`：那边的 `is_protected` 会把整棵 `/Library`
    // 挡死，见 [`needs_privileged_delete`]。
    if effective_root() {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    unsafe { libc::access(cstring(parent).as_ptr(), libc::W_OK) != 0 }
}

/// 残留清理是否必须走 [`elevated_remove`]，而不是 `clean_path`。
///
/// `/Library` 整棵子树在 `is_protected` 里是禁删的。白名单路径就算当前
/// 进程已经能写（标准 `sudo` 下 `access` 对 root 会成功，或 `euid==0`
/// 让 [`needs_elevation`] 短路），也必须进提权批次，由那边套
/// [`is_elevated_residual_target`] 后再删。
pub fn needs_privileged_delete(path: &Path) -> bool {
    is_elevated_residual_target(path) || needs_elevation(path)
}

fn cstring(path: &Path) -> std::ffi::CString {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap_or_default()
}

/// 批量提权删除，返回**确实已经不存在**的路径集合。
///
/// 不看 `osascript` 的退出码：脚本里每条命令都吞掉了错误，一条失败不该
/// 连累其余。删没删掉一律以事后 `symlink_metadata` 为准。
///
/// 用户在密码框点取消时返回空集合，调用方把这些项当作失败保留在列表里，
/// 下次可以重试。
pub fn elevated_remove(paths: &[PathBuf]) -> BTreeSet<PathBuf> {
    // daemon → agent → 普通路径。必须先把 launchd 里的登记卸掉，再删它
    // 指向的 Application Support 目录，否则中间那段时间进程还活着。
    let mut planned: Vec<(ElevatedAction, &PathBuf)> = paths
        .iter()
        .filter(|path| is_elevated_residual_target(path))
        .map(|path| (ElevatedAction::for_path(path), path))
        .collect();
    planned.sort_by_key(|(action, _)| *action);
    if planned.is_empty() {
        return BTreeSet::new();
    }

    // 已经是 root 就不必再弹 osascript 密码框，但仍走同一套白名单和
    // bootout 顺序。`clean_path` 过不了 `is_protected`。
    let ran = if effective_root() {
        for (action, path) in &planned {
            apply_elevated_action(*action, path);
        }
        true
    } else {
        run_osascript(&planned)
    };
    if !ran {
        return BTreeSet::new();
    }

    planned
        .into_iter()
        .map(|(_, path)| path.clone())
        .filter(|path| std::fs::symlink_metadata(path).is_err())
        .collect()
}

fn run_osascript(planned: &[(ElevatedAction, &PathBuf)]) -> bool {
    let args: Vec<String> = planned
        .iter()
        .map(|(action, path)| format!("{}{}", action.tag(), path.to_string_lossy()))
        .collect();
    std::process::Command::new("osascript")
        .arg("-")
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                stdin.write_all(REMOVE_SCRIPT.as_bytes())?;
            }
            child.wait_with_output()
        })
        .is_ok()
}

/// 与 `REMOVE_SCRIPT` 同一套动作，给已经是 root 的进程直接跑，避免再走
/// osascript。路径只经 `Command::arg`，不经过 shell。
fn apply_elevated_action(action: ElevatedAction, path: &Path) {
    match action {
        ElevatedAction::Daemon => {
            let _ = std::process::Command::new("/bin/launchctl")
                .args(["bootout", "system"])
                .arg(path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            let _ = std::process::Command::new("/bin/rm")
                .args(["-f"])
                .arg(path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
        ElevatedAction::Agent => {
            // 真实 UID：seteuid-root 时是原用户（对 gui 域才有意义）；
            // 标准 sudo 两边都是 0，和 osascript 特权 shell 里的 `id -u` 一致。
            let uid = unsafe { libc::getuid() };
            let _ = std::process::Command::new("/bin/launchctl")
                .args(["bootout", &format!("gui/{uid}")])
                .arg(path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            let _ = std::process::Command::new("/bin/rm")
                .args(["-f"])
                .arg(path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
        ElevatedAction::Remove => {
            let _ = std::process::Command::new("/bin/rm")
                .args(["-rf"])
                .arg(path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launchd_paths_get_bootout_and_sort_first() {
        let daemon = PathBuf::from("/Library/LaunchDaemons/org.pqrs.x.plist");
        let agent = PathBuf::from("/Library/LaunchAgents/org.pqrs.y.plist");
        let support = PathBuf::from("/Library/Application Support/org.pqrs");

        assert_eq!(ElevatedAction::for_path(&daemon), ElevatedAction::Daemon);
        assert_eq!(ElevatedAction::for_path(&agent), ElevatedAction::Agent);
        assert_eq!(ElevatedAction::for_path(&support), ElevatedAction::Remove);

        // 用户级 LaunchAgents 不该被当成需要提权的系统项
        assert_eq!(
            ElevatedAction::for_path(Path::new("/Users/me/Library/LaunchAgents/x.plist")),
            ElevatedAction::Remove
        );

        // 卸载登记必须排在删除支持目录之前
        let mut order = [
            ElevatedAction::Remove,
            ElevatedAction::Agent,
            ElevatedAction::Daemon,
        ];
        order.sort();
        assert_eq!(
            order,
            [
                ElevatedAction::Daemon,
                ElevatedAction::Agent,
                ElevatedAction::Remove
            ]
        );
    }

    #[test]
    fn user_owned_paths_do_not_need_elevation() {
        let home = dirs::home_dir().unwrap();
        assert!(!needs_elevation(&home.join("Library/Caches/probe")));
        // /Library 本身不可写，其下的项都要提权（root 下 needs_elevation
        // 会短路，改由 needs_privileged_delete 接住）
        let library_plist = Path::new("/Library/LaunchDaemons/x.plist");
        if effective_root() {
            assert!(!needs_elevation(library_plist));
        } else {
            assert!(needs_elevation(library_plist));
        }
        assert!(needs_privileged_delete(library_plist));
    }

    /// `/Library` 被 `is_protected` 整棵挡住。白名单残留必须走
    /// `elevated_remove`，不能因为「已经是 root / 父目录可写」掉进 `clean_path`。
    #[test]
    fn protected_library_residuals_use_privileged_delete() {
        use crate::core::safety::is_protected;
        let path = Path::new("/Library/LaunchDaemons/x.plist");
        assert!(is_protected(path));
        assert!(
            needs_privileged_delete(path),
            "/Library 残留被 is_protected 挡住，必须走 elevated_remove"
        );
    }

    /// 脚本文本是常量，路径只经 argv 进来——这条断言防止后来有人改成拼字符串。
    #[test]
    fn script_never_interpolates_paths() {
        assert!(!REMOVE_SCRIPT.contains("{}"));
        assert!(REMOVE_SCRIPT.contains("quoted form of"));
        assert_eq!(REMOVE_SCRIPT.matches("do shell script").count(), 1);
    }

    /// 白名单之外的路径必须在进脚本之前就被滤掉，哪怕调用方传了进来。
    #[test]
    fn elevated_remove_rejects_paths_outside_allowlist() {
        assert!(elevated_remove(&[
            PathBuf::from("/System/Library/CoreServices"),
            PathBuf::from("/Library"),
            PathBuf::from("/Library/Application Support"),
            PathBuf::from("/usr/bin/env"),
        ])
        .is_empty());
    }
}
