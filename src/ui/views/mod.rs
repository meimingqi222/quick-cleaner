//! 页面视图模块总装

pub mod apps;
mod apps_components;
pub mod dashboard;
pub mod declutter;
pub mod disk;
mod disk_breakdown;
mod disk_components;
mod disk_left;
mod disk_right;
mod disk_volume;
pub mod junk;
pub mod search;
pub mod status;

pub use apps::{render_apps_context_menu, render_apps_view};
pub use dashboard::render_dashboard_view;
pub use declutter::{
    render_declutter_context_menu, render_declutter_view, DeclutterContextMenu, DeclutterState,
    DeclutterTab,
};
pub use disk::{render_disk_view, DiskTab};
pub use disk_right::render_disk_clean_bar;
pub use disk_volume::render_disk_volume_dropdown;
pub use junk::{render_clean_bar, render_junk_view};
pub use search::render_search_view;
pub use status::render_status_view;
