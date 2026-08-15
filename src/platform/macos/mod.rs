//! macOS 平台总装
//!
//! 导出的函数集合必须与 `platform::mod` 的门面契约完全一致，
//! 由 `platform_contract!` 在编译期校验。

pub mod apps;
pub mod disk;
pub mod residuals;
pub mod trash;

pub use apps::{list_installed_apps, reveal_in_explorer, run_uninstaller_and_wait};
pub use disk::{get_volume_space, is_elevated, list_ntfs_volumes, scan_volume};
pub use residuals::{clean_residuals, scan_residuals, verify_residuals};
pub use trash::empty_trash;
