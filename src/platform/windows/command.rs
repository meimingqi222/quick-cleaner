//! 拉起「用户工具链命令」：npm / pnpm / pipx / conda 这一类。
//!
//! 存在的理由只有一个：这些命令在 Windows 上常常**只有 `.cmd` 垫片、没有
//! `.exe`**（npm 装出来的是 `npm`、`npm.cmd`、`npm.ps1`；pnpm 是
//! `pnpm.CMD`）。
//!
//! `Command::new("npm")` 起不来，但**不是**因为 `CreateProcess` 拒绝批处理：
//! 实测给它一个带 `.cmd` 的完整路径是能跑的（它会隐式经 cmd 处理）。真正的
//! 原因是 `CreateProcess` **不查 `PATHEXT`**——它只找 `npm` 和 `npm.exe`，
//! 而 `npm.cmd` 两个都不是。所以这里补的是 `PATH` × `PATHEXT` 解析。
//!
//! 失败的表现不是报错，而是「这个生态什么都查不到」：清单命令跑不起来 →
//! 通道静默退化成「只展示、不可移除」，而且**卸载命令同样跑不起来**，等于
//! npm / pnpm / pipx 三条通道在 Windows 上整体失效。
//!
//! `.cmd` / `.bat` 仍然显式经 `cmd.exe /d /s /c` 启动，不依赖上面那个隐式
//! 处理：一是 `/d` 能关掉 `AutoRun`（注册表里的 AutoRun 命令会在我们的命令
//! 之前先跑），隐式路径给不了这个开关；二是显式一个字符串比让 std 拼命令行
//! 再交给 cmd 重解析更好推理。两条路 cmd 都会**重新解析**参数，所以参数里
//! 的元字符必须挡掉，见 [`cmd_line`]。

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 找到 `name` 对应的可执行文件。绝对路径直接判定，否则按当前 `PATH` 与
/// `PATHEXT` 查找。
pub fn resolve_tool_program(name: &str) -> Option<PathBuf> {
    let direct = Path::new(name);
    if direct.is_absolute() {
        return direct.is_file().then(|| direct.to_path_buf());
    }
    if direct.extension().is_some() {
        // 名字自带扩展名（如 `npm.cmd`）时只按它自己找，不再叠 PATHEXT。
        return resolve_in(name, &std::env::var_os("PATH")?, "");
    }
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    resolve_in(name, &std::env::var_os("PATH")?, &pathext)
}

/// `PATH` × `PATHEXT` 查找的核心，取参数而不是读环境变量。
///
/// 抽成纯函数是为了可测：改 `PATH` 是进程级的，测试并行跑时会互相干扰，
/// 而「只找 `.exe`、漏掉 `.cmd`」正是这次要修的那个缺陷。
///
/// 顺序按 `PATHEXT` 走，所以 `.EXE` 排在 `.CMD` 前面——能直接启动的优先。
/// `pathext` 为空表示名字已经带扩展名，只按原名找。
fn resolve_in(name: &str, path: &OsStr, pathext: &str) -> Option<PathBuf> {
    let extensions: Vec<String> = if pathext.is_empty() {
        vec![String::new()]
    } else {
        pathext
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(str::to_string)
            .collect()
    };
    for dir in std::env::split_paths(path) {
        for extension in &extensions {
            let candidate = if extension.is_empty() {
                dir.join(name)
            } else {
                dir.join(format!("{name}{extension}"))
            };
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 为一条工具链命令构造 `Command`；找不到程序时返回 `None`。
///
/// `.cmd` / `.bat` 经 `cmd.exe` 启动；其余直接启动。
pub fn tool_command(name: &str, args: &[&str]) -> Option<Command> {
    let program = resolve_tool_program(name)?;
    let is_script = program
        .extension()
        .map(|extension| {
            let extension = extension.to_string_lossy().to_ascii_lowercase();
            extension == "cmd" || extension == "bat"
        })
        .unwrap_or(false);

    if is_script {
        use std::os::windows::process::CommandExt;
        let line = cmd_line(&program, args)?;
        let mut command = Command::new("cmd.exe");
        // /d 关闭 AutoRun，/v:off 禁用延迟展开，/s 去掉最外层引号。
        // 完整 cmd 命令不能再经过 CRT 的反斜杠转义。
        command
            .args(["/d", "/v:off", "/s", "/c"])
            .raw_arg(format!("\"{line}\""));
        Some(command)
    } else {
        let mut command = Command::new(&program);
        command.args(args);
        Some(command)
    }
}

/// 拼出交给 `cmd.exe /c` 的那一行；参数里出现 cmd 元字符时返回 `None`。
///
/// `CreateProcess` 的引号规则**不够**用：cmd 会把这一行重新解析一遍，
/// `&`、`|`、`<`、`>`、`%` 即使在引号内也可能被 cmd 拿去展开。与其把
/// 两层转义叠在一起（多一层就多一处能写错的地方），这里直接拒绝——这些
/// 参数本来就只有包名、绝对路径与固定开关，撞上元字符说明有东西不对。
fn cmd_line(program: &Path, args: &[&str]) -> Option<String> {
    let program = program.to_str()?;
    let forbidden =
        |arg: &str| arg.contains(['"', '&', '|', '<', '>', '^', '%', '!', '\r', '\n', '\0']);
    if forbidden(program) || args.iter().any(|arg| forbidden(arg)) {
        return None;
    }
    let mut line = format!("\"{program}\"");
    for arg in args {
        line.push(' ');
        if arg.is_empty() || arg.contains([' ', '\t', '(', ')']) {
            line.push_str(&format!("\"{arg}\""));
        } else {
            line.push_str(arg);
        }
    }
    Some(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absolute_path_is_taken_at_face_value() {
        let file = std::env::temp_dir().join("qc-tool-command-probe.exe");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(
            resolve_tool_program(&file.to_string_lossy()),
            Some(file.clone())
        );
        let _ = std::fs::remove_file(&file);

        assert_eq!(
            resolve_tool_program("C:\\definitely\\not\\here\\qc.exe"),
            None
        );
    }

    /// 本机必然存在的命令：cmd.exe 自己。
    #[test]
    fn a_system_command_resolves_through_path_and_pathext() {
        let resolved = resolve_tool_program("cmd.exe").expect("cmd.exe 必须在 PATH 上");
        assert!(resolved.is_absolute());
        assert!(resolved.is_file());
    }

    #[test]
    fn an_unknown_command_resolves_to_nothing() {
        assert_eq!(resolve_tool_program("qc-definitely-not-a-command"), None);
    }

    /// 这次要修的缺陷本身：`CreateProcess` 不查 `PATHEXT`，所以只按名字与
    /// `.exe` 找会漏掉 npm / pnpm / pipx 的 `.cmd` 垫片，整条通道静默失效。
    #[test]
    fn a_command_that_only_exists_as_a_shim_is_still_found() {
        let dir = std::env::temp_dir().join("qc-pathext-probe");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("qc-shim-only.cmd"), b"@echo off\r\n").unwrap();
        let path = dir.clone().into_os_string();

        assert!(
            resolve_in("qc-shim-only", &path, ".COM;.EXE;.BAT;.CMD").is_some(),
            "只有 .cmd 垫片的命令必须能被解析到"
        );
        assert!(
            resolve_in("qc-shim-only", &path, ".EXE").is_none(),
            "只找 .exe 时会漏掉它——这正是修复前的行为"
        );
        assert!(
            resolve_in("qc-shim-only", &path, "").is_none(),
            "完全不叠扩展名同样找不到：命令名本身不是文件"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pathext_order_decides_which_candidate_wins() {
        let dir = std::env::temp_dir().join("qc-pathext-order");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("qc-both.exe"), b"x").unwrap();
        std::fs::write(dir.join("qc-both.cmd"), b"x").unwrap();
        let path = dir.clone().into_os_string();
        // 返回的路径按 PATHEXT 里的大小写拼（`.EXE`），Windows 上两者是同一个
        // 文件，所以只比较大小写无关的形式。
        let lowered = |resolved: Option<PathBuf>| {
            resolved.map(|resolved| resolved.to_string_lossy().to_ascii_lowercase())
        };
        let lower_dir = dir.to_string_lossy().to_ascii_lowercase();

        assert_eq!(
            lowered(resolve_in("qc-both", &path, ".EXE;.CMD")),
            Some(format!("{lower_dir}\\qc-both.exe")),
            "能直接启动的 .exe 优先"
        );
        assert_eq!(
            lowered(resolve_in("qc-both", &path, ".CMD;.EXE")),
            Some(format!("{lower_dir}\\qc-both.cmd")),
            "顺序完全交给 PATHEXT，不自己排优先级"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_name_that_already_has_an_extension_is_not_extended_again() {
        let dir = std::env::temp_dir().join("qc-pathext-ext");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("qc-explicit.cmd"), b"x").unwrap();

        assert_eq!(
            resolve_in("qc-explicit.cmd", &dir.clone().into_os_string(), ""),
            Some(dir.join("qc-explicit.cmd"))
        );
        assert!(
            resolve_in(
                "qc-explicit.cmd",
                &dir.clone().into_os_string(),
                ".EXE;.CMD"
            )
            .is_none(),
            "自带扩展名时不再叠 PATHEXT，否则会去找 qc-explicit.cmd.EXE"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 这正是这次要修的那件事：只有 `.cmd` 的命令也必须能启动。
    ///
    /// `cmd.cmd` 是本机 System32 下真实存在的批处理垫片形态之一；用它
    /// 走到「经 cmd.exe 启动」那条分支，确认真的能跑起来并拿到输出。
    #[test]
    fn a_script_shim_is_launched_through_cmd() {
        let dir = std::env::temp_dir().join("qc-tool-shim");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let shim = dir.join("qc-probe.cmd");
        std::fs::write(&shim, "@echo off\r\necho shim-ran %1\r\n").unwrap();

        let output = tool_command(&shim.to_string_lossy(), &["ARG"])
            .expect("垫片应能构造出命令")
            .output()
            .expect("垫片应能启动");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("shim-ran ARG"),
            "经 cmd.exe 的垫片应真的执行并拿到参数：{:?}",
            String::from_utf8_lossy(&output.stdout)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_argument_with_cmd_metacharacters_is_refused() {
        let program = Path::new("C:\\temp\\x.cmd");
        assert!(cmd_line(program, &["--global", "typescript"]).is_some());
        // 这些字符一旦进入 cmd 那一行就可能变成第二条命令。
        for hostile in ["a&b", "a|b", "a>b", "a<b", "a^b", "a%b", "a!b", "a\"b"] {
            assert!(
                cmd_line(program, &["uninstall", hostile]).is_none(),
                "{hostile} 必须被拒绝"
            );
        }
    }

    #[test]
    fn review_shim_preserves_spaces_and_trailing_backslash() {
        let dir = crate::core::testing::fixture("qc review shim spaces");
        let shim = dir.join("probe shim.cmd");
        std::fs::write(
            &shim,
            "@echo off\r\necho [%~1]\r\necho [%~2]\r\necho [%~3]\r\n",
        )
        .unwrap();
        let output = tool_command(
            &shim.to_string_lossy(),
            &["two words", "C:\\some dir\\", ""],
        )
        .unwrap()
        .output()
        .unwrap();
        let _ = std::fs::remove_dir_all(dir);
        assert!(output.status.success(), "{:?}", output);
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
            "[two words]\n[C:\\some dir\\]\n[]\n"
        );
    }
}
