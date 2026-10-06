//! QuickCleaner 核心业务领域层
//!
//! 这一层不依赖 GPUI，也不直接调用操作系统 API（需要时经 `platform` 门面）。

pub mod apps;
pub mod brew;
pub mod categories;
pub mod cleaner;
pub mod declutter;
pub mod devscan;
pub mod disk;
mod disk_selection;
pub mod docker;
pub mod fs_query;
pub mod history;
pub mod i18n;
pub mod inuse;
pub mod log;
// 纯解析逻辑：macOS 平台代码用（`target_os = "macos"`），测试构建里也编译，
// 从而其单元测试在 Windows 上也能跑。Windows 非测试构建不编译，避免 dead_code。
#[cfg(any(test, target_os = "macos"))]
pub(crate) mod macos_text;
pub mod model;
pub mod owner;
pub mod proc;
pub mod rules;
pub mod safety;
pub mod scanner;
pub mod settings;
pub mod status;
pub(crate) mod testing;
pub mod updater;
pub mod whitelist;
pub mod worktrees;

pub use fs_query::{FSIndexEngine, FileIndexQuery, IndexedFile, QueryFilter};

pub use apps::{
    AppFilterPreset, AppRegRoot, AppSortColumn, AppSortState, InstalledApp, ResidualKind,
    ResidualScanResult, SortDirection,
};
pub use categories::{all_targets, CategoryId, Safety, ScanTarget};
pub use cleaner::{
    clean_arbitrary, clean_arbitrary_items, clean_path, clean_targets, ArbitraryTarget,
    CleanProgress, CleanReport, CleanResult, CleanSnapshot, Disposal,
};
pub use disk::{DirUsage, DiskSelectionState, Node, ScanResult, VolumeId};
pub use i18n::Language;
pub use model::{commas, fmt_size, truncate, Check};
pub use safety::{is_protected, is_system_root_dir};
pub use scanner::{scan_all, CategorySummary, ScanItem};
pub use settings::Settings;
