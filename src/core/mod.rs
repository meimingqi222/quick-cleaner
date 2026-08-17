//! QuickCleaner 核心业务领域层
//!
//! 这一层不依赖 GPUI，也不直接调用操作系统 API（需要时经 `platform` 门面）。

pub mod apps;
pub mod categories;
pub mod cleaner;
pub mod devscan;
pub mod disk;
pub mod i18n;
pub mod log;
pub mod model;
pub mod safety;
pub mod scanner;
pub mod settings;

pub use apps::{
    AppFilterPreset, AppRegRoot, AppSortColumn, AppSortState, InstalledApp, ResidualKind,
    ResidualScanResult, SortDirection,
};
pub use categories::{all_targets, CategoryId, Safety, ScanTarget};
pub use cleaner::{
    clean_arbitrary, clean_path, clean_targets, CleanProgress, CleanReport, CleanResult,
    CleanSnapshot, Disposal,
};
pub use disk::{DirUsage, DiskSelectionState, Node, ScanResult, VolumeId};
pub use i18n::Language;
pub use model::{commas, fmt_size, fmt_size_si, truncate, Check};
pub use safety::{is_protected, is_system_root_dir};
pub use scanner::{scan_all, CategorySummary, ScanItem};
pub use settings::Settings;
