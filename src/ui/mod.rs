//! GPUI 界面根视图与状态管理

pub mod components;
pub mod i18n;
pub mod text_input;
pub mod theme;
pub mod views;

use crate::core::apps::filter_and_sort_apps;
use crate::core::apps::{
    AppFilterPreset, AppSortState, InstalledApp, ResidualItem, ResidualScanResult,
};
use crate::core::categories::{all_targets, CategoryId};
use crate::core::cleaner::{
    clean_arbitrary, clean_targets, CleanProgress, CleanReport, CleanSnapshot, CleanTarget,
    Disposal,
};
use crate::core::disk::{DiskSelectionState, Node, ScanResult, VolumeId};
use crate::core::i18n::{bilingual, Language, Text};
use crate::core::model::{fmt_size, fmt_size_si, Check};
use crate::core::safety::is_protected;
#[cfg(windows)]
use crate::core::scanner::dominant_volume;
#[cfg(windows)]
use crate::core::scanner::scan_discovered;
#[cfg(not(windows))]
use crate::core::scanner::scan_discovered_arc;
use crate::core::scanner::{
    apply_clean_result, merge_discovered, scan_fixed, scan_fixed_with_tree, CategorySummary,
    ScanItem,
};
use crate::core::settings::Settings;
use crate::platform::scan_volume;
use crate::platform::{
    clean_residuals, get_volume_space, is_elevated, list_installed_apps, list_volumes,
    run_uninstaller_and_wait, scan_residuals, verify_residuals,
};
// `components` 与 `views` 导出的名字没有共同前缀（`card` / `checkbox` /
// `render_donut`），glob 进来就看不出谁是谁，所以显式列出。
// `i18n::*`（清一色 `tr_` 前缀）和 `theme::*`（清一色大写色值常量）保持
// glob——那两个是「命名空间即词汇表」，逐个列反而更难读。
use crate::ui::components::{
    render_confirm_dialog, render_progress_bar, render_residual_modal, render_scan_line,
    render_sidebar, render_top_bar, ConfirmKind, ConfirmRequest, View,
};
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::views::{
    render_apps_context_menu, render_apps_view, render_clean_bar, render_dashboard_view,
    render_disk_clean_bar, render_disk_view, render_disk_volume_dropdown, render_junk_view,
    DiskTab,
};

use gpui::{div, prelude::*, px, rgb, Context, IntoElement, Render, Task, Window};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// 智能清理页的状态。
pub struct JunkState {
    pub categories: Vec<CategorySummary>,
    pub scanned: bool,
    pub scanning: bool,
    pub scan_task: Option<Task<()>>,
    /// 第二阶段（构建产物检索）的任务槽。它比第一阶段慢一个数量级，
    /// 必须独立持有，否则会和第一阶段互相顶掉句柄。
    pub discover_task: Option<Task<()>>,
    /// 第二阶段是否还在跑。界面靠它给开发者类目显示「检索中」。
    pub discovering: bool,
    /// 每发起一轮扫描就自增。第二阶段回来时用它判断「我属于的那轮扫描
    /// 是不是已经被新的一轮顶掉了」，避免把过期结果并进新数据。
    pub gen: u64,
    pub selected: HashSet<PathBuf>,
    pub expanded: HashSet<CategoryId>,
    /// 每个分类展开后各自的滚动位置。「项目构建产物」这类可能有近千条，
    /// 必须走虚拟化列表，而 uniform_list 需要一个长期持有的滚动句柄。
    pub scroll: std::collections::HashMap<CategoryId, gpui::UniformListScrollHandle>,
    /// 正在拖拽哪个分类的滚动条滑块：(分类, 按下时鼠标 y, 按下时滚动偏移)
    pub scroll_drag: Option<(CategoryId, f32, f32)>,
}

impl JunkState {
    /// 全部条目（跨类目铺平）。
    pub fn items(&self) -> impl Iterator<Item = &ScanItem> {
        self.categories.iter().flat_map(|c| c.items.iter())
    }

    pub fn total_cleanable(&self) -> u64 {
        self.categories.iter().map(|c| c.total_size).sum()
    }

    pub fn total_item_count(&self) -> usize {
        self.items().count()
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
    /// 用来给工具栏上的「推荐」按钮做选中态高亮，让用户一眼看出自己
    /// 是不是还停在默认状态。
    ///
    /// 最后那句 `n == self.selected.len()` 不是多余的：勾选集合里可能
    /// 残留着已经不在扫描结果里的路径（清理完成后就地更新过），
    /// 光比对每个条目发现不了这种多出来的。
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

    pub fn selected_paths(&self) -> Vec<PathBuf> {
        self.selected_items().map(|i| i.path.clone()).collect()
    }

    /// 勾选项连同各自的处置方式（整个删掉还是只清空内容）。
    pub fn selected_targets(&self) -> Vec<CleanTarget> {
        self.selected_items()
            .map(|i| CleanTarget {
                path: i.path.clone(),
                remove_dir: i.category.removes_directory(),
            })
            .collect()
    }

    pub fn selected_size(&self) -> u64 {
        self.selected_items().map(|i| i.size).sum()
    }

    pub fn selected_file_count(&self) -> u64 {
        self.selected_items().map(|i| i.file_count).sum()
    }

    pub fn selected_count(&self) -> usize {
        self.selected_items().count()
    }

    fn selected_items(&self) -> impl Iterator<Item = &ScanItem> {
        self.items().filter(|i| self.selected.contains(&i.path))
    }

    /// 某个类目的勾选态：全选 / 部分 / 未选。
    pub fn category_check(&self, c: &CategorySummary) -> Check {
        let n = c
            .items
            .iter()
            .filter(|i| self.selected.contains(&i.path))
            .count();
        Check::from_counts(n, c.items.len())
    }

    /// 点类目标题上的复选框：全选状态下取消整组，否则补齐整组。
    pub fn toggle_category(&mut self, id: CategoryId) {
        let Some(c) = self.categories.iter().find(|c| c.category == id) else {
            return;
        };
        let paths: Vec<PathBuf> = c.items.iter().map(|i| i.path.clone()).collect();
        if self.category_check(c) == Check::On {
            for p in &paths {
                self.selected.remove(p);
            }
        } else {
            for p in paths {
                self.selected.insert(p);
            }
        }
    }

    /// 展开 / 收起某个类目。
    pub fn toggle_expand(&mut self, id: CategoryId) {
        if !self.expanded.remove(&id) {
            self.expanded.insert(id);
        }
    }

    /// 勾选 / 取消单个条目。
    pub fn toggle_item(&mut self, path: &Path) {
        let pb = path.to_path_buf();
        if !self.selected.remove(&pb) {
            self.selected.insert(pb);
        }
    }
}

/// 软件管理页的状态（Geek Uninstaller 风格）。
pub struct AppsState {
    pub list: Vec<InstalledApp>,
    pub scanned: bool,
    pub scanning: bool,
    pub task: Option<Task<()>>,
    pub sort: AppSortState,
    pub preset: AppFilterPreset,
    pub search: String,
    /// 搜索框光标/选区的**字节**范围
    pub search_sel: std::ops::Range<usize>,
    /// 输入法正在组合中的那段文本的字节范围（拼音串，尚未确认）
    pub search_marked: Option<std::ops::Range<usize>>,
    /// 搜索框最近一次绘制的位置，用来定位输入法候选窗口
    pub search_bounds: Option<gpui::Bounds<gpui::Pixels>>,
    /// 软件表每次被整体替换就自增，用来判定渲染缓存是否失效
    pub gen: u64,
    /// 过滤 + 排序后的 `list` 下标，渲染直接读这里
    pub view: Vec<usize>,
    pub(super) view_key: Option<AppsViewKey>,
    /// 软件表也走虚拟化列表，句柄需长期持有
    pub scroll: gpui::UniformListScrollHandle,
    pub scroll_drag: Option<(f32, f32)>,
    pub focus_handle: gpui::FocusHandle,
    pub context_menu: Option<AppsContextMenu>,
}

/// 深度卸载的残留扫描状态。
pub struct ResidualState {
    pub result: Option<ResidualScanResult>,
    pub scanning: bool,
    pub task: Option<Task<()>>,
    pub selected: HashSet<usize>,
}

/// 磁盘透镜（Disk Lens 空间分析）的状态。
pub struct DiskState {
    /// 用 Arc 共享，macOS 上避免从缓存索引克隆 6.6M 条目的 SizeTree。
    pub mft: Option<std::sync::Arc<ScanResult>>,
    pub scanning: bool,
    /// 保留错误值本身而不是渲染好的字符串：错误卡片会一直挂在界面上，
    /// 用户中途切语言时它也得跟着变。
    pub error: Option<crate::core::disk::ScanError>,
    pub task: Option<Task<()>>,
    pub volumes: Vec<VolumeId>,
    pub volume: VolumeId,
    pub tab: DiskTab,
    pub path: Vec<u32>,
    /// 磁盘透镜的勾选状态（含继承与局部排除），实现见 `core::disk`
    pub sel: DiskSelectionState,
    pub space: Option<(u64, u64)>,
    /// 当前目录（或最大文件列表）的渲染行缓存
    pub rows: Vec<DiskRow>,
    pub(super) rows_key: Option<DiskRowsKey>,
    /// 磁盘切换下拉浮层菜单是否展开
    pub volume_menu_open: bool,
    /// MFT 树每次被替换或就地修改就自增
    pub gen: u64,
}

/// 正在执行的清理任务及其结果。
pub struct CleanState {
    pub running: bool,
    pub progress: Option<Arc<CleanProgress>>,
    /// 清理任务独占的槽位。以前清理任务会借用 scan_task / mft_task，
    /// 一旦清理和扫描重叠就会互相顶掉对方的句柄。
    pub task: Option<Task<()>>,
    pub freed_total: u64,
    pub last_failed: Vec<PathBuf>,
    pub last_failed_files: u64,
    pub show_failed_details: bool,
}

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

    pub junk: JunkState,
    pub apps: AppsState,
    pub residual: ResidualState,
    pub disk: DiskState,
    pub clean: CleanState,
    /// macOS 用户目录索引缓存。垃圾扫描阶段加载/构建后存在这里。
    /// 用 Arc 共享，避免克隆 6.6M 条目的 SizeTree。
    #[cfg(not(windows))]
    pub macos_index: Option<std::sync::Arc<crate::core::disk::ScanResult>>,
    /// macOS 整盘索引缓存（磁盘透镜显示 `/` 时用）。
    #[cfg(not(windows))]
    pub macos_root_index: Option<std::sync::Arc<crate::core::disk::ScanResult>>,
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
type DiskRowsKey = (String, u32, DiskTab, u64);

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
        let disk_space = get_volume_space(&disk_volume);
        let apps_focus_handle = cx.focus_handle();
        // 有配置文件就照配置文件，没有就按系统显示语言（中文系统用中文，其余英文）
        let settings = Settings::load();
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
                gen: 0,
                view: Vec::new(),
                view_key: None,
                scroll: gpui::UniformListScrollHandle::new(),
                scroll_drag: None,
                focus_handle: apps_focus_handle,
                context_menu: None,
            },

            residual: ResidualState {
                result: None,
                scanning: false,
                task: None,
                selected: HashSet::new(),
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

            #[cfg(not(windows))]
            macos_index: None,
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

    pub fn open_app_context_menu(&mut self, app: InstalledApp, x: f32, y: f32) {
        self.apps.context_menu = Some(AppsContextMenu { app, x, y });
    }

    pub fn close_context_menu(&mut self) {
        self.apps.context_menu = None;
    }

    pub fn toggle_disk_volume_menu(&mut self) {
        self.disk.volume_menu_open = !self.disk.volume_menu_open;
    }

    pub fn close_disk_volume_menu(&mut self) {
        self.disk.volume_menu_open = false;
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
        if self.junk.scanning {
            return;
        }
        // 通知上一轮（可能还在跑的第二阶段）停下
        self.live.store(false, Ordering::Relaxed);
        self.junk.scan_task.take();
        self.junk.discover_task.take();

        self.junk.gen += 1;
        let gen = self.junk.gen;
        self.junk.scanning = true;
        self.junk.scanned = false;
        self.junk.discovering = false;
        self.status = bilingual(|l| tr_status_scanning(l).to_string());
        let live = Arc::new(AtomicBool::new(true));
        self.live = live.clone();
        self.start_tick(cx);
        cx.notify();

        let targets = all_targets();
        // 提权时先解析目标最集中的那个卷的 $MFT，阶段一在树上查表而不是
        // 遍历目录。看着是给首屏多加了一步，实测反而更快：本机 MFT 解析
        // 3.3 秒，而遍历要 4.1~4.9 秒——阶段一的瓶颈是 `go\pkg\mod`、
        // `npm-cache` 这类几十万个小文件的目录，每一个都要几秒，而它们的
        // 递归体积在 MFT 树里查一次表就有。
        //
        // 解析出来的树随后原样交给阶段二，一次解析两个阶段用，省掉第二次
        // 全盘解析。内存峰值不变——阶段二本来也要在内存里放一棵树。
        // Windows 上读 $MFT 需要管理员权限，未提权时跳过预扫描。
        //
        // macOS：先加载/构建用户目录索引，阶段一在树上查表（毫秒级），
        // 阶段二在树上 DFS。索引复用后首次启动和后续启动都受益。
        #[cfg(windows)]
        let prescan_volume = if is_elevated() {
            dominant_volume(&targets)
        } else {
            None
        };
        #[cfg(not(windows))]
        let prescan_volume: Option<VolumeId> = None;
        let scan = cx.background_executor().spawn(async move {
            #[cfg(windows)]
            let pre = prescan_volume.and_then(|v| scan_volume(&v, 0).ok());
            #[cfg(not(windows))]
            let pre = {
                let _ = prescan_volume; // 消除未使用变量警告
                crate::core::devscan::load_or_build_macos_index(&live)
            };
            let cats = match &pre {
                Some(s) => scan_fixed_with_tree(&targets, &live, &s.tree),
                None => scan_fixed(&targets, &live),
            };
            (cats, pre)
        });
        self.junk.scan_task = Some(cx.spawn(async move |this, cx| {
            let (result, prescanned) = scan.await;
            this.update(cx, |this, cx| {
                this.junk.categories = result;
                this.junk.scanned = true;
                this.junk.scanning = false;
                this.select_recommended();
                let total_str = fmt_size(this.total_cleanable());
                this.status = bilingual(|l| tr_status_scan_fixed_done(l, &total_str));
                // macOS：load_or_build_macos_index 已返回 Arc<ScanResult>，
                // 存一份给磁盘透镜复用，另一份（Arc clone）交给 start_discovery。
                #[cfg(not(windows))]
                {
                    this.macos_index = prescanned.clone();
                    this.start_discovery_arc(gen, prescanned, cx);
                }
                #[cfg(windows)]
                {
                    this.start_discovery(gen, prescanned, cx);
                }
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
    #[cfg(windows)]
    fn start_discovery(
        &mut self,
        gen: u64,
        prescanned: Option<crate::core::disk::ScanResult>,
        cx: &mut Context<Self>,
    ) {
        self.junk.discovering = true;
        let live = self.live.clone();
        let discover = cx
            .background_executor()
            .spawn(async move { scan_discovered(&live, prescanned) });

        self.junk.discover_task = Some(cx.spawn(async move |this, cx| {
            let items = discover.await;
            this.update(cx, |this, cx| {
                if this.junk.gen != gen {
                    return;
                }
                this.junk.discovering = false;
                merge_discovered(&mut this.junk.categories, items);
                let total_str = fmt_size(this.total_cleanable());
                this.status = bilingual(|l| tr_status_scan_done(l, &total_str));
                cx.notify();
            })
            .ok();
        }));
    }

    /// macOS 专用：接受 `Arc<ScanResult>` 的 start_discovery 变体。
    /// 避免从 prescanned 中 clone 6.6M 条目的 ScanResult。
    #[cfg(not(windows))]
    fn start_discovery_arc(
        &mut self,
        gen: u64,
        prescanned: Option<std::sync::Arc<crate::core::disk::ScanResult>>,
        cx: &mut Context<Self>,
    ) {
        self.junk.discovering = true;
        let live = self.live.clone();
        let discover = cx
            .background_executor()
            .spawn(async move { scan_discovered_arc(&live, prescanned) });

        self.junk.discover_task = Some(cx.spawn(async move |this, cx| {
            let items = discover.await;
            this.update(cx, |this, cx| {
                if this.junk.gen != gen {
                    return;
                }
                this.junk.discovering = false;
                merge_discovered(&mut this.junk.categories, items);
                let total_str = fmt_size(this.total_cleanable());
                this.status = bilingual(|l| tr_status_scan_done(l, &total_str));
                cx.notify();
            })
            .ok();
        }));
    }

    // ---- 智能清理：转发给 JunkState，逻辑与测试都在那边 ----

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

    pub fn switch_disk_volume(&mut self, vol: VolumeId, cx: &mut Context<Self>) {
        self.disk.volume_menu_open = false;
        if self.disk.volume == vol && (self.disk.mft.is_some() || self.disk.scanning) {
            return;
        }
        self.disk.volume = vol;
        self.disk.mft = None;
        self.disk.rows.clear();
        self.disk.rows_key = None;
        self.disk.path = vec![crate::core::disk::ROOT_NODE];
        self.disk.sel.clear();
        self.start_mft_scan(cx);
    }

    pub fn start_mft_scan(&mut self, cx: &mut Context<Self>) {
        if self.disk.scanning {
            return;
        }
        self.disk.scanning = true;
        self.disk.error = None;
        let vol = self.disk.volume.clone();
        self.disk.space = get_volume_space(&vol);
        self.disk.sel.clear();
        let saved_path = self.current_disk_full_path();
        self.status = bilingual(|l| tr_status_disk_scanning(l, &vol));
        self.start_tick(cx);
        cx.notify();

        // macOS：磁盘透镜根据所选卷加载不同索引。
        // 主卷 `/`：加载/构建整盘索引，首次可能需要 1-2 分钟。
        // 其他卷：直接扫描。
        // Windows：仍然走 scan_volume 解析 $MFT。
        #[cfg(not(windows))]
        let cached_root_index = self.macos_root_index.clone();
        let scan = cx.background_executor().spawn(async move {
            #[cfg(windows)]
            {
                scan_volume(&vol, 0).map(std::sync::Arc::new)
            }
            #[cfg(not(windows))]
            {
                let is_root = vol.mount_point() == std::path::Path::new("/");
                if is_root {
                    if let Some(scan) = cached_root_index {
                        crate::log!("磁盘透镜复用已缓存整盘索引：{} 条记录", scan.records_read);
                        Ok(scan)
                    } else {
                        let t0 = std::time::Instant::now();
                        let live = std::sync::atomic::AtomicBool::new(true);
                        let result =
                            match crate::core::devscan::load_or_build_macos_root_index(&live) {
                                Some(scan) => {
                                    crate::log!("磁盘透镜加载整盘索引：{:?}", t0.elapsed());
                                    Ok(scan)
                                }
                                None => Err(crate::core::disk::ScanError::Io(
                                    "无法加载或构建整盘索引".into(),
                                )),
                            };
                        result
                    }
                } else {
                    let t0 = std::time::Instant::now();
                    let result = scan_volume(&vol, 0).map(std::sync::Arc::new);
                    crate::log!("磁盘透镜扫描外接卷 {}：{:?}", vol.display(), t0.elapsed());
                    result
                }
            }
        });

        let vol_for_task = self.disk.volume.clone();
        self.disk.task = Some(cx.spawn(async move |this, cx| {
            let result = scan.await;
            this.update(cx, |this, cx| {
                this.disk.scanning = false;
                match result {
                    Ok(s) => {
                        // 磁盘总占用用 statfs 的「总量-空闲」，不用 SizeTree 累加。
                        // APFS 快照/克隆/硬链接会导致「所有文件大小相加」超过物理容量。
                        // 使用 1000 进制（SI）与 macOS / CleanMyMac 对齐。
                        let used = this
                            .disk
                            .space
                            .map(|(total, free)| fmt_size_si(total - free))
                            .unwrap_or_else(|| fmt_size(s.total_size));
                        let files = s.file_count;
                        this.status = bilingual(|l| tr_status_disk_done(l, files, &used));
                        // 仅当 saved_path 确实属于当前卷时才尝试恢复层级；跨盘切换时直接回到新盘根目录
                        let is_same_vol = saved_path.as_ref().is_some_and(|p| {
                            p.to_string_lossy().starts_with(vol_for_task.display())
                        });
                        if is_same_vol {
                            if let Some(target_path) = saved_path {
                                let resolved = s.tree.find_path(&target_path);
                                this.disk.path = if resolved.is_empty() {
                                    vec![s.tree.root()]
                                } else {
                                    resolved
                                };
                            } else {
                                this.disk.path = vec![s.tree.root()];
                            }
                        } else {
                            this.disk.path = vec![s.tree.root()];
                        }
                        // 主卷 `/` 的整盘索引缓存起来，避免下次打开磁盘透镜重扫
                        #[cfg(not(windows))]
                        if vol_for_task.mount_point() == std::path::Path::new("/") {
                            this.macos_root_index = Some(s.clone());
                        }
                        this.disk.mft = Some(s);
                        this.disk.gen += 1;
                    }
                    Err(e) => {
                        this.status =
                            bilingual(|l| tr_status_disk_failed(l, &tr_scan_error(l, &e)));
                        this.disk.error = Some(e);
                        this.disk.mft = None;
                        this.disk.gen += 1;
                    }
                }
                cx.notify();
            })
            .ok();
        }));
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

    pub fn request_clean_disk_selected(&mut self, cx: &mut Context<Self>) {
        if self.disk.sel.is_empty() || self.clean.running {
            return;
        }
        let total_size = self.disk_selected_size();
        let count = self.disk.sel.len();
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
        if self.apps.scanning {
            return;
        }
        self.apps.scanning = true;
        self.apps.scanned = false;
        self.status = bilingual(|l| tr_status_apps_scanning(l).to_string());
        self.start_tick(cx);
        cx.notify();

        let live = Arc::new(AtomicBool::new(true));
        let scan = cx
            .background_executor()
            .spawn(async move { list_installed_apps(&live) });

        self.apps.task = Some(cx.spawn(async move |this, cx| {
            let result = scan.await;
            this.update(cx, |this, cx| {
                this.apps.list = result;
                this.apps.gen += 1;
                this.apps.scanned = true;
                this.apps.scanning = false;
                let total_size: u64 = this.apps.list.iter().map(|a| a.estimated_size).sum();
                let (count, size) = (this.apps.list.len(), fmt_size(total_size));
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
        if self.residual.scanning || self.clean.running {
            return;
        }
        let lang = self.language;
        let app_name = app.name.clone();
        let size_str = if app.estimated_size > 0 {
            format!(" ({})", fmt_size(app.estimated_size))
        } else {
            String::new()
        };

        let (title, body, detail) = match lang {
            Language::Zh => (
                format!("确认卸载「{app_name}」？"),
                if cfg!(target_os = "macos") {
                    format!("将把「{app_name}」{size_str} 移入废纸篓或调用自带卸载程序，完成后自动扫描并深度清理卸载残留。")
                } else {
                    format!("将启动「{app_name}」{size_str} 官方卸载程序，完成后自动扫描并深度清理卸载残留。")
                },
                "卸载完成后将自动唤起残留扫描，彻底清除关联配置与缓存。".to_string(),
            ),
            Language::En => (
                format!("Uninstall \"{app_name}\"?"),
                if cfg!(target_os = "macos") {
                    format!("This will move \"{app_name}\"{size_str} to Trash or run its uninstaller, followed by automatic residual cleanup.")
                } else {
                    format!("This will launch the official uninstaller for \"{app_name}\"{size_str}, followed by automatic residual cleanup.")
                },
                "Residual files and configurations will be automatically scanned and cleaned up afterwards.".to_string(),
            ),
        };

        self.confirm = Some(ConfirmRequest {
            title,
            body,
            detail,
            kind: ConfirmKind::UninstallApp(Box::new(app)),
        });
        cx.notify();
    }

    pub fn execute_uninstall_app(&mut self, app: InstalledApp, cx: &mut Context<Self>) {
        let name = app.name.clone();
        let pre_target = app.clone();
        let uninst_target = app.clone();

        self.residual.scanning = true;
        self.residual.result = None;
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

        self.residual.task = Some(cx.spawn(async move |this, cx| {
            let (result, remaining) = work.await;
            this.update(cx, |this, cx| {
                this.residual.scanning = false;
                let total: u64 = remaining.iter().map(|i| i.size()).sum();
                let res = ResidualScanResult {
                    app_name: name.clone(),
                    items: remaining,
                    total_file_size: total,
                };
                // 失败原因以前直接丢在这里——界面只显示「卸载失败」，
                // 日志里也没有任何线索。至少让它留下一行。
                if let Err(reason) = &result {
                    crate::log!("卸载「{name}」失败：{reason}");
                }
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
                this.residual.selected = res.default_selection();
                this.residual.result = Some(res);

                // 卸载由外部卸载器执行，我们不知道确切删了哪些路径，
                // 无法局部更新 SizeTree。失效磁盘透镜缓存，下次打开时
                // 走 FSEvents 增量更新。
                if ok {
                    this.disk.mft = None;
                    #[cfg(not(windows))]
                    {
                        this.macos_index = None;
                        this.macos_root_index = None;
                    }
                }

                cx.notify();
            })
            .ok();
        }));
    }

    pub fn start_residual_scan(&mut self, app: InstalledApp, cx: &mut Context<Self>) {
        if self.residual.scanning {
            return;
        }
        self.residual.scanning = true;
        self.residual.result = None;
        let scanning_name = app.name.clone();
        self.status = bilingual(|l| tr_status_residual_scanning(l, &scanning_name));
        cx.notify();

        let target = app.clone();
        let scan = cx
            .background_executor()
            .spawn(async move { scan_residuals(&target) });

        self.residual.task = Some(cx.spawn(async move |this, cx| {
            let res = scan.await;
            this.update(cx, |this, cx| {
                this.residual.scanning = false;
                let count = res.items.len();
                // 只预勾「确定」的；模糊匹配出来的交给用户自己判断
                this.residual.selected = res.default_selection();
                let (name, size) = (res.app_name.clone(), fmt_size(res.total_file_size));
                this.status = bilingual(|l| tr_status_residual_done(l, &name, count, &size));
                this.residual.result = Some(res);
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn clean_selected_residuals(&mut self, cx: &mut Context<Self>) {
        let Some(res) = self.residual.result.take() else {
            return;
        };
        let items_to_clean: Vec<ResidualItem> = self
            .residual
            .selected
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
        // 提取残留路径，用于清理后局部更新磁盘透镜
        let residual_paths: Vec<PathBuf> = items_to_clean
            .iter()
            .filter_map(|it| match &it.kind {
                crate::core::apps::ResidualKind::File(p, _)
                | crate::core::apps::ResidualKind::Directory(p, _) => Some(p.clone()),
                _ => None,
            })
            .collect();
        self.status = bilingual(|l| tr_status_residual_cleaning(l, &cleaning_name, cleaning_count));
        cx.notify();

        let clean = cx
            .background_executor()
            .spawn(async move { clean_residuals(&items_to_clean, &prog) });

        self.residual.task = Some(cx.spawn(async move |this, cx| {
            let report = clean.await;
            this.update(cx, |this, cx| {
                let snap = progress.snapshot();
                this.clean.freed_total += snap.bytes;
                this.residual.selected.clear();

                // 局部更新：软件确实被清干净时，直接把它从内存里的软件表
                // 摘掉，不再触发一轮完整的注册表枚举 + 全盘安装目录遍历。
                let removed = cleaned_everything && report.failed.is_empty();
                if removed {
                    this.apps.list.retain(|a| a.name != app_name);
                    this.apps.gen += 1;
                }

                // 同步更新磁盘透镜的 SizeTree：残留文件/目录在磁盘透镜里也显示，
                // 不局部扣减的话切过去看还是旧大小。
                let deleted: Vec<PathBuf> = residual_paths
                    .iter()
                    .filter(|p| !report.failed.contains(p))
                    .cloned()
                    .collect();
                this.prune_deleted_from_mft(&deleted, snap.bytes, cx);

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
        if count == 0 || self.clean.running || !self.junk.scanned {
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
        if self.clean.running {
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
            ConfirmKind::UninstallApp(app) => self.execute_uninstall_app(*app, cx),
        }
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

    pub fn clean_snapshot(&self) -> Option<CleanSnapshot> {
        self.clean.progress.as_ref().map(|p| p.snapshot())
    }

    pub fn cancel_clean(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = &self.clean.progress {
            p.request_cancel();
            self.status = bilingual(|l| tr_status_stopping(l).to_string());
            cx.notify();
        }
    }

    /// 三个清理入口共用的编排。
    ///
    /// 「置 cleaning → 建进度 → 写状态栏 → 起心跳 → 后台删 → 回主线程收尾」
    /// 这套仪式以前在 `start_clean` / `start_clean_path` /
    /// `start_clean_disk_selected` 里各抄了一遍，连「从内存 MFT 树剔除已删
    /// 路径」那段都是逐行重复的。三份实现漂移过一次（清理任务曾经借用
    /// scan_task 的槽位），收敛掉才不会有第二次。
    ///
    /// `work` 在后台线程上跑，`finish` 回到主线程收尾——差异全在这两个闭包里。
    fn spawn_clean(
        &mut self,
        totals: (u64, u64),
        status: Text,
        work: impl FnOnce(&CleanProgress) -> CleanReport + Send + 'static,
        finish: impl FnOnce(&mut Self, CleanReport, CleanSnapshot, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let (total_files, total_bytes) = totals;
        self.clean.running = true;
        let progress = Arc::new(CleanProgress::new(total_files, total_bytes));
        self.clean.progress = Some(progress.clone());
        self.status = status;
        self.start_tick(cx);
        cx.notify();

        let clean = cx
            .background_executor()
            .spawn(async move { work(&progress) });

        self.clean.task = Some(cx.spawn(async move |this, cx| {
            let report: CleanReport = clean.await;
            this.update(cx, |this, cx| {
                this.clean.running = false;
                let snap = this.clean_snapshot().unwrap_or_default();
                this.clean.freed_total += snap.bytes;
                finish(this, report, snap, cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// 删除后局部更新 SizeTree，无需全量重扫。
    ///
    /// 用 `Arc::make_mut` 获取可变引用，对每个已删路径调用
    /// `ScanResult::remove_path`：标记子树为 unused，沿父链扣减聚合大小。
    /// UI 立即看到目录消失和容量变化，当前位置保留不变。
    fn prune_deleted_from_mft(
        &mut self,
        deleted: &[PathBuf],
        freed_bytes: u64,
        _cx: &mut Context<Self>,
    ) {
        if let Some(mft) = &mut self.disk.mft {
            // Arc::make_mut：如果 Arc 是独占的则直接获取可变引用，
            // 否则克隆一份再修改——这里磁盘透镜的 Arc 通常独占，不会触发克隆。
            let mft_mut = std::sync::Arc::make_mut(mft);
            for path in deleted {
                mft_mut.remove_path(path);
            }
            self.disk.gen += 1;

            // 当前所在目录可能已被删除，沿 path 栈往回退到有效节点
            while self.disk.path.len() > 1 {
                let cur = *self.disk.path.last().unwrap();
                if mft_mut.tree.valid(cur) {
                    break;
                }
                self.disk.path.pop();
            }
        }
        if let Some((_, free)) = &mut self.disk.space {
            *free += freed_bytes;
        }
    }

    /// 智能清理页：删掉当前勾选的所有分类项。
    pub fn start_clean(&mut self, cx: &mut Context<Self>) {
        if self.clean.running || !self.junk.scanned {
            return;
        }
        let attempted = self.selected_paths();
        if attempted.is_empty() {
            self.status = bilingual(|l| tr_status_nothing_selected(l).to_string());
            cx.notify();
            return;
        }

        let total_files: u64 = self
            .items()
            .filter(|i| self.junk.selected.contains(&i.path))
            .map(|i| i.file_count)
            .sum();
        let totals = (total_files, self.selected_size());

        self.clean.last_failed.clear();
        let n = attempted.len();
        let targets = self.selected_targets();

        self.spawn_clean(
            totals,
            bilingual(|l| tr_status_deleting_n(l, n)),
            move |p| clean_targets(&targets, p),
            move |this, report, snap, cx| {
                this.clean.last_failed = report.failed;
                this.clean.last_failed_files = snap.failed;

                // 就地更新，不再触发整轮复扫（开发垃圾扫描要几十秒）
                let failed = this.clean.last_failed.clone();
                this.apply_clean_result(&attempted, &failed);

                // 同步更新磁盘透镜的 SizeTree：垃圾清理删掉的路径
                //（缓存、临时文件、构建产物）在磁盘透镜里也会显示，
                // 不局部扣减的话切过去看还是旧大小。
                let deleted: Vec<PathBuf> = attempted
                    .iter()
                    .filter(|p| !failed.contains(p))
                    .cloned()
                    .collect();
                this.prune_deleted_from_mft(&deleted, snap.bytes, cx);

                let fails = this.clean.last_failed.len();
                let (files, size) = (snap.files, fmt_size(snap.bytes));
                this.status = bilingual(|l| {
                    if fails > 0 {
                        tr_status_clean_done_partial(l, files, &size, fails)
                    } else {
                        tr_status_clean_done(l, files, &size)
                    }
                });
            },
            cx,
        );
    }

    /// 磁盘透镜：删掉单个用户点名的路径。
    pub fn start_clean_path(&mut self, path: PathBuf, size: u64, cx: &mut Context<Self>) {
        if self.clean.running {
            return;
        }
        let target = path.clone();
        let shown = path.display().to_string();
        let disposal = self.disposal();

        self.spawn_clean(
            (0, size),
            bilingual(|l| tr_status_deleting_path(l, &shown)),
            move |p| clean_arbitrary(std::slice::from_ref(&target), disposal, p),
            move |this, report, snap, cx| {
                let shown = path.display().to_string();
                if !report.failed.is_empty() {
                    this.status = bilingual(|l| tr_status_delete_failed(l, &shown));
                    return;
                }
                let (files, size) = (snap.files, fmt_size(snap.bytes));
                this.status = bilingual(|l| tr_status_deleted_path(l, &shown, files, &size));
                this.prune_deleted_from_mft(std::slice::from_ref(&path), snap.bytes, cx);
            },
            cx,
        );
    }

    /// 磁盘透镜：删掉当前勾选的一批路径。
    pub fn start_clean_disk_selected(&mut self, cx: &mut Context<Self>) {
        if self.clean.running || self.disk.sel.is_empty() {
            return;
        }
        // 展开成实际删除目标：勾选目录里若埋着被排除的子孙，会自动下钻绕开。
        let targets = self.disk.sel.resolve_targets();
        if targets.is_empty() {
            return;
        }
        let total_size = self.disk_selected_size();
        let n = targets.len();
        let to_clean = targets.clone();
        let disposal = self.disposal();

        self.spawn_clean(
            (0, total_size),
            bilingual(|l| tr_status_batch_deleting(l, n)),
            move |p| clean_arbitrary(&to_clean, disposal, p),
            move |this, report, snap, cx| {
                this.clear_disk_selection();

                let (files, fails, size) = (snap.files, report.failed.len(), fmt_size(snap.bytes));
                this.status = bilingual(|l| {
                    if fails == 0 {
                        tr_status_batch_done(l, files, &size)
                    } else {
                        tr_status_batch_done_partial(l, &size, fails)
                    }
                });

                // 只摘真正删成功的那些
                let deleted: Vec<PathBuf> = targets
                    .into_iter()
                    .filter(|t| !report.failed.contains(t))
                    .collect();
                this.prune_deleted_from_mft(&deleted, snap.bytes, cx);
            },
            cx,
        );
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

        if self.junk.scanning {
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

        if let Some(menu) = render_apps_context_menu(self, cx) {
            root = root.child(menu);
        }

        if let Some(dropdown) = render_disk_volume_dropdown(self, cx) {
            root = root.child(dropdown);
        }

        root.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::i18n::bilingual;

    fn item(path: &str, cat: CategoryId, size: u64, files: u64) -> ScanItem {
        ScanItem {
            path: PathBuf::from(path),
            label: bilingual(|_| path.to_string()),
            size,
            file_count: files,
            category: cat,
            last_modified: 0,
        }
    }

    /// 一个类目「推荐勾选」、一个类目「默认不勾」，覆盖两种策略。
    fn junk_fixture() -> JunkState {
        let recommended = CategoryId::ALL
            .iter()
            .copied()
            .find(|c| c.default_selected())
            .expect("至少要有一个默认勾选的类目");
        let opt_in = CategoryId::ALL
            .iter()
            .copied()
            .find(|c| !c.default_selected())
            .expect("至少要有一个默认不勾的类目");

        JunkState {
            categories: vec![
                CategorySummary {
                    category: recommended,
                    total_size: 300,
                    items: vec![
                        item(r"C:\rec\a", recommended, 100, 3),
                        item(r"C:\rec\b", recommended, 200, 5),
                    ],
                },
                CategorySummary {
                    category: opt_in,
                    total_size: 50,
                    items: vec![item(r"C:\opt\c", opt_in, 50, 1)],
                },
            ],
            scanned: true,
            scanning: false,
            scan_task: None,
            discover_task: None,
            discovering: false,
            gen: 0,
            selected: HashSet::new(),
            expanded: HashSet::new(),
            scroll: std::collections::HashMap::new(),
            scroll_drag: None,
        }
    }

    fn recommended_cat(j: &JunkState) -> CategoryId {
        j.categories[0].category
    }

    fn opt_in_cat(j: &JunkState) -> CategoryId {
        j.categories[1].category
    }

    #[test]
    fn totals_span_every_category() {
        let j = junk_fixture();
        assert_eq!(j.total_cleanable(), 350);
        assert_eq!(j.total_item_count(), 3);
    }

    /// 推荐勾选必须跳过「默认不勾」的开发者类目——它们删掉不坏系统，
    /// 但会让下次构建重来，甚至丢掉未提交的改动。
    #[test]
    fn recommended_selection_skips_opt_in_categories() {
        let mut j = junk_fixture();
        j.select_recommended();

        assert_eq!(j.selected_count(), 2);
        assert_eq!(j.selected_size(), 300);
        assert_eq!(j.selected_file_count(), 8);
        assert!(!j.selected.contains(&PathBuf::from(r"C:\opt\c")));
        assert!(j.selection_is_recommended());
    }

    #[test]
    fn select_every_and_none_are_inverses() {
        let mut j = junk_fixture();
        j.select_every();
        assert_eq!(j.selected_count(), 3);
        assert_eq!(j.selected_size(), 350);
        assert!(!j.selection_is_recommended(), "全选不等于推荐");

        j.select_none();
        assert_eq!(j.selected_count(), 0);
        assert_eq!(j.selected_size(), 0);
    }

    #[test]
    fn invert_flips_every_item() {
        let mut j = junk_fixture();
        j.select_recommended();
        j.invert_selection();

        assert_eq!(j.selected_count(), 1);
        assert!(j.selected.contains(&PathBuf::from(r"C:\opt\c")));

        j.invert_selection();
        assert!(j.selection_is_recommended());
    }

    /// 勾选集合里残留了扫描结果之外的路径时，不能再算作「推荐状态」。
    /// 这正是 `selection_is_recommended` 末尾那句长度比对在防的事。
    #[test]
    fn stale_selection_is_not_recommended() {
        let mut j = junk_fixture();
        j.select_recommended();
        assert!(j.selection_is_recommended());

        j.selected.insert(PathBuf::from(r"C:\already\deleted"));
        assert!(!j.selection_is_recommended(), "多出来的残留项没被发现");
    }

    #[test]
    fn category_check_reports_partial_state() {
        let mut j = junk_fixture();
        let cat = &j.categories[0].clone();

        assert_eq!(j.category_check(cat), Check::Off);
        j.toggle_item(Path::new(r"C:\rec\a"));
        assert_eq!(j.category_check(cat), Check::Partial);
        j.toggle_item(Path::new(r"C:\rec\b"));
        assert_eq!(j.category_check(cat), Check::On);
    }

    /// 类目复选框：全选态点一下清空，否则补齐整组（含部分选中的情况）。
    #[test]
    fn toggling_a_category_fills_then_clears() {
        let mut j = junk_fixture();
        let id = recommended_cat(&j);

        j.toggle_category(id);
        assert_eq!(j.selected_count(), 2);

        j.toggle_category(id);
        assert_eq!(j.selected_count(), 0);

        // 部分选中时应当补齐，而不是清空
        j.toggle_item(Path::new(r"C:\rec\a"));
        j.toggle_category(id);
        assert_eq!(j.selected_count(), 2);
    }

    #[test]
    fn set_category_selected_only_touches_that_category() {
        let mut j = junk_fixture();
        j.select_every();
        j.set_category_selected(opt_in_cat(&j), false);

        assert_eq!(j.selected_count(), 2);
        assert!(!j.selected.contains(&PathBuf::from(r"C:\opt\c")));
    }

    #[test]
    fn toggle_item_and_expand_round_trip() {
        let mut j = junk_fixture();
        let id = recommended_cat(&j);

        j.toggle_item(Path::new(r"C:\rec\a"));
        assert!(j.selected.contains(&PathBuf::from(r"C:\rec\a")));
        j.toggle_item(Path::new(r"C:\rec\a"));
        assert!(j.selected.is_empty());

        assert!(!j.expanded.contains(&id));
        j.toggle_expand(id);
        assert!(j.expanded.contains(&id));
        j.toggle_expand(id);
        assert!(!j.expanded.contains(&id));
    }

    /// 每个条目的处置方式来自它所属的类目：系统缓存目录要保留目录本身，
    /// 开发产物要连目录一起删（空的 node_modules 比不存在更糟）。
    #[test]
    fn selected_targets_carry_per_category_disposal() {
        let mut j = junk_fixture();
        j.select_every();

        let targets = j.selected_targets();
        assert_eq!(targets.len(), 3);
        for t in &targets {
            let cat = j
                .items()
                .find(|i| i.path == t.path)
                .expect("目标必须来自扫描结果")
                .category;
            assert_eq!(t.remove_dir, cat.removes_directory());
        }
    }

    /// 清理成功的条目要从勾选里摘掉；失败的仍然留着，好让用户重试。
    #[test]
    fn clean_result_drops_cleared_items_from_selection() {
        let mut j = junk_fixture();
        j.select_every();

        let attempted = vec![
            PathBuf::from(r"C:\rec\a"),
            PathBuf::from(r"C:\rec\b"),
            PathBuf::from(r"C:\opt\c"),
        ];
        let failed = vec![PathBuf::from(r"C:\rec\b")];
        j.apply_clean_result(&attempted, &failed);

        assert!(
            !j.selected.contains(&PathBuf::from(r"C:\rec\a")),
            "删成功的还留在勾选里"
        );
        assert!(
            j.selected.contains(&PathBuf::from(r"C:\rec\b")),
            "删失败的不该被摘掉"
        );
    }

    #[test]
    fn empty_state_is_well_behaved() {
        let mut j = junk_fixture();
        j.categories.clear();

        assert_eq!(j.total_cleanable(), 0);
        assert_eq!(j.total_item_count(), 0);
        assert!(j.selected_targets().is_empty());
        j.select_recommended();
        assert!(
            j.selection_is_recommended(),
            "空扫描结果 + 空勾选就是推荐状态"
        );
    }
}
