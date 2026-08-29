//! 完全磁盘访问（Full Disk Access）的探测与引导
//!
//! macOS 的 TCC 会拦住一批目录，**即使以 root 运行也拦**——这一点和 Windows
//! 的「提权就能读」完全不同，`is_elevated()` 在这里没有参考价值。
//!
//! 实测（本机 macOS 15）：未授权时 `~/Library` 下有 146 个目录被拦，
//! 但**全部是 Apple 自家的数据**（Mail、Safari、Messages、Photos、HomeKit、
//! Reminders、各类 `com.apple.*` / `group.com.apple.*` 容器）。第三方应用的
//! 缓存与容器一个都没被拦。所以未授权时本工具**不会功能归零**，只是少扫
//! 一部分系统自带应用的数据——影响面最大的是 Safari 缓存。
//!
//! 另一个关键点：TCC 授权授给的是**责任进程**。从终端跑 `cargo run` 时责任
//! 进程是终端本身，所以开发机上「能读」不代表打包签名后的 .app 也能读——
//! .app 必须自己出现在授权列表里。

use std::path::{Path, PathBuf};

/// TCC 拦截返回 `EPERM`（Operation not permitted），而不是普通权限不足的
/// `EACCES`（Permission denied）。区分开才能给出正确的提示：前者要去系统设置
/// 里授权，后者是文件属主/模式的问题，授权也没用。
pub fn is_tcc_denied(err: &std::io::Error) -> bool {
    err.raw_os_error() == Some(libc::EPERM)
}

/// 当前进程是否已获得完全磁盘访问。
///
/// 逐个探测只有 FDA 才读得到的位置，任意一个读成功即视为已授权。
///
/// 注意不能用 `metadata()` 探测：TCC 只拦 `open` / `readdir`，`stat` 照样成功，
/// 用 `exists()` 判断会得到「看得见但打不开」的假阳性。
pub fn has_full_disk_access() -> bool {
    let Some(home) = super::user_env::user_home() else {
        return false;
    };

    // TCC.db 是最标准的探针；后两个是它不存在时的兜底。
    let probes = [
        home.join("Library/Application Support/com.apple.TCC/TCC.db"),
        home.join("Library/Mail"),
        home.join("Library/Safari"),
    ];

    for probe in &probes {
        match try_read(probe) {
            // 探针不存在，换下一个——不能据此判定有没有权限。
            None => continue,
            Some(readable) => return readable,
        }
    }
    // 一个探针都不存在（极少见）。保守报未授权，宁可多提示一次。
    false
}

/// 尝试真正打开 `path`。`None` 表示路径不存在，无从判断。
fn try_read(path: &Path) -> Option<bool> {
    let result = if path.is_dir() {
        std::fs::read_dir(path).map(|_| ())
    } else {
        std::fs::File::open(path).map(|_| ())
    };

    match result {
        Ok(()) => Some(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => Some(false),
    }
}

/// 打开「系统设置 → 隐私与安全性 → 完全磁盘访问」。
///
/// 用户仍需自己把本应用拖进列表并打开开关——系统不允许程序代劳。
pub fn open_full_disk_access_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
        .spawn();
}

/// 当前可执行文件所在的 .app bundle 路径（用于提示用户该把哪个拖进授权列表）。
///
/// 从终端直接跑二进制时没有 bundle，返回 `None`——此时责任进程是终端，
/// 该授权的是终端而不是本程序。
pub fn enclosing_app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // .../Foo.app/Contents/MacOS/foo → .../Foo.app
    let bundle = exe.parent()?.parent()?.parent()?;
    (bundle.extension()? == "app").then(|| bundle.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 探测本身不能崩，且要和「直接读 TCC.db」的结果一致。
    #[test]
    fn detection_agrees_with_a_direct_read() {
        let detected = has_full_disk_access();

        let direct = dirs::home_dir()
            .map(|h| h.join("Library/Application Support/com.apple.TCC/TCC.db"))
            .filter(|p| p.is_file() || p.symlink_metadata().is_ok())
            .map(|p| std::fs::File::open(&p).is_ok());

        if let Some(direct) = direct {
            assert_eq!(detected, direct, "探测结果与直接读 TCC.db 不一致");
        }
    }

    /// 人工核对授权状态用，结果依赖运行环境，默认不跑：
    /// `cargo test --lib -- --ignored --nocapture full_disk_access_status`
    #[test]
    #[ignore]
    fn full_disk_access_status() {
        eprintln!("完全磁盘访问: {}", has_full_disk_access());
        eprintln!(
            "所在 .app bundle: {:?}（None = 直接跑的二进制，责任进程是终端）",
            enclosing_app_bundle()
        );
        let home = dirs::home_dir().unwrap();
        for probe in [
            "Library/Mail",
            "Library/Safari",
            "Library/Caches/com.apple.Safari",
        ] {
            let p = home.join(probe);
            let state = match std::fs::read_dir(&p) {
                Ok(_) => "可读".to_string(),
                Err(e) if is_tcc_denied(&e) => "被 TCC 拦截 (EPERM)".to_string(),
                Err(e) => format!("{e}"),
            };
            eprintln!("  {probe}: {state}");
        }
    }

    /// EPERM 要能和 EACCES 区分开，否则提示会指错方向。
    #[test]
    fn tcc_denial_is_distinguished_from_plain_permission_error() {
        let eperm = std::io::Error::from_raw_os_error(libc::EPERM);
        let eacces = std::io::Error::from_raw_os_error(libc::EACCES);
        assert!(is_tcc_denied(&eperm));
        assert!(!is_tcc_denied(&eacces));
    }
}
