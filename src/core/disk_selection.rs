//! 磁盘透镜的多级勾选状态

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
