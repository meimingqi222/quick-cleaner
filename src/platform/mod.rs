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
//! | `user_home` / `user_cache_dir` / `user_data_dir` / `user_temp_dir` | 真实用户的常用目录 |
//! | `read_thermal` | 风扇转速与 CPU 温度（拿不到的平台如实返回空） |
//! | `read_gpus` | 每张 GPU 的利用率（拿不到的平台如实返回空表） |
//! | `read_battery` | 电池电量 / 循环次数 / 健康度（无电池设备返回 None） |
//! | `system_uptime_secs` | 系统开机以来的秒数 |
//! | `terminate_process` | 请求结束一个进程（状态监控页的「结束进程」） |
//! | `process_unique_id` | 平台进程身份（Darwin uniqueid；Windows 无对等概念） |
//! | `fan_control_supported` | 这个平台能不能改风扇档位（Windows 只读转速） |
//! | `set_fan_mode` | 风扇控制档位（自动 / 全速） |
//! | `elevated_fan_control` | 经常驻特权守护进程设定档位（直写被固件拒绝时） |
//! | `fan_helper_installed` | 特权守护进程是否已安装 |
//! | `install_fan_helper` / `uninstall_fan_helper` | 安装/卸载特权守护进程（各弹一次授权框，正文由调用方按语言传入） |

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
            use crate::core::inuse::{Busy, SpotCheck};
            use crate::core::status::{FanError, FanMode, ThermalReading};
            use std::collections::HashMap;
            use std::path::Path;
            use std::path::PathBuf;
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
            let _: fn(&[PathBuf]) -> HashMap<PathBuf, Busy> = detect_inuse;
            let _: fn(&[PathBuf]) -> HashMap<PathBuf, SpotCheck> = spot_check_inuse;
            let _: fn() -> Option<PathBuf> = user_home;
            let _: fn() -> Option<PathBuf> = user_cache_dir;
            let _: fn() -> Option<PathBuf> = user_data_dir;
            // Windows 跨账户提权下主目录可能不可信，Temp 也随之拿不到；
            // 拿不到必须返回 None 跳过目标，不许换一个目录顶替。
            let _: fn() -> Option<PathBuf> = user_temp_dir;
            let _: fn() -> ThermalReading = read_thermal;
            let _: fn() -> Vec<crate::core::status::GpuReading> = read_gpus;
            let _: fn() -> Option<crate::core::status::BatteryReading> = read_battery;
            let _: fn() -> u64 = system_uptime_secs;
            let _: fn(u32, u64, Option<u64>) -> Result<(), String> = terminate_process;
            let _: fn(u32) -> Option<u64> = process_unique_id;
            let _: fn() -> bool = fan_control_supported;
            let _: fn(FanMode) -> Result<(), FanError> = set_fan_mode;
            let _: fn(FanMode) -> Result<(), FanError> = elevated_fan_control;
            let _: fn() -> bool = fan_helper_installed;
            let _: fn(&str) -> Result<(), FanError> = install_fan_helper;
            let _: fn(&str) -> Result<(), FanError> = uninstall_fan_helper;
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

/// Windows 使用的删除前复检（macOS 有句柄级占用探测，不走这里）。
///
/// 只对文件保留活 SQLite 家族闸门；目录不在这里扩大判定范围，嵌套活库由
/// 删除入口逐项检查。与 macOS 的删除闸门保持同一套安全规则，避免漂移。
#[cfg(any(not(target_os = "macos"), test))]
pub(crate) fn spot_check_without_handle_probe(
    paths: &[std::path::PathBuf],
) -> std::collections::HashMap<std::path::PathBuf, crate::core::inuse::SpotCheck> {
    use crate::core::inuse::SpotCheck;

    paths
        .iter()
        .map(|path| {
            let status = match std::fs::symlink_metadata(path) {
                Err(_) => SpotCheck::Clear,
                Ok(metadata)
                    if metadata.is_file() && crate::core::safety::is_live_database(path) =>
                {
                    SpotCheck::Busy
                }
                Ok(_) => SpotCheck::Clear,
            };
            (path.clone(), status)
        })
        .collect()
}

#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
/// 门面对外只开放**契约里的函数**，外加两个跨平台通用的图标读取。
///
/// 以前这里是 `pub use windows::*`：平台内部的每个 `pub` 符号都被透传出去，
/// 于是 `crate::platform::real_user_home()` 这类只有 Windows 才有的东西，在
/// Windows 上编得过、在 macOS 上直接编不过——而契约宏只校验它列出的那些函数，
/// 拦不住这种「看起来像门面、其实是单平台」的调用。显式列表把它挡在编译期：
/// 确实需要单平台能力时，得写 `platform::windows::...` 并自己加 `#[cfg]`，
/// 一眼能看出这是平台分支而不是通用接口。
pub use windows::{
    app_icon_from_bundle, app_icon_png, clean_residuals, detect_inuse, detect_occupancy,
    detect_system_language, elevated_fan_control, empty_trash, fan_control_supported,
    fan_helper_installed, get_volume_space, install_fan_helper, is_elevated, is_system_trash,
    list_installed_apps, list_volumes, move_to_trash, open_in_default_app, process_unique_id,
    read_battery, read_gpus, read_thermal, relaunch_as_admin_if_needed, reveal_in_explorer,
    run_uninstaller_and_wait, scan_residuals, scan_volume, set_fan_mode, spot_check_inuse,
    system_uptime_secs, terminate_process, uninstall_fan_helper, user_cache_dir, user_data_dir,
    user_home, user_temp_dir, verify_residuals,
};
#[cfg(windows)]
platform_contract!();

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos::{
    app_icon_from_bundle, app_icon_png, clean_residuals, detect_inuse, detect_occupancy,
    detect_system_language, elevated_fan_control, empty_trash, fan_control_supported,
    fan_helper_installed, get_volume_space, install_fan_helper, is_elevated, is_system_trash,
    list_installed_apps, list_volumes, move_to_trash, open_in_default_app, process_unique_id,
    read_battery, read_gpus, read_thermal, relaunch_as_admin_if_needed, reveal_in_explorer,
    run_uninstaller_and_wait, scan_residuals, scan_volume, set_fan_mode, spot_check_inuse,
    system_uptime_secs, terminate_process, uninstall_fan_helper, user_cache_dir, user_data_dir,
    user_home, user_temp_dir, verify_residuals,
};
#[cfg(target_os = "macos")]
platform_contract!();

#[cfg(test)]
mod tests {
    use super::spot_check_without_handle_probe;
    use crate::core::inuse::SpotCheck;
    use std::path::PathBuf;

    #[test]
    fn handle_probe_fallback_clears_missing_paths() {
        let paths = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        let result = spot_check_without_handle_probe(&paths);
        assert_eq!(result.get(&paths[0]), Some(&SpotCheck::Clear));
        assert_eq!(result.get(&paths[1]), Some(&SpotCheck::Clear));
    }

    #[test]
    fn handle_probe_fallback_marks_live_database_file_not_parent_dir() {
        let base = crate::core::testing::fixture("qc_spot_fallback_live_db");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let db = base.join("cache.db");
        std::fs::write(&db, b"x").unwrap();
        std::fs::write(base.join("cache.db-wal"), b"x").unwrap();

        let result = spot_check_without_handle_probe(&[base.clone(), db.clone()]);
        assert_eq!(result.get(&base), Some(&SpotCheck::Clear));
        assert_eq!(result.get(&db), Some(&SpotCheck::Busy));
        let _ = std::fs::remove_dir_all(base);
    }
}
