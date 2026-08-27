//! UI 状态结构体定义与纯逻辑方法

use crate::core::apps::{AppFilterPreset, AppSortState, InstalledApp, ResidualScanResult};
use crate::core::categories::CategoryId;
use crate::core::cleaner::{CleanProgress, CleanTarget};
use crate::core::disk::{DiskSelectionState, Node, ScanResult, VolumeId};
use crate::core::model::Check;
use crate::core::scanner::{apply_clean_result, CategorySummary, ScanItem};
use crate::ui::views::DiskTab;
use gpui::Task;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;

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

    /// 按每个条目的安全策略预勾选（扫描完成后的初始状态）。
    pub fn select_recommended(&mut self) {
        self.selected = self
            .items()
            .filter(|i| i.recommended)
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
            let want = item.recommended;
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
            .map(|i| {
                // 固定规则也会产生单文件目标（如 .DS_Store）。文件必须走
                // clean_path；clean_dir_contents 只适用于真实目录。
                let is_file_or_link = std::fs::symlink_metadata(&i.path)
                    .is_ok_and(|md| md.is_file() || md.file_type().is_symlink());
                // 虚拟路径（Docker 镜像）删除时不逐文件累计体积，把扫描
                // 阶段的 size_hint 带下去，成功后一次性记账。
                let size_hint = if crate::core::model::is_virtual_path(&i.path) {
                    Some(i.size)
                } else {
                    None
                };
                CleanTarget {
                    path: i.path.clone(),
                    remove_dir: i.category.removes_directory() || is_file_or_link,
                    size_hint,
                }
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

/// 搜索框文本区最近一次绘制的排版，用于鼠标命中测试。
///
/// 必须与屏幕上实际画出的字形是同一份 `ShapedLine`：用另一套字体/字重
/// 再 layout 一次，或用固定像素去猜文本起点，都会把光标点偏一格。
pub struct SearchTextHit {
    pub bounds: gpui::Bounds<gpui::Pixels>,
    pub line: gpui::ShapedLine,
}

/// 一个单行文本输入框的全部状态。
///
/// Apps 搜索框和文件搜索框本来各带一套完全相同的字段，`text_input.rs`
/// 的每个方法、`search_box.rs` 的每个回调都要写一遍
/// `if is_file_search { …search… } else { …apps… }`——两份实现已经开始
///各自漂移。现在两边共用这一个结构体，分支只剩「拿哪一个」。
pub struct TextInputState {
    pub text: String,
    /// 光标/选区的**字节**范围
    pub sel: std::ops::Range<usize>,
    /// 输入法正在组合中的那段文本的字节范围（拼音串，尚未确认）
    pub marked: Option<std::ops::Range<usize>>,
    /// 输入框最近一次绘制的位置，用来定位输入法候选窗口
    pub bounds: Option<gpui::Bounds<gpui::Pixels>>,
    /// 文本区真实排版（绘制与点击命中共用）
    pub text_hit: Option<SearchTextHit>,
    /// 文本拖拽选区的锚点字节偏移（鼠标按下时的位置）
    pub text_drag: Option<usize>,
    pub focus_handle: gpui::FocusHandle,
}

impl TextInputState {
    pub fn new(focus_handle: gpui::FocusHandle) -> Self {
        Self {
            text: String::new(),
            sel: 0..0,
            marked: None,
            bounds: None,
            text_hit: None,
            text_drag: None,
            focus_handle,
        }
    }

    /// 光标（或选区）的字节范围，已钳到合法的字符边界。
    pub fn selection(&self) -> std::ops::Range<usize> {
        crate::ui::text_input::clamp_to_boundary(&self.text, self.sel.clone())
    }

    /// 把光标收到末尾。内容被外部改动（清空按钮、切换筛选）后要调一次，
    /// 否则残留的旧偏移会指到字符串外面。
    pub fn reset_caret(&mut self) {
        let end = self.text.len();
        self.sel = end..end;
        self.marked = None;
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.reset_caret();
    }

    /// 提交文本：普通打字与输入法确认后的汉字都走这里。
    pub fn replace(&mut self, range_utf16: Option<&std::ops::Range<usize>>, new_text: &str) {
        let (sel, marked) = crate::ui::text_input::replace_text(
            &mut self.text,
            &self.sel,
            &self.marked,
            range_utf16,
            new_text,
        );
        self.sel = sel;
        self.marked = marked;
    }

    /// 组合中的文本：输入法候选阶段的拼音串，尚未确认。
    pub fn replace_and_mark(
        &mut self,
        range_utf16: Option<&std::ops::Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<&std::ops::Range<usize>>,
    ) {
        let (sel, marked) = crate::ui::text_input::replace_and_mark_text(
            &mut self.text,
            &self.sel,
            &self.marked,
            range_utf16,
            new_text,
            new_selected_range_utf16,
        );
        self.sel = sel;
        self.marked = marked;
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
    /// 搜索框（文本 + 选区 + IME + 命中测试）
    pub input: TextInputState,
    /// 软件表每次被整体替换就自增，用来判定渲染缓存是否失效
    pub gen: u64,
    /// 过滤 + 排序后的 `list` 下标，渲染直接读这里
    pub view: Vec<usize>,
    pub(super) view_key: Option<AppsViewKey>,
    /// 软件表也走虚拟化列表，句柄需长期持有
    pub scroll: gpui::UniformListScrollHandle,
    pub scroll_drag: Option<(f32, f32)>,
    pub context_menu: Option<AppsContextMenu>,
}

/// 深度卸载的残留扫描状态。
pub struct ResidualState {
    pub result: Option<ResidualScanResult>,
    pub scanning: bool,
    pub task: Option<Task<()>>,
    pub selected: HashSet<usize>,
    pub uninstall: Option<Arc<UninstallProgress>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UninstallPhase {
    Discovering = 0,
    Removing = 1,
    Verifying = 2,
}

/// 文件搜索结果排序列（表头配置）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SearchSortCol {
    /// 按名称字母序
    Name,
    /// 按路径字母序
    Path,
    /// 按体积大小
    #[default]
    Size,
}

/// 文件快速检索状态。
///
/// 搜索框走和 AppsState 搜索框一样的 IME 管线（EntityInputHandler），
/// 两边共用 [`TextInputState`]，由焦点决定输入落到哪一个。
pub struct SearchState {
    /// 搜索框（文本 + 选区 + IME + 命中测试）
    pub input: TextInputState,
    pub results: Vec<crate::core::disk::SearchHit>,
    /// 是否正在后台构建搜索索引
    pub indexing: bool,
    pub index_task: Option<Task<()>>,
    /// Windows: 所有卷的 MFT 树。macOS: 复用 Root::macos_root_index。
    #[cfg(windows)]
    pub indices: Vec<std::sync::Arc<crate::core::disk::ScanResult>>,
    /// 搜索结果虚拟列表
    pub scroll: gpui::UniformListScrollHandle,
    /// 滚动条拖拽状态：(按下时鼠标 y, 按下时滚动 top)
    pub scroll_drag: Option<(f32, f32)>,
    /// 异步搜索任务句柄（用于防抖）
    pub search_task: Option<Task<()>>,
    /// 每次输入变化递增；后台遍历定期检查，及时终止已过期查询。
    pub search_generation: Arc<AtomicU64>,
    /// 是否正在搜索中
    pub is_searching: bool,
    /// 一级排序：是否开启同类型文件聚合（默认开启）
    pub group_by_kind: bool,
    /// 二级排序：表头排序列（名称 / 路径 / 大小，默认大小）
    pub sort_col: SearchSortCol,
    /// 排序方向：true 为升序（A-Z / 小到大），false 为降序（Z-A / 大到小）
    pub sort_asc: bool,
    /// 结果集版本号，用于虚拟列表刷新
    pub gen: u64,
}

pub struct UninstallProgress {
    pub app_name: String,
    phase: AtomicU8,
}

impl UninstallProgress {
    pub(crate) fn new(app_name: String) -> Self {
        Self {
            app_name,
            phase: AtomicU8::new(UninstallPhase::Discovering as u8),
        }
    }

    pub fn phase(&self) -> UninstallPhase {
        match self.phase.load(Ordering::Relaxed) {
            1 => UninstallPhase::Removing,
            2 => UninstallPhase::Verifying,
            _ => UninstallPhase::Discovering,
        }
    }

    pub(crate) fn set_phase(&self, phase: UninstallPhase) {
        self.phase.store(phase as u8, Ordering::Relaxed);
    }
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
    /// 面包屑是否展开为全部层级。折叠态只显示「首段 + … + 末两段」，
    /// 深路径下说不清自己在哪；点「…」切换。
    pub crumbs_expanded: bool,
    /// 磁盘透镜的勾选状态（含继承与局部排除），实现见 `core::disk`
    pub sel: DiskSelectionState,
    pub space: Option<(u64, u64)>,
    /// 各卷的容量/可用量缓存。渲染每帧都要给卷列表画「已用 x / 共 y」，
    /// 直接调 `get_volume_space` 等于每帧对每个卷做一次 statfs 系统调用。
    /// 与 `space` 同一时机刷新（构造时、切卷/扫描时）。
    pub volume_spaces: std::collections::HashMap<VolumeId, (u64, u64)>,
    /// 当前目录（或最大文件列表）的渲染行缓存
    pub rows: Vec<DiskRow>,
    pub(super) rows_key: Option<DiskRowsKey>,
    /// 磁盘切换下拉浮层菜单是否展开
    pub volume_menu_open: bool,
    /// MFT 树每次被替换或就地修改就自增
    pub gen: u64,
}

impl DiskState {
    /// 取某个卷的 (总量, 可用量)，读缓存而不是每帧 statfs。
    pub fn volume_space(&self, vol: &VolumeId) -> Option<(u64, u64)> {
        self.volume_spaces.get(vol).copied()
    }

    /// 重新采集所有卷的空间信息，并同步当前卷的 `space`。
    pub fn refresh_volume_spaces(&mut self) {
        self.volume_spaces = self
            .volumes
            .iter()
            .filter_map(|v| crate::platform::get_volume_space(v).map(|s| (v.clone(), s)))
            .collect();
        self.space = self.volume_space(&self.volume);
    }
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
pub type DiskRowsKey = (String, u32, DiskTab, u64);

/// 软件列表视图缓存的失效键
pub type AppsViewKey = (u64, AppFilterPreset, String, AppSortState);

/// 磁盘透镜一屏最多渲染多少行（超出部分用户也看不过来）
pub const DISK_MAX_ROWS: usize = 200;

#[derive(Clone, Debug)]
pub struct AppsContextMenu {
    pub app: InstalledApp,
    pub x: f32,
    pub y: f32,
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
            recommended: cat.default_selected(),
            busy: None,
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
    fn recommended_selection_is_per_item_within_one_category() {
        let mut j = junk_fixture();
        let cat = opt_in_cat(&j);
        let mut safe_cache = item(r"C:\opt\safe-cache", cat, 25, 1);
        safe_cache.recommended = true;
        j.categories[1].items.push(safe_cache);
        j.categories[1].total_size += 25;

        j.select_recommended();

        assert!(j.selected.contains(&PathBuf::from(r"C:\opt\safe-cache")));
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

    /// 目录的处置方式来自类目；单文件始终直接删除。
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

    #[test]
    fn selected_file_target_is_removed_as_a_file() {
        let path = std::env::temp_dir().join("qc_ui_single_file_target");
        std::fs::write(&path, b"x").unwrap();
        let mut j = junk_fixture();
        j.categories = vec![CategorySummary {
            category: CategoryId::UserTemp,
            total_size: 1,
            items: vec![item(&path.to_string_lossy(), CategoryId::UserTemp, 1, 1)],
        }];
        j.select_every();

        let targets = j.selected_targets();

        assert_eq!(targets.len(), 1);
        assert!(targets[0].remove_dir);
        let _ = std::fs::remove_file(path);
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
