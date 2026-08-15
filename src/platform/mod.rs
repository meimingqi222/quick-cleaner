//! 操作系统适配层统一门面
//!
//! 上层（`ui`）只认这一层的函数签名，具体实现按目标平台切换。
//!
//! 下面这份清单就是**契约**：每个平台分支都必须原样提供这些函数，
//! 少一个都会在该平台上编译失败。历史上 macOS 分支导出的是
//! `list_volumes` 而门面要的是 `list_ntfs_volumes`，又没有 `scan_volume`
//! 和 `reveal_in_explorer`，于是「跨平台」只存在于目录结构里——任何
//! 非 Windows 目标都编不过。`platform_contract!` 用来防止这种漂移再次发生。
//!
//! | 函数 | 用途 |
//! | --- | --- |
//! | `is_elevated` | 当前进程是否已提权 |
//! | `list_ntfs_volumes` | 可供深度分析的卷 |
//! | `scan_volume` | 卷的整树空间分析 |
//! | `get_volume_space` | 卷的总容量 / 可用容量 |
//! | `list_installed_apps` | 已安装软件枚举 |
//! | `run_uninstaller_and_wait` | 调用官方卸载程序并等待退出 |
//! | `scan_residuals` / `clean_residuals` | 卸载残留的扫描与清理 |
//! | `reveal_in_explorer` | 在系统文件管理器中定位路径 |

/// 编译期校验：当前平台分支确实提供了门面要求的全部函数，且签名一致。
///
/// 只是把每个函数名按期望类型取一次函数指针，不产生任何运行时开销。
macro_rules! platform_contract {
    () => {
        const _: () = {
            use crate::core::apps::{InstalledApp, ResidualKind, ResidualScanResult};
            use crate::core::cleaner::{CleanProgress, CleanReport};
            use crate::core::disk::{MftError, MftScan};
            use std::path::Path;
            use std::sync::atomic::AtomicBool;

            let _: fn() -> bool = is_elevated;
            let _: fn() -> Vec<char> = list_ntfs_volumes;
            let _: fn(char, usize) -> Result<MftScan, MftError> = scan_volume;
            let _: fn(char) -> Option<(u64, u64)> = get_volume_space;
            let _: fn(&AtomicBool) -> Vec<InstalledApp> = list_installed_apps;
            let _: fn(&InstalledApp) -> Result<(), String> = run_uninstaller_and_wait;
            let _: fn(&InstalledApp) -> ResidualScanResult = scan_residuals;
            let _: fn(&[ResidualKind], &CleanProgress) -> CleanReport = clean_residuals;
            let _: fn(&Path) = reveal_in_explorer;
        };
    };
}

#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
pub use windows::*;
#[cfg(windows)]
platform_contract!();

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;
#[cfg(target_os = "macos")]
platform_contract!();

/// 既不是 Windows 也不是 macOS 时的兜底实现：编译得过，但什么都不做。
#[cfg(all(not(windows), not(target_os = "macos")))]
pub mod fallback {
    use crate::core::apps::{InstalledApp, ResidualKind, ResidualScanResult};
    use crate::core::cleaner::{CleanProgress, CleanReport};
    use crate::core::disk::{MftError, MftScan};
    use std::path::Path;
    use std::sync::atomic::AtomicBool;

    pub fn is_elevated() -> bool {
        false
    }

    pub fn list_ntfs_volumes() -> Vec<char> {
        Vec::new()
    }

    /// 整树空间分析依赖 NTFS 的 `$MFT`，其它平台没有等价物。
    pub fn scan_volume(_vol: char, _top_n: usize) -> Result<MftScan, MftError> {
        Err(MftError::NotNtfs)
    }

    pub fn get_volume_space(_vol: char) -> Option<(u64, u64)> {
        None
    }

    pub fn list_installed_apps(_live: &AtomicBool) -> Vec<InstalledApp> {
        Vec::new()
    }

    pub fn run_uninstaller_and_wait(_app: &InstalledApp) -> Result<(), String> {
        Err("当前平台不支持自动卸载".into())
    }

    pub fn scan_residuals(app: &InstalledApp) -> ResidualScanResult {
        ResidualScanResult {
            app_name: app.name.clone(),
            items: Vec::new(),
            total_file_size: 0,
        }
    }

    pub fn clean_residuals(_items: &[ResidualKind], _prog: &CleanProgress) -> CleanReport {
        CleanReport::default()
    }

    pub fn reveal_in_explorer(_path: &Path) {}
}

#[cfg(all(not(windows), not(target_os = "macos")))]
pub use fallback::*;
#[cfg(all(not(windows), not(target_os = "macos")))]
platform_contract!();
