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
//! | `detect_system_language` | 系统显示语言（首次启动的默认界面语言） |
//! | `list_volumes` | 可供深度分析的卷 |
//! | `scan_volume` | 卷的整树空间分析 |
//! | `get_volume_space` | 卷的总容量 / 可用容量 |
//! | `list_installed_apps` | 已安装软件枚举 |
//! | `run_uninstaller_and_wait` | 调用官方卸载程序并等待退出 |
//! | `scan_residuals` / `verify_residuals` / `clean_residuals` | 卸载残留的采集、复核与清理 |
//! | `reveal_in_explorer` | 在系统文件管理器中定位路径 |
//! | `move_to_trash` | 把单个路径移入回收站/废纸篓（可还原） |
//! | `is_system_trash` / `empty_trash` | 识别系统回收站/废纸篓目录，以及清空它 |

/// 编译期校验：当前平台分支确实提供了门面要求的全部函数，且签名一致。
///
/// 只是把每个函数名按期望类型取一次函数指针，不产生任何运行时开销。
macro_rules! platform_contract {
    () => {
        const _: () = {
            use crate::core::apps::{InstalledApp, ResidualItem, ResidualScanResult};
            use crate::core::cleaner::{CleanProgress, CleanReport};
            use crate::core::disk::{ScanError, ScanResult, VolumeId};
            use crate::core::i18n::Language;
            use std::path::Path;
            use std::sync::atomic::AtomicBool;

            let _: fn() -> bool = is_elevated;
            let _: fn() -> Language = detect_system_language;
            let _: fn() -> bool = relaunch_as_admin_if_needed;
            let _: fn() -> Vec<VolumeId> = list_volumes;
            let _: fn(&VolumeId, usize) -> Result<ScanResult, ScanError> = scan_volume;
            let _: fn(&VolumeId) -> Option<(u64, u64)> = get_volume_space;
            let _: fn(&AtomicBool) -> Vec<InstalledApp> = list_installed_apps;
            let _: fn(&InstalledApp) -> Result<(), String> = run_uninstaller_and_wait;
            let _: fn(&InstalledApp) -> ResidualScanResult = scan_residuals;
            // 残留扫描的进程占用探测：macOS 上活库删除失败的原因用户看不
            // 懂（闸门拒的，不是系统报错），必须在扫描时给出证据。Windows
            // 文件锁自带系统错误原因，占位实现即可。
            let _: fn(&InstalledApp) -> crate::core::apps::ResidualOccupancy = detect_occupancy;
            let _: fn(&[ResidualItem], &CleanProgress) -> CleanReport = clean_residuals;
            let _: fn(Vec<ResidualItem>) -> Vec<ResidualItem> = verify_residuals;
            let _: fn(&Path) = reveal_in_explorer;
            // 「送回收站」以前不在契约里：两个平台各有实现，但 core 的
            // recycle_path 只 cfg 到了 Windows 那份，非 Windows 分支直接退化成
            // 永久删除——用户勾了「删除到回收站」反而拿到不可撤销的删除。
            // 进契约后各平台漏实现会直接编译失败。
            let _: fn(&Path) -> Result<(), String> = move_to_trash;
            // 「识别回收站目录」和「清空回收站」以前也没进契约，core 只能
            // 靠 `#[cfg]` 分别直连 windows::recycle 和 macos::trash，两条分支
            // 的形状还不一样。收进契约后 `clean_targets` 里那段平台分支整个消失。
            let _: fn(&Path) -> bool = is_system_trash;
            let _: fn(&CleanProgress) -> CleanReport = empty_trash;
            let _: fn(&Path) = open_in_default_app;
        };
    };
}

/// 类 Unix 系统上的语言标记：按 POSIX 优先级读 `LC_ALL` → `LC_MESSAGES` → `LANG`。
///
/// 返回 `en_US.UTF-8`、`zh_CN.UTF-8` 这类原始串，交给
/// [`Language::from_locale_tag`](crate::core::i18n::Language::from_locale_tag) 判定。
/// 一个都没设时返回空串，落到英文。
#[cfg(not(windows))]
fn posix_locale_tag() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty())
        .unwrap_or_default()
}

#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
/// 门面对外只开放**契约里那 16 个函数**，外加两个跨平台通用的图标读取。
///
/// 以前这里是 `pub use windows::*`：平台内部的每个 `pub` 符号都被透传出去，
/// 于是 `crate::platform::real_user_home()` 这类只有 Windows 才有的东西，在
/// Windows 上编得过、在 macOS 上直接编不过——而契约宏只校验它列出的那些函数，
/// 拦不住这种「看起来像门面、其实是单平台」的调用。显式列表把它挡在编译期：
/// 确实需要单平台能力时，得写 `platform::windows::...` 并自己加 `#[cfg]`，
/// 一眼能看出这是平台分支而不是通用接口。
pub use windows::{
    app_icon_from_bundle, app_icon_png, clean_residuals, detect_occupancy, detect_system_language,
    empty_trash, get_volume_space, is_elevated, is_system_trash, list_installed_apps, list_volumes,
    move_to_trash, open_in_default_app, relaunch_as_admin_if_needed, reveal_in_explorer,
    run_uninstaller_and_wait, scan_residuals, scan_volume, verify_residuals,
};
#[cfg(windows)]
platform_contract!();

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos::{
    app_icon_from_bundle, app_icon_png, clean_residuals, detect_occupancy, detect_system_language,
    empty_trash, get_volume_space, is_elevated, is_system_trash, list_installed_apps, list_volumes,
    move_to_trash, open_in_default_app, relaunch_as_admin_if_needed, reveal_in_explorer,
    run_uninstaller_and_wait, scan_residuals, scan_volume, verify_residuals,
};
#[cfg(target_os = "macos")]
platform_contract!();

/// 既不是 Windows 也不是 macOS 时的兜底实现：编译得过，但什么都不做。
#[cfg(all(not(windows), not(target_os = "macos")))]
pub mod fallback {
    use crate::core::apps::{InstalledApp, ResidualItem, ResidualScanResult};
    use crate::core::cleaner::{CleanProgress, CleanReport};
    use crate::core::disk::{ScanError, ScanResult, VolumeId};
    use crate::core::i18n::Language;
    use std::path::Path;
    use std::sync::atomic::AtomicBool;

    pub fn is_elevated() -> bool {
        false
    }

    /// 按 POSIX 环境变量推断界面语言。
    pub fn detect_system_language() -> Language {
        Language::from_locale_tag(&crate::platform::posix_locale_tag())
    }

    pub fn relaunch_as_admin_if_needed() -> bool {
        false
    }

    pub fn list_volumes() -> Vec<VolumeId> {
        Vec::new()
    }

    /// 整树空间分析依赖 NTFS 的 `$MFT`，其它平台没有等价物。
    pub fn scan_volume(_vol: &VolumeId, _top_n: usize) -> Result<ScanResult, ScanError> {
        Err(ScanError::NotNtfs)
    }

    pub fn get_volume_space(_vol: &VolumeId) -> Option<(u64, u64)> {
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
            app_id: app.id.clone(),
            ..Default::default()
        }
    }

    pub fn clean_residuals(_items: &[ResidualItem], _prog: &CleanProgress) -> CleanReport {
        CleanReport::default()
    }

    pub fn detect_occupancy(_app: &InstalledApp) -> crate::core::apps::ResidualOccupancy {
        crate::core::apps::ResidualOccupancy::default()
    }

    pub fn verify_residuals(items: Vec<ResidualItem>) -> Vec<ResidualItem> {
        items
    }

    pub fn reveal_in_explorer(_path: &Path) {}

    /// 没有回收站就**如实报错**，绝不退化成永久删除：调用方开这个开关
    /// 就是要「删错了能捞回来」，静默替他抹掉是把安全网抽走。
    pub fn move_to_trash(_path: &Path) -> Result<(), String> {
        Err("当前平台没有回收站".into())
    }

    /// 没有回收站，也就没有「回收站目录」这个概念。
    pub fn is_system_trash(_path: &Path) -> bool {
        false
    }

    pub fn empty_trash(_prog: &CleanProgress) -> CleanReport {
        CleanReport::default()
    }

    pub fn open_in_default_app(_path: &Path) {}
}

#[cfg(all(not(windows), not(target_os = "macos")))]
// fallback 不含 `app_icon_*`：UI 侧对这个 target 已经用 `#[cfg]` 关掉了图标加载。
pub use fallback::{
    clean_residuals, detect_system_language, empty_trash, get_volume_space, is_elevated,
    is_system_trash, list_installed_apps, list_volumes, move_to_trash, open_in_default_app,
    relaunch_as_admin_if_needed, reveal_in_explorer, run_uninstaller_and_wait, scan_residuals,
    scan_volume, verify_residuals,
};
#[cfg(all(not(windows), not(target_os = "macos")))]
platform_contract!();
