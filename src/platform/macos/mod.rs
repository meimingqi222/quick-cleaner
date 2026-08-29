//! macOS 平台总装
//!
//! 导出的函数集合必须与 `platform::mod` 的门面契约完全一致，
//! 由 `platform_contract!` 在编译期校验。

pub mod app_icons;
pub mod apps;
pub mod cache;
pub mod codesign;
pub mod disk;
pub mod disk_tree;
pub mod dock;
pub mod elevate;
pub mod fanhelper;
pub mod fsevents;
pub mod index_v7;
pub mod inuse;
pub mod iokit;
pub mod permission_drop;
pub mod plist;
pub mod residuals;
pub mod status;
pub mod tcc;
pub mod trash;
pub mod user_env;
pub mod walk;

pub use app_icons::{app_icon_from_bundle, app_icon_png};
pub use apps::{
    list_installed_apps, open_in_default_app, reveal_in_explorer, run_uninstaller_and_wait,
};
pub use disk::{
    detect_system_language, get_volume_space, is_elevated, list_volumes,
    relaunch_as_admin_if_needed, scan_volume,
};
pub use dock::set_dock_icon;
pub use fanhelper::{
    elevated_fan_control, fan_helper_installed, install_fan_helper, uninstall_fan_helper,
};
pub use inuse::{detect_inuse, spot_check_inuse};
pub use iokit::{read_battery, read_gpu};
pub use residuals::{clean_residuals, detect_occupancy, scan_residuals, verify_residuals};
pub use status::{
    process_unique_id, read_thermal, set_fan_mode, system_uptime_secs, terminate_process,
};
pub use trash::{empty_trash, is_system_trash, move_to_trash};
pub use user_env::{user_cache_dir, user_data_dir, user_home, user_temp_dir};

// TCC 渐进式增强：UI 用这些函数检测和引导完全磁盘访问授权
pub use tcc::{
    enclosing_app_bundle, has_full_disk_access, is_tcc_denied, open_full_disk_access_settings,
};
