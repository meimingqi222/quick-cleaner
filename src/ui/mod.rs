//! GPUI 界面根视图与状态管理

mod actions;
mod app_icons;
pub mod components;
pub mod i18n;
pub mod state;
pub mod text_input;
pub mod theme;
pub mod views;
pub use state::*;

use crate::core::apps::filter_and_sort_apps;
use crate::core::apps::{AppFilterPreset, AppSortState, InstalledApp};
use crate::core::categories::CategoryId;
use crate::core::cleaner::{CleanTarget, Disposal};
use crate::core::disk::{DiskSelectionState, VolumeId};
use crate::core::i18n::{bilingual, Language, Text};
use crate::core::model::Check;
use crate::core::safety::is_protected;
use crate::core::scanner::{CategorySummary, ScanItem};
use crate::core::settings::Settings;
use crate::platform::{get_volume_space, is_elevated, list_volumes};
// `components` 与 `views` 导出的名字没有共同前缀（`card` / `checkbox` /
// `render_donut`），glob 进来就看不出谁是谁，所以显式列出。
// `i18n::*`（清一色 `tr_` 前缀）和 `theme::*`（清一色大写色值常量）保持
// glob——那两个是「命名空间即词汇表」，逐个列反而更难读。
use crate::ui::components::{
    render_confirm_dialog, render_fda_onboarding_modal, render_progress_bar, render_residual_modal,
    render_scan_line, render_sidebar, render_top_bar, render_uninstall_progress, ConfirmRequest,
    View,
};
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::views::{
    render_apps_context_menu, render_apps_view, render_clean_bar, render_dashboard_view,
    render_declutter_context_menu, render_declutter_view, render_disk_clean_bar, render_disk_view,
    render_disk_volume_dropdown, render_junk_view, render_search_view, DeclutterContextMenu,
    DeclutterState, DiskTab,
};

use gpui::{div, prelude::*, px, rgb, Context, IntoElement, Render, Task, Window};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

/// 应用根视图。
///
/// 四个功能域（智能清理 / 软件管理 / 磁盘透镜 / 深度卸载）各自的状态收在
/// 独立的结构体里，`Root` 自己只保留真正全局的东西。以前这里是五十来个
/// 平铺字段，`xxx_scanning` / `xxx_task` / `xxx_gen` 三件套每加一个功能就
/// 复制一遍，谁属于谁全靠前缀约定。
pub struct Root {
    pub language: Language,
    /// 落盘的用户设置。语言以它为准：首次启动没有配置文件时，
    /// `Settings::default()` 会按系统显示语言给出默认值。
    pub settings: Settings,
    pub view: View,
    /// 状态栏文案。存双语而不是渲染好的字符串——状态栏是常驻的，
    /// 用户切语言时最后那句话也得跟着变，不能停在写入时的语言上。
    pub status: Text,
    pub live: Arc<AtomicBool>,
    pub elevated: bool,
    pub confirm: Option<ConfirmRequest>,
    pub tick_task: Option<Task<()>>,
    pub anim_phase: usize,
    /// 搜索框光标闪烁状态（true=显示，false=隐藏）
    pub cursor_blink_visible: bool,
    pub cursor_blink_task: Option<Task<()>>,
    /// 当前是否有输入框持有焦点。由 `render` 每帧写入，闪烁任务读它决定
    /// 自己该不该继续跑——否则焦点离开后任务会一直空转到进程退出。
    pub cursor_blink_wanted: bool,
    /// macOS 专属：是否已获得完全磁盘访问权限（FDA）。
    pub fda_status: bool,
    /// 是否展示完全磁盘访问权限引导模态弹窗。
    pub show_fda_onboarding: bool,

    pub junk: JunkState,
    pub apps: AppsState,
    pub residual: ResidualState,
    pub disk: DiskState,
    pub clean: CleanState,
    pub declutter: DeclutterState,
    pub search: SearchState,
    /// macOS 整盘索引缓存。垃圾扫描和磁盘透镜共用这一份。
    /// 不再单独持有用户目录索引——整盘索引已包含用户目录，
    /// 省掉 ~700MB 重复内存。
    #[cfg(not(windows))]
    pub macos_root_index: Option<std::sync::Arc<crate::core::disk::ScanResult>>,
}

impl Root {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let volumes = list_volumes();
        // Windows 上默认选 C 盘，其它平台选第一个（macOS 只有一个 `/`）
        #[cfg(windows)]
        let default_vol = VolumeId::from_drive_letter('C');
        #[cfg(not(windows))]
        let default_vol = volumes
            .first()
            .cloned()
            .unwrap_or_else(|| VolumeId::from_mount_point(std::path::PathBuf::from("/")));
        let disk_volume = if volumes.contains(&default_vol) {
            default_vol
        } else {
            volumes.first().cloned().unwrap_or(default_vol)
        };
        let volume_spaces: std::collections::HashMap<_, _> = volumes
            .iter()
            .filter_map(|v| get_volume_space(v).map(|s| (v.clone(), s)))
            .collect();
        let disk_space = volume_spaces.get(&disk_volume).copied();
        let apps_focus_handle = cx.focus_handle();
        let search_focus_handle = cx.focus_handle();
        // 有配置文件就照配置文件，没有就按系统显示语言（中文系统用中文，其余英文）
        let settings = Settings::load();
        #[cfg(target_os = "macos")]
        let fda_status = crate::platform::macos::has_full_disk_access();
        #[cfg(not(target_os = "macos"))]
        let fda_status = true;

        let show_fda_onboarding = if cfg!(target_os = "macos") {
            !fda_status && !settings.macos_fda_dismissed
        } else {
            false
        };

        Self {
            language: settings.language,
            settings,
            view: View::Dashboard,
            status: bilingual(|l| tr_status_ready(l).to_string()),
            live: Arc::new(AtomicBool::new(true)),
            elevated: is_elevated(),
            confirm: None,
            tick_task: None,
            anim_phase: 0,
            cursor_blink_visible: true,
            cursor_blink_task: None,
            cursor_blink_wanted: false,
            fda_status,
            show_fda_onboarding,

            junk: JunkState {
                categories: Vec::new(),
                scanned: false,
                scanning: false,
                scan_task: None,
                discover_task: None,
                discovering: false,
                gen: 0,
                selected: HashSet::new(),
                expanded: HashSet::new(),
                scroll: CategoryId::ALL
                    .iter()
                    .map(|&c| (c, gpui::UniformListScrollHandle::new()))
                    .collect(),
                scroll_drag: None,
            },

            apps: AppsState {
                list: Vec::new(),
                scanned: false,
                scanning: false,
                task: None,
                sort: AppSortState::default(),
                preset: AppFilterPreset::All,
                search: String::new(),
                search_sel: 0..0,
                search_marked: None,
                search_bounds: None,
                text_hit: None,
                gen: 0,
                view: Vec::new(),
                view_key: None,
                scroll: gpui::UniformListScrollHandle::new(),
                scroll_drag: None,
                text_drag: None,
                focus_handle: apps_focus_handle,
                context_menu: None,
            },

            residual: ResidualState {
                result: None,
                scanning: false,
                task: None,
                selected: HashSet::new(),
                uninstall: None,
            },

            disk: DiskState {
                mft: None,
                scanning: false,
                error: None,
                task: None,
                volumes,
                volume: disk_volume,
                tab: DiskTab::Tree,
                path: vec![crate::core::disk::ROOT_NODE],
                sel: DiskSelectionState::new(),
                space: disk_space,
                volume_spaces: volume_spaces.clone(),
                rows: Vec::new(),
                rows_key: None,
                volume_menu_open: false,
                gen: 0,
            },

            clean: CleanState {
                running: false,
                progress: None,
                task: None,
                freed_total: 0,
                last_failed: Vec::new(),
                last_failed_files: 0,
                show_failed_details: false,
            },

            declutter: DeclutterState::default(),

            search: SearchState {
                query: String::new(),
                sel: 0..0,
                marked: None,
                bounds: None,
                text_hit: None,
                focus_handle: search_focus_handle,
                results: Vec::new(),
                indexing: false,
                index_task: None,
                #[cfg(windows)]
                indices: Vec::new(),
                text_drag: None,
                scroll: gpui::UniformListScrollHandle::new(),
                scroll_drag: None,
                search_task: None,
                search_generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
                is_searching: false,
                group_by_kind: true,
                sort_col: SearchSortCol::Size,
                sort_asc: false,
                gen: 0,
            },

            #[cfg(not(windows))]
            macos_root_index: None,
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
        self.apps.gen += 1;
        self.disk.gen += 1;
        self.settings.language = lang;
        self.settings.save();
        cx.notify();
    }

    /// 手选路径当前的处置方式。
    ///
    /// 只影响磁盘透镜里用户点名的路径；分类清理永远是永久删除。
    pub fn disposal(&self) -> Disposal {
        if self.settings.delete_to_recycle_bin {
            Disposal::RecycleBin
        } else {
            Disposal::Permanent
        }
    }

    /// 切换「删除到回收站」，并立刻落盘。
    pub fn toggle_recycle_bin(&mut self, cx: &mut Context<Self>) {
        self.settings.delete_to_recycle_bin = !self.settings.delete_to_recycle_bin;
        self.settings.save();
        cx.notify();
    }

    /// 打开完全磁盘访问权限引导弹窗。
    pub fn open_fda_guide(&mut self, cx: &mut Context<Self>) {
        self.show_fda_onboarding = true;
        cx.notify();
    }

    /// 关闭完全磁盘访问权限引导弹窗。
    ///
    /// 用户点「稍后」走到这里：选择不带 FDA 继续使用。若此时还没扫过垃圾，
    /// 就触发首次扫描——启动时为了不触发 TCC 弹窗刻意跳过了，这里补上。
    pub fn close_fda_guide(&mut self, cx: &mut Context<Self>) {
        self.show_fda_onboarding = false;
        if !self.junk.scanned && !self.junk.scanning && !self.clean.running {
            self.start_scan(cx);
        } else {
            cx.notify();
        }
    }

    /// 切换「不再自动弹出完全磁盘访问权限引导」，并立刻落盘。
    pub fn toggle_fda_dismissed(&mut self, cx: &mut Context<Self>) {
        self.settings.macos_fda_dismissed = !self.settings.macos_fda_dismissed;
        self.settings.save();
        cx.notify();
    }

    /// 重新检查完全磁盘访问权限。
    pub fn check_fda_permission(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        {
            let granted = crate::platform::macos::has_full_disk_access();
            self.fda_status = granted;
            if granted {
                self.show_fda_onboarding = false;
                self.status = bilingual(|l| tr_fda_check_success(l).to_string());
                // 无论之前是否扫过，获得 FDA 后都（重新）触发一次扫描：
                // 首次启动时为了不触发 TCC 弹窗跳过了扫描，这里补上；
                // 已扫过的话则重新扫以扫出刚解锁的 Safari 缓存等。
                if !self.junk.scanning && !self.clean.running {
                    self.start_scan(cx);
                }
            } else {
                self.status = bilingual(|l| tr_fda_check_failed(l).to_string());
                cx.notify();
            }
        }
        // 非 macOS 平台没有 FDA 概念，显式消费 cx 以避免 clippy unused_variables
        #[cfg(not(target_os = "macos"))]
        {
            let _ = cx;
        }
    }

    pub fn open_app_context_menu(&mut self, app: InstalledApp, x: f32, y: f32) {
        self.apps.context_menu = Some(AppsContextMenu { app, x, y });
    }

    pub fn close_context_menu(&mut self) {
        self.apps.context_menu = None;
    }

    pub fn open_declutter_context_menu(&mut self, path: PathBuf, filename: String, x: f32, y: f32) {
        self.declutter.context_menu = Some(DeclutterContextMenu {
            path,
            filename,
            x,
            y,
        });
    }

    pub fn close_declutter_context_menu(&mut self) {
        self.declutter.context_menu = None;
    }

    pub fn toggle_disk_volume_menu(&mut self) {
        self.disk.volume_menu_open = !self.disk.volume_menu_open;
    }

    pub fn close_disk_volume_menu(&mut self) {
        self.disk.volume_menu_open = false;
    }
    pub fn total_cleanable(&self) -> u64 {
        self.junk.total_cleanable()
    }

    pub fn items(&self) -> impl Iterator<Item = &ScanItem> {
        self.junk.items()
    }

    pub fn select_recommended(&mut self) {
        self.junk.select_recommended();
    }

    pub fn select_every(&mut self) {
        self.junk.select_every();
    }

    pub fn select_none(&mut self) {
        self.junk.select_none();
    }

    pub fn invert_selection(&mut self) {
        self.junk.invert_selection();
    }

    pub fn selection_is_recommended(&self) -> bool {
        self.junk.selection_is_recommended()
    }

    pub fn set_category_selected(&mut self, id: CategoryId, on: bool) {
        self.junk.set_category_selected(id, on);
    }

    pub fn apply_clean_result(&mut self, attempted: &[PathBuf], failed: &[PathBuf]) {
        self.junk.apply_clean_result(attempted, failed);
    }

    pub fn total_item_count(&self) -> usize {
        self.junk.total_item_count()
    }

    pub fn selected_paths(&self) -> Vec<PathBuf> {
        self.junk.selected_paths()
    }

    pub fn selected_targets(&self) -> Vec<CleanTarget> {
        self.junk.selected_targets()
    }

    pub fn selected_size(&self) -> u64 {
        self.junk.selected_size()
    }

    pub fn selected_count(&self) -> usize {
        self.junk.selected_count()
    }

    pub fn failures_need_admin(&self) -> bool {
        if self.elevated || self.clean.last_failed.is_empty() {
            return false;
        }
        let win = std::env::var("SystemRoot")
            .unwrap_or_else(|_| r"C:\Windows".into())
            .to_ascii_lowercase();
        self.clean.last_failed.iter().any(|p| {
            let s = p.to_string_lossy().to_ascii_lowercase().replace('/', "\\");
            s.starts_with(&win) || s.contains(r"\program files") || s.contains(r"\programdata")
        })
    }

    pub fn cat_check(&self, c: &CategorySummary) -> Check {
        self.junk.category_check(c)
    }

    pub fn toggle_category(&mut self, id: CategoryId) {
        self.junk.toggle_category(id);
    }

    pub fn toggle_expand(&mut self, id: CategoryId) {
        self.junk.toggle_expand(id);
    }

    pub fn toggle_item(&mut self, path: &std::path::Path) {
        self.junk.toggle_item(path);
    }
    pub fn current_disk_full_path(&self) -> Option<PathBuf> {
        let mft = self.disk.mft.as_ref()?;
        let cur = *self.disk.path.last().unwrap_or(&mft.tree.root());
        Some(PathBuf::from(mft.tree.path_of(cur)))
    }

    // ---- 磁盘勾选：全部委托给 core::disk::DiskSelectionState ----
    // 这套「父级继承 + 子项排除」的逻辑曾经在这里和 core 各写了一份，
    // 两边已经开始出现行为差异，现在只保留 core 那份。

    pub fn is_disk_item_selected(&self, path: &std::path::Path) -> bool {
        self.disk.sel.is_selected(path)
    }

    pub fn toggle_disk_item(&mut self, path: &std::path::Path, size: u64) {
        self.disk.sel.toggle(path, size);
    }

    pub fn clear_disk_selection(&mut self) {
        self.disk.sel.clear();
    }

    pub fn disk_selected_size(&self) -> u64 {
        self.disk.sel.total_size()
    }

    pub fn disk_selected_count(&self) -> usize {
        self.disk.sel.len()
    }

    pub fn is_busy(&self) -> bool {
        self.clean.running
            || self.junk.scanning
            || self.junk.discovering
            || self.apps.scanning
            || self.disk.scanning
            || self.residual.scanning
    }

    pub fn start_tick(&mut self, cx: &mut Context<Self>) {
        if self.tick_task.is_some() {
            return;
        }
        self.tick_task = Some(cx.spawn(async move |this, cx| loop {
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
        }));
    }

    /// 唤醒输入框光标（立即设为可见，并确保闪烁定时任务运行）。
    pub fn poke_cursor_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_blink_visible = true;
        cx.notify();
        self.ensure_cursor_blink(cx);
    }

    /// 确保光标闪烁任务正在运行（以 530ms 频率翻转光标可见性）。
    ///
    /// 任务在焦点离开输入框后自行退出（见 `cursor_blink_wanted`）；下次
    /// 输入框重新拿到焦点时 `render` 会再把它拉起来。
    pub fn ensure_cursor_blink(&mut self, cx: &mut Context<Self>) {
        if self.cursor_blink_task.is_some() {
            return;
        }
        self.cursor_blink_visible = true;
        self.cursor_blink_task = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(530))
                .await;
            let should_continue = this
                .update(cx, |this, cx| {
                    if !this.cursor_blink_wanted {
                        return false;
                    }
                    this.cursor_blink_visible = !this.cursor_blink_visible;
                    cx.notify();
                    true
                })
                .unwrap_or(false);

            if !should_continue {
                this.update(cx, |this, _| {
                    this.cursor_blink_task = None;
                })
                .ok();
                break;
            }
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
        let Some(scan) = &self.disk.mft else {
            self.disk.rows.clear();
            self.disk.rows_key = None;
            return;
        };
        let tree = &scan.tree;
        let cur = *self.disk.path.last().unwrap_or(&tree.root());
        let key: DiskRowsKey = (
            self.disk.volume.display().to_string(),
            cur,
            self.disk.tab,
            self.disk.gen,
        );
        if self.disk.rows_key == Some(key.clone()) {
            return;
        }

        let nodes = match self.disk.tab {
            DiskTab::Tree => tree.children(cur),
            DiskTab::Files => tree.largest_files(DISK_MAX_ROWS),
        };

        // 整批共用一个路径缓存，父链只回溯一次
        let mut path_cache = std::collections::HashMap::new();
        self.disk.rows = nodes
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
        self.disk.rows_key = Some(key);
    }

    fn refresh_apps_view(&mut self) {
        // 逐字段比对而不是「先造 key 再比」：造 key 要克隆搜索串，而这个
        // 函数每帧都跑，扫描期间是 20fps。命中缓存时（绝大多数帧）现在
        // 一次分配都没有，只在真的失效时才克隆一次。
        let hit = self.apps.view_key.as_ref().is_some_and(|k| {
            k.0 == self.apps.gen
                && k.1 == self.apps.preset
                && k.2 == self.apps.search
                && k.3 == self.apps.sort
        });
        if hit {
            return;
        }

        self.apps.view = filter_and_sort_apps(
            &self.apps.list,
            self.apps.preset,
            &self.apps.search,
            self.apps.sort,
        );
        self.apps.view_key = Some((
            self.apps.gen,
            self.apps.preset,
            self.apps.search.clone(),
            self.apps.sort,
        ));
    }

    /// 当前视图里**可勾选**的 (路径, 体积) 列表。
    ///
    /// 受保护项虽然显示在列表里，但不参与勾选，也不能被「全选」带上，
    /// 否则表头复选框永远到不了全选状态。
    pub fn disk_selectable(&self) -> Vec<(PathBuf, u64)> {
        self.disk
            .rows
            .iter()
            .filter(|r| !r.protected)
            .map(|r| (r.path.clone(), r.node.size))
            .collect()
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh_render_caches();

        self.cursor_blink_wanted = self.search.focus_handle.is_focused(window)
            || self.apps.focus_handle.is_focused(window);
        if self.cursor_blink_wanted {
            self.ensure_cursor_blink(cx);
        }

        let content = match self.view {
            View::Dashboard => render_dashboard_view(self, cx),
            View::Junk => render_junk_view(self, cx),
            View::Apps if self.residual.uninstall.is_some() => render_uninstall_progress(self),
            View::Apps => render_apps_view(self, window, cx),
            View::Disk => render_disk_view(self, cx),
            View::Declutter => render_declutter_view(self, cx),
            View::Search => render_search_view(self, window, cx),
        };

        let mut main = div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .bg(rgb(CARD))
            .flex()
            .flex_col()
            .child(render_top_bar(self, cx));

        let is_scanning = match self.view {
            View::Dashboard | View::Junk => self.junk.scanning,
            View::Apps => {
                self.residual.uninstall.is_none() && (self.apps.scanning || self.residual.scanning)
            }
            View::Disk => self.disk.scanning,
            View::Declutter => self.declutter.scanning,
            View::Search => self.search.indexing,
        };

        if is_scanning {
            main = main.child(render_scan_line());
        }

        main = main.child(div().flex_1().min_h(px(0.)).flex().child(content));

        if self.clean.running {
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

        if let Some(fda_modal) = render_fda_onboarding_modal(self, cx) {
            root = root.child(fda_modal);
        }

        if let Some(menu) = render_apps_context_menu(self, cx) {
            root = root.child(menu);
        }

        if let Some(declutter_menu) = render_declutter_context_menu(self, cx) {
            root = root.child(declutter_menu);
        }

        if let Some(dropdown) = render_disk_volume_dropdown(self, cx) {
            root = root.child(dropdown);
        }

        root.into_any_element()
    }
}
