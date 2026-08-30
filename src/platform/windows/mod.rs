//! Windows 平台专用功能总装

pub mod app_icons;
pub mod apps;
pub mod gpu;
pub mod inuse;
pub mod mft;
pub mod nvml;
pub mod pdh;
pub mod process;
pub mod recycle;
pub(crate) mod registry;
pub mod residuals;
pub mod security;
pub mod status;
pub mod thermal;
pub mod user_env;
pub mod volume;
pub mod wmi;

pub use app_icons::{app_icon_from_bundle, app_icon_png};
pub use apps::{
    dir_or_file_size, list_installed_apps, open_in_default_app, reveal_in_explorer,
    run_uninstaller_and_wait,
};
pub use inuse::{detect_inuse, spot_check_inuse};
pub use mft::{scan_volume, ScanError, SizeTree};
pub use recycle::{
    empty_trash, is_recycle_junk_entry, is_system_trash, move_to_trash, sweep_orphaned_recycle,
};
pub use residuals::{clean_residuals, detect_occupancy, scan_residuals, verify_residuals};
pub use security::{current_user_sid, is_elevated, relaunch_as_admin_if_needed};
pub use status::{
    elevated_fan_control, fan_control_supported, fan_helper_installed, install_fan_helper,
    process_unique_id, read_battery, read_gpus, read_thermal, set_fan_mode, system_uptime_secs,
    terminate_process, uninstall_fan_helper,
};
pub use user_env::{
    detect_system_language, get_user_context, init_user_context, real_user_home,
    real_user_known_folders, real_user_local_appdata, real_user_roaming_appdata, real_user_sid,
    real_user_temp, user_cache_dir, user_data_dir, user_home, user_temp_dir,
};
pub use volume::{get_volume_space, list_volumes};
