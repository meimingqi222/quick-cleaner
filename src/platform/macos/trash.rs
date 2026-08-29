//! macOS 废纸篓：清空，以及把单个条目安全地移进去。

use crate::core::cleaner::{clean_dir_contents, CleanProgress, CleanReport};
use objc::runtime::{Object, BOOL, NO};
use objc::{class, msg_send, sel, sel_impl};
use std::path::Path;

/// 这个路径是不是**本机废纸篓 `~/.Trash` 本身**。
///
/// 必须精确匹配,不能用 `contains(".Trash")` 之类的子串判断:外接卷的废纸篓是
/// `/Volumes/<卷>/.Trashes/<uid>`,路径里同样含 ".Trash",但 [`empty_trash`]
/// 清的是 `~/.Trash`——子串匹配会清掉用户没勾的本机废纸篓,还让外接卷自己
/// 那份一个字节都没删。外接卷废纸篓走普通的「清空目录内容」路径即可。
pub fn is_system_trash(path: &Path) -> bool {
    super::user_env::user_home().is_some_and(|home| path == home.join(".Trash"))
}

pub fn empty_trash(p: &CleanProgress) -> CleanReport {
    if let Some(home) = super::user_env::user_home() {
        let trash = home.join(".Trash");
        if trash.exists() {
            return clean_dir_contents(&trash, p);
        }
    }
    CleanReport::default()
}

/// 把 `path` 移入废纸篓，语义与在访达里按删除键一致。
///
/// 不能用 `fs::rename` 到 `~/.Trash` 代替，那样有三个问题：跨卷会 `EXDEV`
/// 失败（外接盘上的 App）、丢失「放回原处」信息、同名文件会直接互相覆盖。
/// `NSFileManager.trashItemAtURL:` 三个问题都不存在。
///
/// 注意这走的是当前进程的权限：`/Applications` 下 root 属主的 App 仍可能
/// `EPERM`，需要上层提示用户授权，而不是在这里静默失败。
pub fn move_to_trash(path: &Path) -> Result<(), String> {
    match move_to_trash_with_file_manager(path) {
        Ok(()) => Ok(()),
        Err(file_manager_error) => move_to_trash_with_finder(path).map_err(|finder_error| {
            format!("{file_manager_error}；Finder 回退也失败：{finder_error}")
        }),
    }
}

fn move_to_trash_with_file_manager(path: &Path) -> Result<(), String> {
    let s = path.to_str().ok_or("路径不是合法 UTF-8")?;

    // SAFETY: 下面全是标准 Cocoa 调用。每个对象的生命周期都在本函数内闭合：
    // 自己 alloc 的 NSString 显式 release，其余是 autoreleased 对象，由本函数
    // 自建的 autorelease pool 兜底（后台线程上没有现成的 pool）。
    unsafe {
        let pool: *mut Object = msg_send![class!(NSAutoreleasePool), new];

        let ns_path: *mut Object = msg_send![class!(NSString), alloc];
        // 4 = NSUTF8StringEncoding
        let ns_path: *mut Object = msg_send![
            ns_path,
            initWithBytes: s.as_ptr() as *const std::ffi::c_void
            length: s.len()
            encoding: 4usize
        ];
        if ns_path.is_null() {
            let _: () = msg_send![pool, drain];
            return Err("NSString 创建失败".into());
        }

        let url: *mut Object = msg_send![class!(NSURL), fileURLWithPath: ns_path];
        if url.is_null() {
            let _: () = msg_send![ns_path, release];
            let _: () = msg_send![pool, drain];
            return Err("NSURL 创建失败".into());
        }

        let fm: *mut Object = msg_send![class!(NSFileManager), defaultManager];
        let mut err: *mut Object = std::ptr::null_mut();
        // resultingItemURL 传 null：我们不关心它落在废纸篓里的最终名字。
        let ok: BOOL = msg_send![
            fm,
            trashItemAtURL: url
            resultingItemURL: std::ptr::null_mut::<*mut Object>()
            error: &mut err
        ];

        let result = if ok == NO {
            Err(if err.is_null() {
                "移入废纸篓失败".to_string()
            } else {
                let desc: *mut Object = msg_send![err, localizedDescription];
                nsstring_to_string(desc).unwrap_or_else(|| "移入废纸篓失败".into())
            })
        } else {
            Ok(())
        };

        let _: () = msg_send![ns_path, release];
        let _: () = msg_send![pool, drain];
        result
    }
}

/// `NSFileManager` 可能因 App Management/TCC 权限拒绝 `/Applications` 下的
/// bundle。与 Mole 的卸载策略一致，回退到 Finder 执行“移到废纸篓”，让
/// macOS 走自己的授权流程。路径通过 argv 传入，不拼进 AppleScript。
fn move_to_trash_with_finder(path: &Path) -> Result<(), String> {
    let script = r#"on run argv
set targetPath to POSIX file (item 1 of argv)
tell application "Finder" to delete targetPath
end run"#;
    let mut child = std::process::Command::new("osascript")
        .arg("-")
        .arg(path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("无法启动 Finder：{error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        if let Err(error) = stdin.write_all(script.as_bytes()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("无法调用 Finder：{error}"));
        }
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Finder 授权或移除操作超时".into());
            }
            Err(error) => return Err(format!("等待 Finder 失败：{error}")),
        }
    };

    let mut reason = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        use std::io::Read;
        let _ = stderr.read_to_string(&mut reason);
    }

    if status.success() && !path.exists() {
        Ok(())
    } else {
        let reason = reason.trim();
        Err(if reason.is_empty() {
            "Finder 未移除目标".into()
        } else {
            reason.to_string()
        })
    }
}

/// 读 `NSString` 的 UTF-8 内容。`s` 为 null 或非法 UTF-8 时返回 `None`。
///
/// # Safety
/// `s` 必须为 null 或指向存活的 `NSString`。
unsafe fn nsstring_to_string(s: *mut Object) -> Option<String> {
    if s.is_null() {
        return None;
    }
    let utf8: *const std::ffi::c_char = msg_send![s, UTF8String];
    if utf8.is_null() {
        return None;
    }
    std::ffi::CStr::from_ptr(utf8)
        .to_str()
        .ok()
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 外接卷废纸篓的路径同样含 ".Trash"，绝不能被当成本机废纸篓：
    /// 那会清掉用户没勾的 `~/.Trash`，且外接卷自己那份一个字节都不删。
    #[test]
    fn only_home_trash_is_system_trash() {
        let home = dirs::home_dir().expect("测试环境必须有 home");
        assert!(is_system_trash(&home.join(".Trash")));

        for other in [
            std::path::PathBuf::from("/Volumes/外接盘/.Trashes/501"),
            std::path::PathBuf::from("/Volumes/Backup/.Trashes"),
            home.join(".TrashOld"),
            home.join("Documents/Docs.Trash"),
            home.join(".Trash/子目录"),
        ] {
            assert!(
                !is_system_trash(&other),
                "{} 不该被当成本机废纸篓",
                other.display()
            );
        }
    }

    /// 真的往废纸篓里放一个临时文件再捞出来核对。
    ///
    /// 会在用户的废纸篓里留下痕迹，所以默认不跑：
    /// `cargo test -- --ignored move_to_trash_relocates_the_file`
    #[test]
    #[ignore]
    fn move_to_trash_relocates_the_file() {
        let name = format!("quick-cleaner-trash-test-{}", std::process::id());
        let src = std::env::temp_dir().join(&name);
        std::fs::write(&src, b"quick-cleaner").expect("建临时文件失败");

        move_to_trash(&src).expect("移入废纸篓失败");
        assert!(!src.exists(), "源文件还在原地");

        let trashed = dirs::home_dir().unwrap().join(".Trash").join(&name);
        assert!(trashed.exists(), "废纸篓里找不到 {name}");
        let _ = std::fs::remove_file(&trashed);
    }
}
