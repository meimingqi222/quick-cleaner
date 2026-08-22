//! 冗余整理 (Declutter) 模块化视图总装
//!
//! 包含 Bento Grid 总览、相似照片画廊、重复文件、大文件/旧文件、下载项以及右键上下文菜单。

pub mod common;
pub mod context_menu;
pub mod downloads;
pub mod duplicates;
pub mod large_files;
pub mod overview;
pub mod photos;

pub use context_menu::{render_declutter_context_menu, DeclutterContextMenu};
pub use downloads::render_downloads_tab;
pub use duplicates::render_duplicates_tab;
pub use large_files::render_large_files_tab;
pub use overview::render_overview_tab;
pub use photos::render_similar_photos_tab;

use crate::core::declutter::{DownloadItem, DuplicateGroup, LargeFileItem, PhotoGroup};
use crate::core::i18n::Language;
use crate::ui::Root;
use gpui::{AnyElement, Context, Task};

/// 冗余整理子标签页
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeclutterTab {
    Overview,
    SimilarPhotos,
    Duplicates,
    LargeFiles,
    Downloads,
}

impl DeclutterTab {
    pub fn title_lang(&self, lang: Language) -> &'static str {
        match (self, lang) {
            (DeclutterTab::Overview, Language::Zh) => "概览",
            (DeclutterTab::Overview, Language::En) => "Overview",
            (DeclutterTab::SimilarPhotos, Language::Zh) => "相似图片",
            (DeclutterTab::SimilarPhotos, Language::En) => "Similar Photos",
            (DeclutterTab::Duplicates, Language::Zh) => "重复文件",
            (DeclutterTab::Duplicates, Language::En) => "Duplicates",
            (DeclutterTab::LargeFiles, Language::Zh) => "大型与旧文件",
            (DeclutterTab::LargeFiles, Language::En) => "Large & Old Files",
            (DeclutterTab::Downloads, Language::Zh) => "下载项",
            (DeclutterTab::Downloads, Language::En) => "Downloads",
        }
    }
}

pub struct DeclutterState {
    pub tab: DeclutterTab,
    pub scanning: bool,
    pub scanned: bool,
    /// 上次扫描耗时（秒），未扫描时为 None。
    pub scan_elapsed_secs: Option<f64>,
    pub photo_groups: Vec<PhotoGroup>,
    pub duplicate_groups: Vec<DuplicateGroup>,
    pub large_files: Vec<LargeFileItem>,
    pub download_items: Vec<DownloadItem>,
    // 筛选条件
    pub min_size_filter: u64,
    pub age_filter_months: u32,
    pub kind_filter: Option<usize>,
    pub context_menu: Option<DeclutterContextMenu>,
    pub expanded_photo_groups: std::collections::HashSet<usize>,
    /// 正在后台执行删除。四个页签的「清理所选项」以前在 on_click 里同步
    /// 删文件，几百个条目就足以把 UI 线程卡到无响应；现在走后台任务，
    /// 这个标志用来挡住任务未完成时的重复点击。
    pub cleaning: bool,
    /// 删除任务独占的句柄，与扫描任务分开持有，两者不会互相顶掉。
    pub clean_task: Option<Task<()>>,
}

impl Default for DeclutterState {
    fn default() -> Self {
        Self {
            tab: DeclutterTab::Overview,
            scanning: false,
            scanned: false,
            scan_elapsed_secs: None,
            photo_groups: Vec::new(),
            duplicate_groups: Vec::new(),
            large_files: Vec::new(),
            download_items: Vec::new(),
            min_size_filter: 100_000_000,
            age_filter_months: 3,
            kind_filter: None,
            context_menu: None,
            expanded_photo_groups: std::collections::HashSet::new(),
            cleaning: false,
            clean_task: None,
        }
    }
}

impl DeclutterState {
    pub fn auto_pick_best_photos(&mut self) {
        for group in &mut self.photo_groups {
            for photo in &mut group.photos {
                photo.selected = !photo.is_best_shot;
            }
        }
    }

    pub fn pick_duplicates_keep_newest(&mut self) {
        for group in &mut self.duplicate_groups {
            let last_idx = group.files.len().saturating_sub(1);
            for (idx, file) in group.files.iter_mut().enumerate() {
                file.selected = idx < last_idx;
            }
        }
    }

    pub fn pick_duplicates_keep_oldest(&mut self) {
        for group in &mut self.duplicate_groups {
            for (idx, file) in group.files.iter_mut().enumerate() {
                file.selected = idx > 0;
            }
        }
    }

    pub fn total_downloads_cleanable(&self) -> u64 {
        self.download_items
            .iter()
            .filter(|i| i.selected)
            .map(|i| i.size)
            .sum()
    }

    pub fn total_downloads_size(&self) -> u64 {
        self.download_items.iter().map(|i| i.size).sum()
    }

    pub fn total_large_files_size(&self) -> u64 {
        self.large_files.iter().map(|i| i.size).sum()
    }

    pub fn total_large_files_cleanable(&self) -> u64 {
        self.large_files
            .iter()
            .filter(|i| i.selected)
            .map(|i| i.size)
            .sum()
    }

    pub fn total_duplicates_cleanable(&self) -> u64 {
        self.duplicate_groups
            .iter()
            .map(|g| g.cleanable_size())
            .sum()
    }

    pub fn total_photos_cleanable(&self) -> u64 {
        self.photo_groups.iter().map(|g| g.cleanable_size()).sum()
    }

    pub fn total_potential_savings(&self) -> u64 {
        self.total_downloads_cleanable()
            + self.total_large_files_cleanable()
            + self.total_duplicates_cleanable()
            + self.total_photos_cleanable()
    }
}

/// 视图总装渲染入口
pub fn render_declutter_view(root: &Root, cx: &mut Context<Root>) -> AnyElement {
    let state = &root.declutter;

    match state.tab {
        DeclutterTab::Overview => render_overview_tab(root, cx),
        DeclutterTab::SimilarPhotos => render_similar_photos_tab(root, cx),
        DeclutterTab::Duplicates => render_duplicates_tab(root, cx),
        DeclutterTab::LargeFiles => render_large_files_tab(root, cx),
        DeclutterTab::Downloads => render_downloads_tab(root, cx),
    }
}
