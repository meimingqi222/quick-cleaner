//! 页面视图模块总装

pub mod apps;
mod apps_components;
pub mod dashboard;
pub mod disk;
mod disk_components;
pub mod junk;

pub use apps::{render_apps_context_menu, render_apps_view};
pub use dashboard::render_dashboard_view;
pub use disk::{render_disk_clean_bar, render_disk_view, render_disk_volume_dropdown, DiskTab};
pub use junk::{render_clean_bar, render_junk_view};
