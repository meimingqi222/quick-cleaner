//! 磁盘空间分析通用领域模型

#[cfg(windows)]
pub use crate::platform::windows::mft::{
    DirUsage, MftError, MftScan, MftTree, Node, ROOT_RECORD as ROOT_NODE,
};

#[cfg(not(windows))]
pub mod fallback {
    #[derive(Clone, Debug)]
    pub struct DirUsage {
        pub path: String,
        pub size: u64,
        pub file_count: u64,
    }

    #[derive(Clone, Debug)]
    pub struct Node {
        pub idx: u32,
        pub name: String,
        pub is_dir: bool,
        pub size: u64,
        pub file_count: u64,
        pub own_size: u64,
    }

    /// 目录树根节点的下标。Windows 上是 `$MFT` 的 5 号记录，这里没有 MFT，用 0。
    pub const ROOT_NODE: u32 = 0;

    /// 非 Windows 平台的空目录树。
    ///
    /// 方法集必须与 `platform::windows::mft::MftTree` 中 UI 实际调用到的
    /// 那一套保持一致，否则非 Windows 目标一编译就断。
    #[derive(Clone, Debug, Default)]
    pub struct MftTree;

    impl MftTree {
        pub fn volume(&self) -> char { '/' }
        pub fn root(&self) -> u32 { 0 }
        pub fn valid(&self, _idx: u32) -> bool { false }
        pub fn is_dir(&self, _idx: u32) -> bool { false }
        pub fn size_of(&self, _idx: u32) -> u64 { 0 }
        pub fn file_count_of(&self, _idx: u32) -> u64 { 0 }
        pub fn name_of(&self, _idx: u32) -> String { String::new() }
        pub fn parent_of(&self, _idx: u32) -> Option<u32> { None }
        pub fn path_of(&self, _idx: u32) -> String { String::new() }
        pub fn path_of_with(
            &self,
            _idx: u32,
            _cache: &mut std::collections::HashMap<u32, String>,
        ) -> String {
            String::new()
        }
        pub fn children(&self, _idx: u32) -> Vec<Node> { Vec::new() }
        pub fn largest_files(&self, _n: usize) -> Vec<Node> { Vec::new() }
        pub fn find_path(&self, _full_path: &std::path::Path) -> Vec<u32> { vec![0] }
        pub fn find_node_by_path(&self, _full_path: &std::path::Path) -> Option<u32> { None }
        pub fn remove_node(&mut self, _idx: u32) {}
    }

    #[derive(Clone, Debug)]
    pub struct MftScan {
        pub volume: char,
        pub total_size: u64,
        pub file_count: u64,
        pub dir_count: u64,
        pub dirs: Vec<DirUsage>,
        pub tree: MftTree,
        pub elapsed_ms: u64,
        pub records_read: u64,
        pub records_expected: u64,
        pub mft_run_bytes: u64,
        pub ext_records: u64,
        pub ext_data_merged: u64,
        pub hard_links: u64,
        pub unique_size: u64,
        pub unique_files: u64,
    }

    impl MftScan {
        pub fn remove_path(&mut self, _path: &std::path::Path) {}
    }

    #[derive(Debug)]
    pub enum MftError {
        AccessDenied,
        NotNtfs,
        Io(String),
    }

    impl std::fmt::Display for MftError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                MftError::AccessDenied => write!(f, "需要管理员权限"),
                MftError::NotNtfs => write!(f, "不是 NTFS 卷"),
                MftError::Io(e) => write!(f, "IO 错误: {e}"),
            }
        }
    }
}

#[cfg(not(windows))]
pub use fallback::*;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 磁盘透镜的多级勾选状态。
///
/// 语义：勾选一个目录代表「连同其全部子孙一起删」，进入子层后可以再把
/// 个别子项排除掉。因此需要同时维护「显式勾选」与「继承勾选下的排除」
/// 两个集合，`is_selected` 沿父链回溯，就近的那条规则说了算。
#[derive(Clone, Debug, Default)]
pub struct DiskSelectionState {
    /// 用户显式勾选的路径
    selected: HashSet<PathBuf>,
    /// 在某个已勾选祖先之下被单独排除的路径
    deselected: HashSet<PathBuf>,
    /// 显式勾选项的体积，用于 O(K) 汇总，避免渲染时遍历磁盘
    sizes: HashMap<PathBuf, u64>,
    /// 被排除子项的体积，汇总时要从祖先的体积里扣掉
    excluded_sizes: HashMap<PathBuf, u64>,
}

impl DiskSelectionState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 某路径当前是否处于勾选状态（沿父链继承，就近规则优先）。
    pub fn is_selected(&self, path: &Path) -> bool {
        if self.deselected.contains(path) {
            return false;
        }
        if self.selected.contains(path) {
            return true;
        }
        let mut cur = path.parent();
        while let Some(p) = cur {
            if self.deselected.contains(p) {
                return false;
            }
            if self.selected.contains(p) {
                return true;
            }
            cur = p.parent();
        }
        false
    }

    /// 切换勾选状态。`size` 是该项的体积，用于维护增量汇总。
    pub fn toggle(&mut self, path: &Path, size: u64) {
        let pb = path.to_path_buf();

        if self.is_selected(path) {
            if self.selected.remove(&pb) {
                self.sizes.remove(&pb);
                // 这一项底下的排除记录现在没有依附对象了，必须一起清掉，
                // 否则用户「取消再重新勾选」之后，那些子项会被静默漏删。
                self.prune_orphan_exclusions();
            } else {
                // 勾选来自某个祖先，记为局部排除，并从汇总里扣掉它的体积
                self.excluded_sizes.insert(pb.clone(), size);
                self.deselected.insert(pb);
            }
        } else if self.deselected.remove(&pb) {
            // 撤销排除，恢复从祖先继承来的勾选
            self.excluded_sizes.remove(&pb);
        } else {
            self.selected.insert(pb.clone());
            self.sizes.insert(pb.clone(), size);
            self.absorb_covered_descendants(&pb);
        }
    }

    /// 新勾选 `parent` 之后，把已被它覆盖的显式勾选子孙收编掉。
    ///
    /// 不做这一步的话，「先勾子目录、再勾父目录」会让 `sizes` 里同时留下
    /// 两条记录，`total_size` 把同一批字节数了两遍——用户在执行删除**之前**
    /// 看到的就是这个虚高的数字。`resolve_targets` 也会同时吐出父与子。
    ///
    /// 中间隔着排除项的子孙不能收编：它靠的是自己那条显式勾选活着，
    /// 撤掉就真的不删了。
    fn absorb_covered_descendants(&mut self, parent: &Path) {
        let descendants: Vec<PathBuf> = self
            .selected
            .iter()
            .filter(|d| d.as_path() != parent && d.starts_with(parent))
            .cloned()
            .collect();

        for d in descendants {
            self.selected.remove(&d);
            if self.is_selected(&d) {
                // 摘掉显式勾选后仍然是选中的 —— 说明确实被祖先覆盖了
                self.sizes.remove(&d);
            } else {
                // 中间有排除挡着，得把显式勾选放回去
                self.selected.insert(d);
            }
        }
    }

    /// 丢弃那些头上已经没有任何勾选祖先的排除记录。
    ///
    /// 排除只在「某个祖先被勾选」的语境下才有意义。祖先一旦取消勾选，
    /// 残留的排除记录会在下次重新勾选时悄悄生效。
    fn prune_orphan_exclusions(&mut self) {
        let orphans: Vec<PathBuf> = self
            .deselected
            .iter()
            .filter(|e| !self.has_selected_ancestor(e))
            .cloned()
            .collect();

        for e in orphans {
            self.deselected.remove(&e);
            self.excluded_sizes.remove(&e);
        }
    }

    /// 父链上是否存在显式勾选项（不含自身）。
    fn has_selected_ancestor(&self, path: &Path) -> bool {
        let mut cur = path.parent();
        while let Some(p) = cur {
            if self.selected.contains(p) {
                return true;
            }
            cur = p.parent();
        }
        false
    }

    /// 把某项设为指定勾选状态（用于「全选/全不选」这类批量操作）。
    pub fn set(&mut self, path: &Path, size: u64, on: bool) {
        if self.is_selected(path) != on {
            self.toggle(path, size);
        }
    }

    pub fn clear(&mut self) {
        self.selected.clear();
        self.deselected.clear();
        self.sizes.clear();
        self.excluded_sizes.clear();
    }

    /// 勾选总体积 = 显式勾选之和 − 被排除子项之和。
    ///
    /// 纯内存累加，不碰磁盘：这个值在渲染路径上每帧都要读。
    pub fn total_size(&self) -> u64 {
        let picked: u64 = self.sizes.values().sum();
        let excluded: u64 = self.excluded_sizes.values().sum();
        picked.saturating_sub(excluded)
    }

    /// 显式勾选的条目数。
    pub fn len(&self) -> usize {
        self.selected.len()
    }

    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    pub fn selected_roots(&self) -> impl Iterator<Item = &PathBuf> {
        self.selected.iter()
    }

    /// 展开成实际要删除的路径列表。
    ///
    /// 勾选的目录里若含有被排除的子孙，就下钻到刚好绕开它们的那一层。
    /// 必须递归：被排除项可能埋在很深的位置，只展开一层会把它一起删掉。
    pub fn resolve_targets(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for root in &self.selected {
            self.expand(root, &mut out);
        }
        out
    }

    fn expand(&self, path: &Path, out: &mut Vec<PathBuf>) {
        if self.deselected.contains(path) {
            return;
        }
        let has_excluded_descendant = self
            .deselected
            .iter()
            .any(|d| d != path && d.starts_with(path));
        if !has_excluded_descendant {
            out.push(path.to_path_buf());
            return;
        }
        let Ok(rd) = std::fs::read_dir(path) else {
            return;
        };
        for entry in rd.flatten() {
            self.expand(&entry.path(), out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disk_selection_inheritance_and_deselection() {
        let mut state = DiskSelectionState::new();
        let parent = PathBuf::from(r"C:\Users\test\AppData\Roaming\TRAE SOLO CN");
        let child1 = parent.join("resources");
        let child2 = parent.join("extensions");
        let grandchild = child1.join("app.asar");

        // 1. 选中父文件夹
        state.toggle(&parent, 5_000_000_000);
        assert!(state.is_selected(&parent));
        assert_eq!(state.total_size(), 5_000_000_000);

        // 2. 进入子目录，子项目和孙项目均自动继承选中
        assert!(state.is_selected(&child1));
        assert!(state.is_selected(&child2));
        assert!(state.is_selected(&grandchild));

        // 3. 反选/取消勾选 child1
        state.toggle(&child1, 1_000_000);
        assert!(!state.is_selected(&child1));
        assert!(!state.is_selected(&grandchild)); // 孙项目也随之被排除
        assert!(state.is_selected(&child2)); // child2 仍然保持继承选中
        assert!(state.is_selected(&parent)); // 父级依然保留

        // 4. 重新勾选 child1
        state.toggle(&child1, 1_000_000);
        assert!(state.is_selected(&child1));
        assert!(state.is_selected(&grandchild));

        // 5. 清空选择
        state.clear();
        assert!(!state.is_selected(&parent));
        assert!(!state.is_selected(&child1));
        assert_eq!(state.total_size(), 0);
    }

    #[test]
    fn excluded_child_is_subtracted_from_total() {
        let mut state = DiskSelectionState::new();
        let parent = PathBuf::from(r"C:\proj");
        let child = parent.join("node_modules");

        state.toggle(&parent, 1_000);
        assert_eq!(state.total_size(), 1_000);

        // 排除一个继承勾选的子项后，汇总体积必须相应减少，
        // 否则确认框会虚报「将释放多少空间」。
        state.toggle(&child, 400);
        assert_eq!(state.total_size(), 600);

        // 撤销排除后恢复
        state.toggle(&child, 400);
        assert_eq!(state.total_size(), 1_000);
    }

    #[test]
    fn total_size_tracks_multiple_explicit_picks() {
        let mut state = DiskSelectionState::new();
        state.toggle(Path::new(r"C:\a"), 10);
        state.toggle(Path::new(r"C:\b"), 25);
        assert_eq!(state.len(), 2);
        assert_eq!(state.total_size(), 35);

        state.toggle(Path::new(r"C:\a"), 10);
        assert_eq!(state.len(), 1);
        assert_eq!(state.total_size(), 25);
    }

    /// 先勾子目录、再勾父目录：父项应当把子项收编，而不是两笔都记账。
    ///
    /// 这个数字会在**执行删除之前**显示给用户看，错了就是在误导人。
    #[test]
    fn selecting_a_parent_absorbs_already_selected_children() {
        let mut st = DiskSelectionState::new();
        let parent = PathBuf::from("C:\\a");
        let child = PathBuf::from("C:\\a\\b");

        st.toggle(&child, 100);
        st.toggle(&parent, 1000);

        assert_eq!(st.total_size(), 1000, "父子体积被重复累加了");
        assert_eq!(st.len(), 1, "子项没有被父项收编");
        assert_eq!(st.resolve_targets(), vec![parent.clone()]);
        assert!(st.is_selected(&child), "子项仍应处于选中（继承自父）");
    }

    /// 但中间隔着排除项的孙子不能被收编——它靠自己那条显式勾选活着。
    #[test]
    fn absorption_stops_at_an_excluded_level() {
        let mut st = DiskSelectionState::new();
        let a = PathBuf::from("C:\\a");
        let b = PathBuf::from("C:\\a\\b");
        let c = PathBuf::from("C:\\a\\b\\c");

        st.toggle(&a, 1000); // 勾 a
        st.toggle(&b, 300); // 排除 b
        st.toggle(&c, 50); // 但 b 底下的 c 还是要删

        assert!(st.is_selected(&a));
        assert!(!st.is_selected(&b));
        assert!(st.is_selected(&c), "显式勾选的 c 不该被 b 的排除吃掉");

        // 再勾一次 a（本就选中，这里退化成取消），c 的显式勾选要留着
        let mut targets = st.resolve_targets();
        targets.sort();
        assert!(targets.contains(&c));
    }

    /// 取消父项时，它底下的排除记录必须一并作废。
    ///
    /// 否则「取消 → 重新勾选」之后，被排除过的子项会静默地继续不删，
    /// 而界面上看起来整个目录都是勾上的。
    #[test]
    fn unchecking_a_parent_clears_its_exclusions() {
        let mut st = DiskSelectionState::new();
        let parent = PathBuf::from("C:\\a");
        let child = PathBuf::from("C:\\a\\b");

        st.toggle(&parent, 1000); // 勾父
        st.toggle(&child, 100); // 排除子
        assert_eq!(st.total_size(), 900);

        st.toggle(&parent, 1000); // 取消父
        assert!(!st.is_selected(&parent));
        assert_eq!(st.total_size(), 0);

        st.toggle(&parent, 1000); // 重新勾父
        assert!(st.is_selected(&child), "重新勾选后子项仍被残留的排除挡着");
        assert_eq!(st.total_size(), 1000);
        assert_eq!(st.resolve_targets(), vec![parent]);
    }

    /// `clear` 之后必须是全新状态，不留任何残渣。
    #[test]
    fn clear_wipes_every_map() {
        let mut st = DiskSelectionState::new();
        let parent = PathBuf::from("C:\\a");
        st.toggle(&parent, 1000);
        st.toggle(&parent.join("b"), 100);
        st.clear();

        assert_eq!(st.total_size(), 0);
        assert_eq!(st.len(), 0);
        assert!(st.is_empty());
        assert!(!st.is_selected(&parent));
        assert!(st.resolve_targets().is_empty());
    }

    #[test]
    fn resolve_targets_returns_root_when_nothing_excluded() {
        let mut state = DiskSelectionState::new();
        let root = PathBuf::from(r"C:\a");
        state.toggle(&root, 10);
        assert_eq!(state.resolve_targets(), vec![root]);
    }
}
