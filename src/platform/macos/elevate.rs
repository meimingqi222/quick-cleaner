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

/// 该路径是否需要提权才能删。
///
/// 判据是「父目录可写吗」而不是「文件属于谁」：`rm` 要改的是父目录的项，
/// 目标文件自身的权限位不决定能不能删它。
pub fn needs_elevation(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    unsafe { libc::access(cstring(parent).as_ptr(), libc::W_OK) != 0 }
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
    let mut args: Vec<String> = Vec::new();
    let mut targets: Vec<PathBuf> = Vec::new();

    // daemon → agent → 普通路径。必须先把 launchd 里的登记卸掉，再删它
    // 指向的 Application Support 目录，否则中间那段时间进程还活着。
    let mut planned: Vec<(ElevatedAction, &PathBuf)> = paths
        .iter()
        .filter(|path| is_elevated_residual_target(path))
        .map(|path| (ElevatedAction::for_path(path), path))
        .collect();
    planned.sort_by_key(|(action, _)| *action);

    for (action, path) in planned {
        args.push(format!("{}{}", action.tag(), path.to_string_lossy()));
        targets.push(path.clone());
    }
    if targets.is_empty() {
        return BTreeSet::new();
    }

    let output = std::process::Command::new("osascript")
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
        });
    if output.is_err() {
        return BTreeSet::new();
    }

    targets
        .into_iter()
        .filter(|path| std::fs::symlink_metadata(path).is_err())
        .collect()
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
        // /Library 本身不可写，其下的项都要提权
        assert!(needs_elevation(Path::new("/Library/LaunchDaemons/x.plist")));
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
