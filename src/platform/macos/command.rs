//! 拉起「用户工具链命令」：npm / pnpm / pipx / conda 这一类。
//!
//! Unix 上命令就是可执行文件，没有 Windows 那种批处理垫片的问题。这里仍然
//! 自己查一次 `PATH`，是因为调用方需要区分两件事：「命令不存在」（测不出，
//! 只能展示）与「命令跑完、清单是空的」（真的一个都没有）。交给 `Command`
//! 去查的话，这两种都只会表现成一次启动失败。

use std::path::PathBuf;
use std::process::Command;

/// 找到 `name` 对应的可执行文件。
pub fn resolve_tool_program(name: &str) -> Option<PathBuf> {
    let direct = std::path::Path::new(name);
    if direct.is_absolute() {
        return direct.is_file().then(|| direct.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// 为一条工具链命令构造 `Command`；找不到程序时返回 `None`。
pub fn tool_command(name: &str, args: &[&str]) -> Option<Command> {
    resolve_tool_program(name)?;
    let mut command = Command::new(name);
    command.args(args);
    Some(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_system_command_resolves_through_path() {
        let resolved = resolve_tool_program("sh").expect("sh 必须在 PATH 上");
        assert!(resolved.is_absolute());
    }

    #[test]
    fn an_unknown_command_resolves_to_nothing() {
        assert_eq!(resolve_tool_program("qc-definitely-not-a-command"), None);
    }

    #[test]
    fn an_absolute_path_is_taken_at_face_value() {
        let file = std::env::temp_dir().join("qc-tool-command-probe");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(
            resolve_tool_program(&file.to_string_lossy()),
            Some(file.clone())
        );
        let _ = std::fs::remove_file(&file);
    }
}
