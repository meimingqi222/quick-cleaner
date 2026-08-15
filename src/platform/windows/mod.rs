//! Windows 平台专用功能总装

pub mod apps;
pub mod mft;
pub mod recycle;
pub mod registry;
pub mod residuals;
pub mod security;
pub mod volume;

pub use apps::{dir_or_file_size, list_installed_apps, reveal_in_explorer, run_uninstaller_and_wait};
pub use mft::{scan_volume, MftError, MftTree};
pub use volume::{get_volume_space, list_ntfs_volumes};
pub use recycle::{empty_recycle_bin, is_recycle_bin, is_recycle_junk_entry, sweep_orphaned_recycle};
pub use residuals::{clean_residuals, scan_residuals};
pub use security::{current_user_sid, is_elevated};
