//! GPUI 界面根视图与状态管理

pub mod components;
pub mod theme;
pub mod views;

use crate::core::apps::{
    AppFilterPreset, AppSortState, InstalledApp, ResidualKind, ResidualScanResult,
};
use crate::core::categories::{all_targets, CategoryId};
use crate::core::cleaner::{
    clean_arbitrary, clean_targets, CleanProgress, CleanReport, CleanSnapshot, CleanTarget,
};
use crate::core::safety::is_protected;
use crate::core::apps::filter_and_sort_apps;
use crate::core::disk::{DiskSelectionState, MftScan, Node};
use crate::core::model::{fmt_size, Check};
use crate::core::scanner::{apply_clean_result, scan_all, CategorySummary, ScanItem};
use crate::platform::{
    get_volume_space, is_elevated, list_installed_apps, list_ntfs_volumes, run_uninstaller_and_wait,
    scan_residuals, clean_residuals, scan_volume,
};
use crate::ui::components::*;
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
    pub categories: Vec<CategorySummary>,
    pub scanned: bool,
    pub scanning: bool,
    pub view: View,
    pub cleaning: bool,
    pub scan_task: Option<Task<()>>,
    pub live: Arc<AtomicBool>,
    pub status: String,
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
    pub mft_error: Option<String>,
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
        Self {
            categories: Vec::new(),
            scanned: false,
            scanning: false,
            view: View::Dashboard,
            cleaning: false,
            scan_task: None,
            live: Arc::new(AtomicBool::new(true)),
            status: "就绪".into(),
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
            disk_path: vec![5],
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

    pub fn open_app_context_menu(&mut self, app: InstalledApp, x: f32, y: f32) {
        self.apps_context_menu = Some(AppsContextMenu { app, x, y });
    }

    pub fn close_context_menu(&mut self) {
        self.apps_context_menu = None;
    }

    pub fn start_scan(&mut self, cx: &mut Context<Self>) {
        if self.scanning {
            return;
        }
        self.live.store(false, Ordering::Relaxed);
        self.scan_task.take();

        self.scanning = true;
        self.scanned = false;
        self.status = "正在扫描可清理内容…".into();
        let live = Arc::new(AtomicBool::new(true));
        self.live = live.clone();
        self.start_tick(cx);
        cx.notify();

        let targets = all_targets();
        let scan = cx
            .background_executor()
            .spawn(async move { scan_all(&targets, &live) });
        self.scan_task = Some(cx.spawn(async move |this, cx| {
            let result = scan.await;
            this.update(cx, |this, cx| {
                this.categories = result;
                this.scanned = true;
                this.scanning = false;
                this.select_recommended();
                let total: u64 = this.categories.iter().map(|c| c.total_size).sum();
                this.status = format!("扫描完成，共发现 {} 可清理", fmt_size(total));
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
        self.status = format!("正在深度分析磁盘 {vol}: 空间占用…");
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
                        this.status = format!(
                            "磁盘分析完成：已索引 {} 个文件，占用 {}",
                            s.file_count,
                            fmt_size(s.total_size)
                        );
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
                        this.mft = Some(s);
                        this.mft_gen += 1;
                    }
                    Err(e) => {
                        this.status = format!("磁盘分析失败：{e}");
                        this.mft_error = Some(e.to_string());
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
        self.confirm = Some(ConfirmRequest {
            title: "确认永久删除选中项".into(),
            body: format!("将永久删除 {} 项，释放约 {} 磁盘空间。", count, fmt_size(total_size)),
            detail: "文件与目录不会进入回收站，删除后无法恢复。请确认没有重要数据。".into(),
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
        self.status = "正在智能检索已安装软件与空间占用…".into();
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
                this.status = format!(
                    "已加载 {} 款软件，估算总占用 {}",
                    this.apps.len(),
                    fmt_size(total_size)
                );
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn request_uninstall_app(&mut self, app: InstalledApp, cx: &mut Context<Self>) {
        let name = app.name.clone();
        let target_app = app.clone();

        self.status = format!("正在运行「{name}」官方卸载程序，完成后将自动深度扫描残留…");
        cx.notify();

        let uninst_task = cx
            .background_executor()
            .spawn(async move { run_uninstaller_and_wait(&target_app) });

        self.apps_task = Some(cx.spawn(async move |this, cx| {
            let res = uninst_task.await;
            this.update(cx, |this, cx| {
                match res {
                    Ok(_) => {
                        this.status = format!("「{name}」官方卸载已完成，正在扫描残留文件与注册表…");
                    }
                    Err(e) => {
                        this.status = format!("「{name}」官方卸载未正常完成（{e}），转入强力残留清理…");
                    }
                }
                this.start_residual_scan(app, cx);
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
        self.status = format!("正在深度扫描「{}」的文件与注册表残留…", app.name);
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
                this.residual_selected = (0..count).collect();
                this.status = format!(
                    "残留扫描完成：发现「{}」的 {} 项残留，共 {}",
                    res.app_name,
                    count,
                    fmt_size(res.total_file_size)
                );
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
        let items_to_clean: Vec<ResidualKind> = self
            .residual_selected
            .iter()
            .filter_map(|&idx| res.items.get(idx).cloned())
            .collect();

        if items_to_clean.is_empty() {
            self.status = "未选择任何要清除的残留项".into();
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
        self.status = format!("正在彻底清除「{}」的 {} 项残留…", res.app_name, items_to_clean.len());
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

                this.status = if report.failed.is_empty() {
                    format!(
                        "已彻底清除「{}」的 {} 项残留，释放 {}",
                        app_name,
                        report.ok,
                        fmt_size(snap.bytes)
                    )
                } else {
                    format!(
                        "「{}」清除完成，释放 {}（{} 项被占用或权限不足已跳过）",
                        app_name,
                        fmt_size(snap.bytes),
                        report.failed.len()
                    )
                };
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
        self.confirm = Some(ConfirmRequest {
            title: "确认永久删除".into(),
            body: format!("将删除 {} 项，共 {}。", count, fmt_size(self.selected_size())),
            detail: "文件不会进入回收站，删除后无法恢复。".into(),
            kind: ConfirmKind::CleanSelected,
        });
        cx.notify();
    }

    pub fn request_clean_path(&mut self, path: PathBuf, size: u64, cx: &mut Context<Self>) {
        if self.cleaning {
            return;
        }
        if is_protected(&path) {
            self.status = format!("「{}」是受保护的系统路径，不能删除", path.display());
            cx.notify();
            return;
        }
        self.confirm = Some(ConfirmRequest {
            title: "确认永久删除".into(),
            body: format!("将删除 {}（{}）。", path.display(), fmt_size(size)),
            detail: "文件不会进入回收站，删除后无法恢复。请确认它不是正在使用的程序或数据。"
                .into(),
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
            self.status = "正在停止清理…".into();
            cx.notify();
        }
    }

    pub fn start_clean(&mut self, cx: &mut Context<Self>) {
        if self.cleaning || !self.scanned {
            return;
        }
        let paths = self.selected_paths();
        if paths.is_empty() {
            self.status = "没有勾选任何要清理的内容".into();
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
        self.status = format!("正在永久删除 {} 项…", paths.len());
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
                this.status = if fails > 0 {
                    format!(
                        "清理完成：已删除 {} 个文件，释放 {}（{} 项被占用已跳过）",
                        snap.files,
                        fmt_size(snap.bytes),
                        fails
                    )
                } else {
                    format!(
                        "清理完成：已删除 {} 个文件，释放 {}",
                        snap.files,
                        fmt_size(snap.bytes)
                    )
                };
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
        self.status = format!("正在删除 {}…", path.display());
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
                    this.status = format!(
                        "已删除 {}（{} 个文件，{}）",
                        path.display(),
                        snap.files,
                        fmt_size(snap.bytes)
                    );
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
                    this.status =
                        format!("删除失败：{}（被占用或权限不足）", path.display());
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
        self.status = format!("正在批量删除 {} 项…", targets.len());
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
                if report.failed.is_empty() {
                    this.status = format!(
                        "批量删除完成：已删除 {} 个文件，释放 {}",
                        snap.files,
                        fmt_size(snap.bytes)
                    );
                } else {
                    this.status = format!(
                        "批量删除完成，释放 {}（{} 项受保护或被占用已跳过）",
                        fmt_size(snap.bytes),
                        report.failed.len()
                    );
                }
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
                    .child(self.status.clone()),
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
