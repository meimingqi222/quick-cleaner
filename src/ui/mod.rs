//! GPUI 界面根视图与状态管理

pub mod components;
pub mod i18n;
pub mod text_input;
pub mod theme;
pub mod views;

use crate::core::apps::{
    AppFilterPreset, AppSortState, InstalledApp, ResidualItem, ResidualScanResult,
};
use crate::core::categories::{all_targets, CategoryId};
use crate::core::cleaner::{
    clean_arbitrary, clean_targets, CleanProgress, CleanReport, CleanSnapshot, CleanTarget,
};
use crate::core::safety::is_protected;
use crate::core::apps::filter_and_sort_apps;
use crate::core::disk::{DiskSelectionState, MftScan, Node};
use crate::core::i18n::{bilingual, Language, Text};
use crate::core::settings::Settings;
use crate::core::model::{fmt_size, Check};
use crate::core::scanner::{
    apply_clean_result, merge_discovered, scan_discovered, scan_fixed, CategorySummary, ScanItem,
};
use crate::platform::{
    get_volume_space, is_elevated, list_installed_apps, list_ntfs_volumes, run_uninstaller_and_wait,
    scan_residuals, clean_residuals, scan_volume, verify_residuals,
};
use crate::ui::components::*;
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::views::*;

use gpui::{
    div, prelude::*, px, rgb, Context, IntoElement, Render, Task, Window,
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct Root {
    pub language: Language,
    /// 落盘的用户设置。语言以它为准：首次启动没有配置文件时，
    /// `Settings::default()` 会按系统显示语言给出默认值。
    pub settings: Settings,
    pub categories: Vec<CategorySummary>,
    pub scanned: bool,
    pub scanning: bool,
    pub view: View,
    pub cleaning: bool,
    pub scan_task: Option<Task<()>>,
    /// 第二阶段（构建产物检索）的任务槽。它比第一阶段慢一个数量级，
    /// 必须独立持有，否则会和第一阶段互相顶掉句柄。
    pub discover_task: Option<Task<()>>,
    /// 第二阶段是否还在跑。界面靠它给开发者类目显示「检索中」。
    pub discovering: bool,
    /// 每发起一轮扫描就自增。第二阶段回来时用它判断「我属于的那轮扫描
    /// 是不是已经被新的一轮顶掉了」，避免把过期结果并进新数据。
    pub scan_gen: u64,
    pub live: Arc<AtomicBool>,
    /// 状态栏文案。存双语而不是渲染好的字符串——状态栏是常驻的，
    /// 用户切语言时最后那句话也得跟着变，不能停在写入时的语言上。
    pub status: Text,
    pub freed_total: u64,
    pub last_failed: Vec<PathBuf>,
    pub last_failed_files: u64,
    pub show_failed_details: bool,

    pub selected: HashSet<PathBuf>,
    pub expanded: HashSet<CategoryId>,
    /// 每个分类展开后各自的滚动位置。「项目构建产物」这类可能有近千条，
    /// 必须走虚拟化列表，而 uniform_list 需要一个长期持有的滚动句柄。
    pub junk_scroll: std::collections::HashMap<CategoryId, gpui::UniformListScrollHandle>,
    /// 正在拖拽哪个分类的滚动条滑块：(分类, 按下时鼠标 y, 按下时滚动偏移)
    pub junk_scroll_drag: Option<(CategoryId, f32, f32)>,

    pub confirm: Option<ConfirmRequest>,
    pub clean_progress: Option<Arc<CleanProgress>>,
    /// 清理任务独占的槽位。以前清理任务会借用 scan_task / mft_task，
    /// 一旦清理和扫描重叠就会互相顶掉对方的句柄。
    pub clean_task: Option<Task<()>>,
    pub tick_task: Option<Task<()>>,
    pub elevated: bool,

    // ---- 软件管理 (Geek Uninstaller 风格) ----
    pub apps: Vec<InstalledApp>,
    pub apps_scanned: bool,
    pub apps_scanning: bool,
    pub apps_task: Option<Task<()>>,
    pub apps_sort: AppSortState,
    pub apps_preset: AppFilterPreset,
    pub apps_search: String,
    /// 搜索框光标/选区的**字节**范围
    pub apps_search_sel: std::ops::Range<usize>,
    /// 输入法正在组合中的那段文本的字节范围（拼音串，尚未确认）
    pub apps_search_marked: Option<std::ops::Range<usize>>,
    /// 搜索框最近一次绘制的位置，用来定位输入法候选窗口
    pub apps_search_bounds: Option<gpui::Bounds<gpui::Pixels>>,
    /// 软件表每次被整体替换就自增，用来判定渲染缓存是否失效
    pub apps_gen: u64,
    /// 过滤 + 排序后的 `apps` 下标，渲染直接读这里
    pub apps_view: Vec<usize>,
    apps_view_key: Option<AppsViewKey>,
    /// 软件表也走虚拟化列表，句柄需长期持有
    pub apps_list_scroll: gpui::UniformListScrollHandle,
    pub apps_scroll_drag: Option<(f32, f32)>,
    pub residual_result: Option<ResidualScanResult>,
    pub residual_scanning: bool,
    pub residual_task: Option<Task<()>>,
    pub residual_selected: HashSet<usize>,

    // ---- 磁盘分析（Disk Lens 空间透镜）----
    pub mft: Option<MftScan>,
    pub mft_scanning: bool,
    /// 保留错误值本身而不是渲染好的字符串：错误卡片会一直挂在界面上，
    /// 用户中途切语言时它也得跟着变。
    pub mft_error: Option<crate::core::disk::MftError>,
    pub mft_task: Option<Task<()>>,
    pub volumes: Vec<char>,
    pub disk_volume: char,
    pub disk_tab: DiskTab,
    pub disk_path: Vec<u32>,
    /// 磁盘透镜的勾选状态（含继承与局部排除），实现见 `core::disk`
    pub disk_sel: DiskSelectionState,
    pub disk_space: Option<(u64, u64)>,
    /// 当前目录（或最大文件列表）的渲染行缓存
    pub disk_rows: Vec<DiskRow>,
    disk_rows_key: Option<DiskRowsKey>,
    /// MFT 树每次被替换或就地修改就自增
    pub mft_gen: u64,
    pub anim_phase: usize,
    pub apps_focus_handle: gpui::FocusHandle,
    pub apps_context_menu: Option<AppsContextMenu>,
}

/// 磁盘透镜列表里的一行，连同渲染需要的派生数据一起算好。
///
/// `path_of` 要沿父链回溯到根，`is_protected` 要归一化路径并比对规则表，
/// 两者以前在每一帧、每一行上重复调用三次。现在只在目录/标签页/树本身
/// 变化时算一次。
#[derive(Clone, Debug)]
pub struct DiskRow {
    pub node: Node,
    pub path: PathBuf,
    pub protected: bool,
}

/// 磁盘行缓存的失效键
type DiskRowsKey = (char, u32, DiskTab, u64);

/// 软件列表视图缓存的失效键
type AppsViewKey = (u64, AppFilterPreset, String, AppSortState);

/// 磁盘透镜一屏最多渲染多少行（超出部分用户也看不过来）
pub const DISK_MAX_ROWS: usize = 200;

#[derive(Clone, Debug)]
pub struct AppsContextMenu {
    pub app: InstalledApp,
    pub x: f32,
    pub y: f32,
}

impl Root {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let volumes = list_ntfs_volumes();
        let disk_volume = if volumes.contains(&'C') {
            'C'
        } else {
            volumes.first().copied().unwrap_or('C')
        };
        let disk_space = get_volume_space(disk_volume);
        let apps_focus_handle = cx.focus_handle();
        // 有配置文件就照配置文件，没有就按系统显示语言（中文系统用中文，其余英文）
        let settings = Settings::load();
        Self {
            language: settings.language,
            settings,
            categories: Vec::new(),
            scanned: false,
            scanning: false,
            view: View::Dashboard,
            cleaning: false,
            scan_task: None,
            discover_task: None,
            discovering: false,
            scan_gen: 0,
            live: Arc::new(AtomicBool::new(true)),
            status: bilingual(|l| tr_status_ready(l).to_string()),
            freed_total: 0,
            last_failed: Vec::new(),
            last_failed_files: 0,
            show_failed_details: false,
            selected: HashSet::new(),
            expanded: HashSet::new(),
            junk_scroll: CategoryId::ALL
                .iter()
                .map(|&c| (c, gpui::UniformListScrollHandle::new()))
                .collect(),
            junk_scroll_drag: None,
            confirm: None,
            clean_progress: None,
            clean_task: None,
            tick_task: None,
            elevated: is_elevated(),
            apps: Vec::new(),
            apps_scanned: false,
            apps_scanning: false,
            apps_task: None,
            apps_sort: AppSortState::default(),
            apps_preset: AppFilterPreset::All,
            apps_search: String::new(),
            apps_search_sel: 0..0,
            apps_search_marked: None,
            apps_search_bounds: None,
            apps_gen: 0,
            apps_view: Vec::new(),
            apps_view_key: None,
            apps_list_scroll: gpui::UniformListScrollHandle::new(),
            apps_scroll_drag: None,
            residual_result: None,
            residual_scanning: false,
            residual_task: None,
            residual_selected: HashSet::new(),
            mft: None,
            mft_scanning: false,
            mft_error: None,
            mft_task: None,
            volumes,
            disk_volume,
            disk_tab: DiskTab::Tree,
            disk_path: vec![crate::core::disk::ROOT_NODE],
            disk_sel: DiskSelectionState::new(),
            disk_space,
            disk_rows: Vec::new(),
            disk_rows_key: None,
            mft_gen: 0,
            anim_phase: 0,
            apps_focus_handle,
            apps_context_menu: None,
        }
    }

    pub fn toggle_language(&mut self, cx: &mut Context<Self>) {
        self.set_language(self.language.toggle(), cx);
    }

    /// 切换界面语言并**立刻落盘**。
    ///
    /// 写盘放在这里而不是退出时：GPUI 应用被任务管理器结束、或者清理过程中崩溃，
    /// 都不会走到退出路径，那样用户就会觉得「设置没保存」。一次几十字节的写入，
    /// 频率是「用户点了语言按钮」，不值得为它做延迟落盘。
    pub fn set_language(&mut self, lang: Language, cx: &mut Context<Self>) {
        if self.language == lang {
            return;
        }
        self.language = lang;
        // 双语标签是随渲染缓存一起取的，改语言必须让两个派生缓存失效
        self.apps_gen += 1;
        self.mft_gen += 1;
        self.settings.language = lang;
        self.settings.save();
        cx.notify();
    }

    pub fn open_app_context_menu(&mut self, app: InstalledApp, x: f32, y: f32) {
        self.apps_context_menu = Some(AppsContextMenu { app, x, y });
    }

    pub fn close_context_menu(&mut self) {
        self.apps_context_menu = None;
    }

    /// 发起一轮扫描。**分两个阶段**，界面不必等最慢的那条通道。
    ///
    /// 第一阶段扫固定路径表（`%TEMP%`、各种缓存目录），本机约 1 秒就能出结果，
    /// 界面立刻可用；第二阶段才去全盘检索构建产物，那是整轮里最贵的一步
    /// （本机 25 秒量级），跑完再把结果并进列表。
    ///
    /// 之所以值得拆：耗时几乎全在第二阶段，而它对应的「项目构建产物」类目
    /// **默认根本不勾选**——让用户为一个默认不清的类目干等半分钟，代价和收益
    /// 完全不成比例。
    pub fn start_scan(&mut self, cx: &mut Context<Self>) {
        if self.scanning {
            return;
        }
        // 通知上一轮（可能还在跑的第二阶段）停下
        self.live.store(false, Ordering::Relaxed);
        self.scan_task.take();
        self.discover_task.take();

        self.scan_gen += 1;
        let gen = self.scan_gen;
        self.scanning = true;
        self.scanned = false;
        self.discovering = false;
        self.status = bilingual(|l| tr_status_scanning(l).to_string());
        let live = Arc::new(AtomicBool::new(true));
        self.live = live.clone();
        self.start_tick(cx);
        cx.notify();

        let targets = all_targets();
        let scan = cx
            .background_executor()
            .spawn(async move { scan_fixed(&targets, &live) });
        self.scan_task = Some(cx.spawn(async move |this, cx| {
            let result = scan.await;
            this.update(cx, |this, cx| {
                this.categories = result;
                this.scanned = true;
                this.scanning = false;
                this.select_recommended();
                let total_str = fmt_size(this.total_cleanable());
                this.status = bilingual(|l| tr_status_scan_fixed_done(l, &total_str));
                this.start_discovery(gen, cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// 第二阶段：全盘检索构建产物，跑完并进已有分类。
    ///
    /// `gen` 是发起这轮扫描时的 `scan_gen`。回来时如果对不上，说明用户已经
    /// 点了「重新扫描」，这份结果属于上一轮，直接丢掉——否则会把过期数据
    /// （甚至是被取消后只跑了一半的数据）并进新列表。
    fn start_discovery(&mut self, gen: u64, cx: &mut Context<Self>) {
        self.discovering = true;
        let live = self.live.clone();
        let discover = cx
            .background_executor()
            .spawn(async move { scan_discovered(&live) });

        self.discover_task = Some(cx.spawn(async move |this, cx| {
            let items = discover.await;
            this.update(cx, |this, cx| {
                if this.scan_gen != gen {
                    return;
                }
                this.discovering = false;
                merge_discovered(&mut this.categories, items);
                let total_str = fmt_size(this.total_cleanable());
                this.status = bilingual(|l| tr_status_scan_done(l, &total_str));
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn total_cleanable(&self) -> u64 {
        self.categories.iter().map(|c| c.total_size).sum()
    }

    pub fn items(&self) -> impl Iterator<Item = &ScanItem> {
        self.categories.iter().flat_map(|c| c.items.iter())
    }

    /// 按各类目的默认策略预勾选（扫描完成后的初始状态）。
    ///
    /// 开发者类目（AI 助手缓存、构建产物、worktree）一律不默认勾选：
    /// 它们删掉不会坏系统，但会让下次构建重来，甚至丢掉未提交的改动。
    pub fn select_recommended(&mut self) {
        self.selected = self
            .items()
            .filter(|i| i.category.default_selected())
            .map(|i| i.path.clone())
            .collect();
    }

    /// 勾选全部条目，包括开发者类目。
    pub fn select_every(&mut self) {
        self.selected = self.items().map(|i| i.path.clone()).collect();
    }

    /// 清空所有勾选。
    pub fn select_none(&mut self) {
        self.selected.clear();
    }

    /// 反选：已勾的取消，没勾的选上。
    pub fn invert_selection(&mut self) {
        self.selected = self
            .items()
            .filter(|i| !self.selected.contains(&i.path))
            .map(|i| i.path.clone())
            .collect();
    }

    /// 当前勾选是否恰好等于「推荐」的那一套。
    ///
    /// 用来给工具栏上的「推荐」按钮做选中态高亮，让用户一眼看出
    /// 自己是不是还停在默认状态。
    pub fn selection_is_recommended(&self) -> bool {
        let mut n = 0usize;
        for item in self.items() {
            let want = item.category.default_selected();
            if want != self.selected.contains(&item.path) {
                return false;
            }
            if want {
                n += 1;
            }
        }
        n == self.selected.len()
    }

    /// 把某一批类目整体勾上或取消（供分类标题上的复选框用）。
    pub fn set_category_selected(&mut self, id: CategoryId, on: bool) {
        let paths: Vec<PathBuf> = self
            .categories
            .iter()
            .filter(|c| c.category == id)
            .flat_map(|c| c.items.iter().map(|i| i.path.clone()))
            .collect();
        for p in paths {
            if on {
                self.selected.insert(p);
            } else {
                self.selected.remove(&p);
            }
        }
    }

    /// 清理完成后就地更新扫描结果，替代整轮重扫。实现见 `core::scanner`。
    pub fn apply_clean_result(&mut self, attempted: &[PathBuf], failed: &[PathBuf]) {
        let cleared = apply_clean_result(&mut self.categories, attempted, failed);
        // 已经清空的条目不该继续占着勾选状态
        for p in cleared {
            self.selected.remove(&p);
        }
    }

    /// 全部条目数（用于工具栏显示「已选 N / 共 M」）。
    pub fn total_item_count(&self) -> usize {
        self.items().count()
    }

    pub fn selected_paths(&self) -> Vec<PathBuf> {
        self.items()
            .filter(|i| self.selected.contains(&i.path))
            .map(|i| i.path.clone())
            .collect()
    }

    /// 勾选项连同各自的处置方式（整个删掉还是只清空内容）。
    pub fn selected_targets(&self) -> Vec<CleanTarget> {
        self.items()
            .filter(|i| self.selected.contains(&i.path))
            .map(|i| CleanTarget {
                path: i.path.clone(),
                remove_dir: i.category.removes_directory(),
            })
            .collect()
    }

    pub fn selected_size(&self) -> u64 {
        self.items()
            .filter(|i| self.selected.contains(&i.path))
            .map(|i| i.size)
            .sum()
    }

    pub fn selected_count(&self) -> usize {
        self.items()
            .filter(|i| self.selected.contains(&i.path))
            .count()
    }

    pub fn failures_need_admin(&self) -> bool {
        if self.elevated || self.last_failed.is_empty() {
            return false;
        }
        let win = std::env::var("SystemRoot")
            .unwrap_or_else(|_| r"C:\Windows".into())
            .to_ascii_lowercase();
        self.last_failed.iter().any(|p| {
            let s = p.to_string_lossy().to_ascii_lowercase().replace('/', "\\");
            s.starts_with(&win) || s.contains(r"\program files") || s.contains(r"\programdata")
        })
    }

    pub fn cat_check(&self, c: &CategorySummary) -> Check {
        let n = c
            .items
            .iter()
            .filter(|i| self.selected.contains(&i.path))
            .count();
        Check::from_counts(n, c.items.len())
    }

    pub fn toggle_category(&mut self, id: CategoryId) {
        let Some(c) = self.categories.iter().find(|c| c.category == id) else {
            return;
        };
        let paths: Vec<PathBuf> = c.items.iter().map(|i| i.path.clone()).collect();
        if self.cat_check(c) == Check::On {
            for p in &paths {
                self.selected.remove(p);
            }
        } else {
            for p in paths {
                self.selected.insert(p);
            }
        }
    }

    pub fn toggle_expand(&mut self, id: CategoryId) {
        if self.expanded.contains(&id) {
            self.expanded.remove(&id);
        } else {
            self.expanded.insert(id);
        }
    }

    pub fn toggle_item(&mut self, path: &std::path::Path) {
        let pb = path.to_path_buf();
        if self.selected.contains(&pb) {
            self.selected.remove(&pb);
        } else {
            self.selected.insert(pb);
        }
    }

    pub fn switch_disk_volume(&mut self, vol: char, cx: &mut Context<Self>) {
        if self.disk_volume == vol && (self.mft.is_some() || self.mft_scanning) {
            return;
        }
        self.disk_volume = vol;
        self.mft = None;
        self.disk_rows.clear();
        self.disk_rows_key = None;
        self.disk_path = vec![crate::core::disk::ROOT_NODE];
        self.disk_sel.clear();
        self.start_mft_scan(cx);
    }

    pub fn start_mft_scan(&mut self, cx: &mut Context<Self>) {
        if self.mft_scanning {
            return;
        }
        self.mft_scanning = true;
        self.mft_error = None;
        let vol = self.disk_volume;
        self.disk_space = get_volume_space(vol);
        self.disk_sel.clear();
        let saved_path = self.current_disk_full_path();
        self.status = bilingual(|l| tr_status_disk_scanning(l, vol));
        self.start_tick(cx);
        cx.notify();

        let scan = cx
            .background_executor()
            .spawn(async move { scan_volume(vol, 0) });

        self.mft_task = Some(cx.spawn(async move |this, cx| {
            let result = scan.await;
            this.update(cx, |this, cx| {
                this.mft_scanning = false;
                match result {
                    Ok(s) => {
                        let (files, used) = (s.file_count, fmt_size(s.total_size));
                        this.status = bilingual(|l| tr_status_disk_done(l, files, &used));
                        // 仅当 saved_path 确实属于当前卷时才尝试恢复层级；跨盘切换时直接回到新盘根目录
                        let is_same_vol = saved_path.as_ref().is_some_and(|p| {
                            p.to_string_lossy().starts_with(&format!("{vol}:"))
                        });
                        if is_same_vol {
                            if let Some(target_path) = saved_path {
                                let resolved = s.tree.find_path(&target_path);
                                this.disk_path = if resolved.is_empty() {
                                    vec![s.tree.root()]
                                } else {
                                    resolved
                                };
                            } else {
                                this.disk_path = vec![s.tree.root()];
                            }
                        } else {
                            this.disk_path = vec![s.tree.root()];
                        }
                        this.mft = Some(s);
                        this.mft_gen += 1;
                    }
                    Err(e) => {
                        this.status = bilingual(|l| tr_status_disk_failed(l, &tr_mft_error(l, &e)));
                        this.mft_error = Some(e);
                        this.mft = None;
                        this.mft_gen += 1;
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn current_disk_full_path(&self) -> Option<PathBuf> {
        let mft = self.mft.as_ref()?;
        let cur = *self.disk_path.last().unwrap_or(&mft.tree.root());
        Some(PathBuf::from(mft.tree.path_of(cur)))
    }

    // ---- 磁盘勾选：全部委托给 core::disk::DiskSelectionState ----
    // 这套「父级继承 + 子项排除」的逻辑曾经在这里和 core 各写了一份，
    // 两边已经开始出现行为差异，现在只保留 core 那份。

    pub fn is_disk_item_selected(&self, path: &std::path::Path) -> bool {
        self.disk_sel.is_selected(path)
    }

    pub fn toggle_disk_item(&mut self, path: &std::path::Path, size: u64) {
        self.disk_sel.toggle(path, size);
    }

    pub fn clear_disk_selection(&mut self) {
        self.disk_sel.clear();
    }

    pub fn disk_selected_size(&self) -> u64 {
        self.disk_sel.total_size()
    }

    pub fn disk_selected_count(&self) -> usize {
        self.disk_sel.len()
    }

    pub fn request_clean_disk_selected(&mut self, cx: &mut Context<Self>) {
        if self.disk_sel.is_empty() || self.cleaning {
            return;
        }
        let total_size = self.disk_selected_size();
        let count = self.disk_sel.len();
        let lang = self.language;

        self.confirm = Some(ConfirmRequest {
            title: tr_confirm_delete_selected_title(lang).to_string(),
            body: tr_confirm_delete_selected_msg(lang, count, &fmt_size(total_size)),
            detail: tr_confirm_no_recycle_check_data(lang).to_string(),
            kind: ConfirmKind::CleanDiskSelected,
        });
        cx.notify();
    }

    pub fn start_apps_scan(&mut self, cx: &mut Context<Self>) {
        if self.apps_scanning {
            return;
        }
        self.apps_scanning = true;
        self.apps_scanned = false;
        self.status = bilingual(|l| tr_status_apps_scanning(l).to_string());
        self.start_tick(cx);
        cx.notify();

        let live = Arc::new(AtomicBool::new(true));
        let scan = cx
            .background_executor()
            .spawn(async move { list_installed_apps(&live) });

        self.apps_task = Some(cx.spawn(async move |this, cx| {
            let result = scan.await;
            this.update(cx, |this, cx| {
                this.apps = result;
                this.apps_gen += 1;
                this.apps_scanned = true;
                this.apps_scanning = false;
                let total_size: u64 = this.apps.iter().map(|a| a.estimated_size).sum();
                let (count, size) = (this.apps.len(), fmt_size(total_size));
                this.status = bilingual(|l| tr_status_apps_done(l, count, &size));
                cx.notify();
            })
            .ok();
        }));
    }

    /// 卸载软件：**先采集残留候选，再运行官方卸载程序**。
    ///
    /// 顺序很关键。安装目录、指向它的注册表值、服务的 ImagePath——这些
    /// 证据只在卸载之前存在。原先是卸载跑完才扫，那时安装目录已经没了，
    /// 所有基于路径的匹配全部落空，于是几乎每个软件都被报成「非常干净」。
    /// 现在提前扫一遍留下候选集，卸载结束后再复核哪些还在，剩下的才是
    /// 官方卸载程序没清干净的部分。
    pub fn request_uninstall_app(&mut self, app: InstalledApp, cx: &mut Context<Self>) {
        let name = app.name.clone();
        let pre_target = app.clone();
        let uninst_target = app.clone();

        self.residual_scanning = true;
        self.residual_result = None;
        self.status = bilingual(|l| tr_status_uninstall_waiting(l, &name));
        self.start_tick(cx);
        cx.notify();

        let work = cx.background_executor().spawn(async move {
            // 1. 卸载前采集候选（此时安装目录还在，证据最全）
            let pre = scan_residuals(&pre_target);
            // 2. 运行官方卸载程序并等它退出
            let result = run_uninstaller_and_wait(&uninst_target);
            // 3. 复核：只留下卸载程序没清掉的
            let remaining = verify_residuals(pre.items);
            (result, remaining)
        });

        self.residual_task = Some(cx.spawn(async move |this, cx| {
            let (result, remaining) = work.await;
            this.update(cx, |this, cx| {
                this.residual_scanning = false;
                let total: u64 = remaining.iter().map(|i| i.size()).sum();
                let res = ResidualScanResult {
                    app_name: name.clone(),
                    items: remaining,
                    total_file_size: total,
                };
                let ok = result.is_ok();
                let (count, size) = (res.items.len(), fmt_size(res.total_file_size));
                this.status = bilingual(|l| {
                    let head = if ok {
                        tr_status_uninstall_done(l, &name)
                    } else {
                        tr_status_uninstall_failed(l, &name)
                    };
                    tr_status_uninstall_residual(l, &head, count, &size)
                });
                this.residual_selected = res.default_selection();
                this.residual_result = Some(res);
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn start_residual_scan(&mut self, app: InstalledApp, cx: &mut Context<Self>) {
        if self.residual_scanning {
            return;
        }
        self.residual_scanning = true;
        self.residual_result = None;
        let scanning_name = app.name.clone();
        self.status = bilingual(|l| tr_status_residual_scanning(l, &scanning_name));
        cx.notify();

        let target = app.clone();
        let scan = cx
            .background_executor()
            .spawn(async move { scan_residuals(&target) });

        self.residual_task = Some(cx.spawn(async move |this, cx| {
            let res = scan.await;
            this.update(cx, |this, cx| {
                this.residual_scanning = false;
                let count = res.items.len();
                // 只预勾「确定」的；模糊匹配出来的交给用户自己判断
                this.residual_selected = res.default_selection();
                let (name, size) = (res.app_name.clone(), fmt_size(res.total_file_size));
                this.status = bilingual(|l| tr_status_residual_done(l, &name, count, &size));
                this.residual_result = Some(res);
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn clean_selected_residuals(&mut self, cx: &mut Context<Self>) {
        let Some(res) = self.residual_result.take() else {
            return;
        };
        let items_to_clean: Vec<ResidualItem> = self
            .residual_selected
            .iter()
            .filter_map(|&idx| res.items.get(idx).cloned())
            .collect();

        if items_to_clean.is_empty() {
            self.status = bilingual(|l| tr_status_residual_none_selected(l).to_string());
            cx.notify();
            return;
        }

        let total_bytes: u64 = items_to_clean.iter().map(|it| it.size()).sum();
        let prog = Arc::new(CleanProgress::new(items_to_clean.len() as u64, total_bytes));
        // 用来读实际删掉的字节数——按预期值记账会在有删除失败时虚报释放量
        let progress = prog.clone();
        // 残留项被全部选中时，这个软件才算真的清干净了，行才能从列表移除
        let cleaned_everything = items_to_clean.len() == res.items.len();
        let app_name = res.app_name.clone();
        let cleaning_name = res.app_name.clone();
        let cleaning_count = items_to_clean.len();
        self.status =
            bilingual(|l| tr_status_residual_cleaning(l, &cleaning_name, cleaning_count));
        cx.notify();

        let clean = cx
            .background_executor()
            .spawn(async move { clean_residuals(&items_to_clean, &prog) });

        self.residual_task = Some(cx.spawn(async move |this, cx| {
            let report = clean.await;
            this.update(cx, |this, cx| {
                let snap = progress.snapshot();
                this.freed_total += snap.bytes;
                this.residual_selected.clear();

                // 局部更新：软件确实被清干净时，直接把它从内存里的软件表
                // 摘掉，不再触发一轮完整的注册表枚举 + 全盘安装目录遍历。
                let removed = cleaned_everything && report.failed.is_empty();
                if removed {
                    this.apps.retain(|a| a.name != app_name);
                    this.apps_gen += 1;
                }

                let (ok, fails, size) = (report.ok, report.failed.len(), fmt_size(snap.bytes));
                this.status = bilingual(|l| {
                    if fails == 0 {
                        tr_status_residual_cleaned(l, &app_name, ok, &size)
                    } else {
                        tr_status_residual_cleaned_partial(l, &app_name, &size, fails)
                    }
                });
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn request_clean_selected(&mut self, cx: &mut Context<Self>) {
        let count = self.selected_count();
        if count == 0 || self.cleaning || !self.scanned {
            return;
        }
        let lang = self.language;
        self.confirm = Some(ConfirmRequest {
            title: tr_confirm_delete_title(lang).to_string(),
            body: tr_confirm_delete_msg(lang, count, &fmt_size(self.selected_size())),
            detail: tr_confirm_no_recycle(lang).to_string(),
            kind: ConfirmKind::CleanSelected,
        });
        cx.notify();
    }

    pub fn request_clean_path(&mut self, path: PathBuf, size: u64, cx: &mut Context<Self>) {
        if self.cleaning {
            return;
        }
        let lang = self.language;
        if is_protected(&path) {
            let shown = path.display().to_string();
            self.status = bilingual(|l| tr_protected_path(l, &shown));
            cx.notify();
            return;
        }

        self.confirm = Some(ConfirmRequest {
            title: tr_confirm_delete_title(lang).to_string(),
            body: tr_confirm_delete_path_msg(lang, &path.display().to_string(), &fmt_size(size)),
            detail: tr_confirm_no_recycle_check_running(lang).to_string(),
            kind: ConfirmKind::CleanPath(path, size),
        });
        cx.notify();
    }

    pub fn confirm_accept(&mut self, cx: &mut Context<Self>) {
        let Some(req) = self.confirm.take() else {
            return;
        };
        match req.kind {
            ConfirmKind::CleanSelected => self.start_clean(cx),
            ConfirmKind::CleanPath(p, size) => self.start_clean_path(p, size, cx),
            ConfirmKind::CleanDiskSelected => self.start_clean_disk_selected(cx),
        }
    }

    pub fn is_busy(&self) -> bool {
        self.cleaning
            || self.scanning
            || self.discovering
            || self.apps_scanning
            || self.mft_scanning
            || self.residual_scanning
    }

    pub fn start_tick(&mut self, cx: &mut Context<Self>) {
        if self.tick_task.is_some() {
            return;
        }
        self.tick_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let running = this
                    .update(cx, |this, cx| {
                        this.anim_phase = (this.anim_phase + 1) % 120;
                        cx.notify();
                        this.is_busy()
                    })
                    .unwrap_or(false);
                if !running {
                    this.update(cx, |this, _| {
                        this.tick_task = None;
                    })
                    .ok();
                    break;
                }
            }
        }));
    }

    pub fn clean_snapshot(&self) -> Option<CleanSnapshot> {
        self.clean_progress.as_ref().map(|p| p.snapshot())
    }

    pub fn cancel_clean(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = &self.clean_progress {
            p.request_cancel();
            self.status = bilingual(|l| tr_status_stopping(l).to_string());
            cx.notify();
        }
    }

    pub fn start_clean(&mut self, cx: &mut Context<Self>) {
        if self.cleaning || !self.scanned {
            return;
        }
        let paths = self.selected_paths();
        if paths.is_empty() {
            self.status = bilingual(|l| tr_status_nothing_selected(l).to_string());
            cx.notify();
            return;
        }

        let total_files: u64 = self
            .items()
            .filter(|i| self.selected.contains(&i.path))
            .map(|i| i.file_count)
            .sum();
        let total_bytes = self.selected_size();

        self.cleaning = true;
        self.last_failed.clear();
        let progress = Arc::new(CleanProgress::new(total_files, total_bytes));
        self.clean_progress = Some(progress.clone());
        let n = paths.len();
        self.status = bilingual(|l| tr_status_deleting_n(l, n));
        self.start_tick(cx);
        cx.notify();

        let attempted = paths.clone();
        let targets = self.selected_targets();
        let clean = cx
            .background_executor()
            .spawn(async move { clean_targets(&targets, &progress) });

        self.clean_task = Some(cx.spawn(async move |this, cx| {
            let report: CleanReport = clean.await;
            this.update(cx, |this, cx| {
                this.cleaning = false;
                this.last_failed = report.failed;
                let snap = this.clean_snapshot().unwrap_or_default();
                this.last_failed_files = snap.failed;
                this.freed_total += snap.bytes;

                // 就地更新，不再触发整轮复扫（开发垃圾扫描要几十秒）
                let failed = this.last_failed.clone();
                this.apply_clean_result(&attempted, &failed);

                let fails = this.last_failed.len();
                let (files, size) = (snap.files, fmt_size(snap.bytes));
                this.status = bilingual(|l| {
                    if fails > 0 {
                        tr_status_clean_done_partial(l, files, &size, fails)
                    } else {
                        tr_status_clean_done(l, files, &size)
                    }
                });
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn start_clean_path(&mut self, path: PathBuf, size: u64, cx: &mut Context<Self>) {
        if self.cleaning {
            return;
        }
        self.cleaning = true;
        let progress = Arc::new(CleanProgress::new(0, size));
        self.clean_progress = Some(progress.clone());
        let shown = path.display().to_string();
        self.status = bilingual(|l| tr_status_deleting_path(l, &shown));
        self.start_tick(cx);
        cx.notify();

        let target = path.clone();
        let clean = cx
            .background_executor()
            .spawn(async move { clean_arbitrary(std::slice::from_ref(&target), &progress) });

        self.clean_task = Some(cx.spawn(async move |this, cx| {
            let report: CleanReport = clean.await;
            this.update(cx, |this, cx| {
                this.cleaning = false;
                let snap = this.clean_snapshot().unwrap_or_default();
                this.freed_total += snap.bytes;
                if report.failed.is_empty() {
                    let (shown, files, size) =
                        (path.display().to_string(), snap.files, fmt_size(snap.bytes));
                    this.status = bilingual(|l| tr_status_deleted_path(l, &shown, files, &size));
                    // 局部即时从内存树中剔除，祖先目录体积自动联动扣减，无需全局重扫
                    if let Some(mft) = &mut this.mft {
                        mft.remove_path(&path);
                        this.mft_gen += 1;
                        while this.disk_path.len() > 1 {
                            let cur_node = *this.disk_path.last().unwrap();
                            if !mft.tree.valid(cur_node) {
                                this.disk_path.pop();
                            } else {
                                break;
                            }
                        }
                    }
                    if let Some((_, free)) = &mut this.disk_space {
                        *free += snap.bytes;
                    }
                } else {
                    let shown = path.display().to_string();
                    this.status = bilingual(|l| tr_status_delete_failed(l, &shown));
                }
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn start_clean_disk_selected(&mut self, cx: &mut Context<Self>) {
        if self.cleaning || self.disk_sel.is_empty() {
            return;
        }
        // 展开成实际删除目标：勾选目录里若埋着被排除的子孙，会自动下钻绕开。
        let targets = self.disk_sel.resolve_targets();
        if targets.is_empty() {
            return;
        }
        let total_size = self.disk_selected_size();

        self.cleaning = true;
        let progress = Arc::new(CleanProgress::new(0, total_size));
        self.clean_progress = Some(progress.clone());
        let n = targets.len();
        self.status = bilingual(|l| tr_status_batch_deleting(l, n));
        self.start_tick(cx);
        cx.notify();

        let clean_targets = targets.clone();
        let clean = cx
            .background_executor()
            .spawn(async move { clean_arbitrary(&clean_targets, &progress) });

        self.clean_task = Some(cx.spawn(async move |this, cx| {
            let report: CleanReport = clean.await;
            this.update(cx, |this, cx| {
                this.cleaning = false;
                let snap = this.clean_snapshot().unwrap_or_default();
                this.freed_total += snap.bytes;
                this.clear_disk_selection();
                let (files, fails, size) = (snap.files, report.failed.len(), fmt_size(snap.bytes));
                this.status = bilingual(|l| {
                    if fails == 0 {
                        tr_status_batch_done(l, files, &size)
                    } else {
                        tr_status_batch_done_partial(l, &size, fails)
                    }
                });
                // 局部即时从内存树中扣减删除项，保留当前所在目录层级
                if let Some(mft) = &mut this.mft {
                    for t in &targets {
                        if !report.failed.contains(t) {
                            mft.remove_path(t);
                        }
                    }
                    this.mft_gen += 1;
                    while this.disk_path.len() > 1 {
                        let cur_node = *this.disk_path.last().unwrap();
                        if !mft.tree.valid(cur_node) {
                            this.disk_path.pop();
                        } else {
                            break;
                        }
                    }
                }
                if let Some((_, free)) = &mut this.disk_space {
                    *free += snap.bytes;
                }
                cx.notify();
            })
            .ok();
        }));
    }
}

impl Root {
    /// 重建那些「贵到不能每帧算」的派生数据。
    ///
    /// 在 `render` 开头调用一次。内部靠失效键判断，状态没变时是个空操作，
    /// 所以扫描期间 20fps 的重绘不会反复付出代价。
    fn refresh_render_caches(&mut self) {
        self.refresh_disk_rows();
        self.refresh_apps_view();
    }

    fn refresh_disk_rows(&mut self) {
        let Some(scan) = &self.mft else {
            self.disk_rows.clear();
            self.disk_rows_key = None;
            return;
        };
        let tree = &scan.tree;
        let cur = *self.disk_path.last().unwrap_or(&tree.root());
        let key: DiskRowsKey = (self.disk_volume, cur, self.disk_tab, self.mft_gen);
        if self.disk_rows_key == Some(key) {
            return;
        }

        let nodes = match self.disk_tab {
            DiskTab::Tree => tree.children(cur),
            DiskTab::Files => tree.largest_files(DISK_MAX_ROWS),
        };

        // 整批共用一个路径缓存，父链只回溯一次
        let mut path_cache = std::collections::HashMap::new();
        self.disk_rows = nodes
            .into_iter()
            .map(|node| {
                let path = PathBuf::from(tree.path_of_with(node.idx, &mut path_cache));
                let protected = is_protected(&path);
                DiskRow {
                    node,
                    path,
                    protected,
                }
            })
            // 受保护项照常入列。磁盘分析的价值就在于「看清谁占了空间」，
            // 而用户目录、Windows、Program Files 恰恰是最大的几个——把它们
            // 藏掉，环形图占比会变得毫无意义，也没法下钻进去找真正的元凶。
            // 它们在行渲染里标注「系统保护项目」、禁用勾选与删除，但可以点进去。
            .take(DISK_MAX_ROWS)
            .collect();
        self.disk_rows_key = Some(key);
    }

    fn refresh_apps_view(&mut self) {
        let key: AppsViewKey = (
            self.apps_gen,
            self.apps_preset,
            self.apps_search.clone(),
            self.apps_sort,
        );
        if self.apps_view_key.as_ref() == Some(&key) {
            return;
        }
        self.apps_view = filter_and_sort_apps(
            &self.apps,
            self.apps_preset,
            &self.apps_search,
            self.apps_sort,
        );
        self.apps_view_key = Some(key);
    }

    /// 当前视图里**可勾选**的 (路径, 体积) 列表。
    ///
    /// 受保护项虽然显示在列表里，但不参与勾选，也不能被「全选」带上，
    /// 否则表头复选框永远到不了全选状态。
    pub fn disk_selectable(&self) -> Vec<(PathBuf, u64)> {
        self.disk_rows
            .iter()
            .filter(|r| !r.protected)
            .map(|r| (r.path.clone(), r.node.size))
            .collect()
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh_render_caches();

        let content = match self.view {
            View::Dashboard => render_dashboard_view(self, cx),
            View::Junk => render_junk_view(self, cx),
            View::Apps => render_apps_view(self, window, cx),
            View::Disk => render_disk_view(self, cx),
        };

        let mut main = div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .bg(rgb(CARD))
            .flex()
            .flex_col()
            .child(render_top_bar(self, cx));

        if self.scanning {
            main = main.child(render_scan_line());
        }

        main = main.child(div().flex_1().min_h(px(0.)).flex().child(content));

        if self.cleaning {
            main = main.child(render_progress_bar(self, cx));
        } else if self.view == View::Junk {
            main = main.child(render_clean_bar(self, cx));
        } else if self.view == View::Disk {
            if let Some(bar) = render_disk_clean_bar(self, cx) {
                main = main.child(bar);
            }
        }

        let mut root = div()
            .size_full()
            .min_w(px(0.))
            .relative()
            .overflow_hidden()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .child(render_sidebar(self, cx))
                    .child(main),
            )
            .child(
                div()
                    .flex_none()
                    .w_full()
                    .px_8()
                    .py_1()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .bg(rgb(BG))
                    .border_t_1()
                    .border_color(rgba(OUTLINE_VAR, 0.6))
                    .child(self.status.get(self.language).to_string()),
            );

        if let Some(req) = self.confirm.clone() {
            root = root.child(render_confirm_dialog(self, &req, cx));
        }

        if let Some(modal) = render_residual_modal(self, cx) {
            root = root.child(modal);
        }

        if let Some(menu) = render_apps_context_menu(self, cx) {
            root = root.child(menu);
        }

        root.into_any_element()
    }
}
