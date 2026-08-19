//! 磁盘空间分析通用领域模型

use std::path::{Path, PathBuf};

/// 跨平台卷标识：Windows 是盘符（`C:`），Unix 是挂载点（`/`、`/Volumes/外接盘`）。
///
/// 旧契约用 `char` 表达卷，macOS 上只能返回 `'/'` 占位，`/Volumes` 下的外接盘
/// 表达不了。`VolumeId` 持有挂载点路径和一个展示用的标签，两边都能用。
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct VolumeId {
    /// 挂载点路径：Windows 上是 `C:\`，macOS 上是 `/` 或 `/Volumes/...`
    mount: PathBuf,
    /// 展示标签：Windows 上是 `C:`，macOS 上是挂载点路径的字符串
    label: String,
}

impl VolumeId {
    /// Windows 上从盘符构造。
    #[cfg(windows)]
    pub fn from_drive_letter(letter: char) -> Self {
        let letter = letter.to_ascii_uppercase();
        Self {
            mount: PathBuf::from(format!("{}:\\", letter)),
            label: format!("{}:", letter),
        }
    }

    /// Unix 上从挂载点路径构造。
    #[cfg(not(windows))]
    pub fn from_mount_point(mount: PathBuf) -> Self {
        let label = mount.display().to_string();
        Self { mount, label }
    }

    /// Unix 上从挂载点路径和自定义标签构造。
    ///
    /// 外接盘的挂载点路径可能很长（`/Volumes/外接盘`），
    /// 但用户看到的卷名可能只是「外接盘」。这个方法允许分离两者。
    #[cfg(not(windows))]
    pub fn from_mount_point_with_label(mount: PathBuf, label: String) -> Self {
        Self { mount, label }
    }

    /// 用户可见的卷标签：`"C:"` / `"/"` / `"/Volumes/外接盘"`。
    pub fn display(&self) -> &str {
        &self.label
    }

    /// 挂载点路径，用于 `statfs` / `GetDiskFreeSpaceEx` 等系统调用。
    pub fn mount_point(&self) -> &Path {
        &self.mount
    }

    /// Windows 上的盘符（大写）。非 Windows 返回 `None`。
    #[cfg(windows)]
    pub fn drive_letter(&self) -> Option<char> {
        self.label.chars().next().map(|c| c.to_ascii_uppercase())
    }
}

impl std::fmt::Display for VolumeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

/// 冗余整理时跳过的目录名（跨平台共享）。
///
/// 隐藏目录、构建产物、依赖缓存等不含用户内容，整理时不应进入。
pub fn is_declutter_ignored_dir_name(name: &str) -> bool {
    let s = name.to_lowercase();
    // 排除所有隐藏目录（以 . 开头，如 .cache, .npm, .cargo, .gradle, .git, .vscode, .idea 等）
    if s.starts_with('.') {
        return true;
    }
    matches!(
        s.as_str(),
        "node_modules"
            | "library"
            | "appdata"
            | "application data"
            | "application support"
            | "local settings"
            | "target"
            | "dist"
            | "build"
            | "out"
            | "bin"
            | "obj"
            | "pkg"
            | "vendor"
            | "pods"
            | "deriveddata"
            | "bower_components"
            | "venv"
            | "env"
            | "__pycache__"
            | "cache"
            | "caches"
            | "temp"
            | "tmp"
            | "logs"
            | "gems"
            | "site-packages"
            | "docs"
            | "doc"
            | "documentation"
            | "manual"
            | "sdk"
            | "javadoc"
    )
}

/// 文件搜索结果条目。跨平台共用。
#[derive(Clone, Debug)]
pub struct SearchHit {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: u64,
}

#[cfg(windows)]
pub use crate::platform::windows::mft::{
    DirUsage, Node, ScanError, ScanResult, SizeTree, ROOT_RECORD as ROOT_NODE,
};

#[cfg(not(windows))]
pub use crate::platform::macos::disk_tree::{
    DirUsage, Node, ScanError, ScanResult, SizeTree, TreeEntry, TreeIndexEntry, TreeSnapshotEntry,
    ROOT_NODE,
};

pub use super::disk_selection::DiskSelectionState;

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个平台合适的绝对路径，`rel` 用 `/` 分段。
    ///
    /// 这些用例原本写死 `C:\\a\\b` 这类 Windows 字面量，但 `Path` 的父子判定是
    /// 平台相关的：Unix 上反斜杠不是分隔符，`C:\\a\\b` 是**单个**组件，
    /// `starts_with("C:\\a")` 恒为 false——父子收编逻辑在 macOS / Linux 上压根
    /// 没被测到，其中 3 个用例还会直接失败。路径按平台构造才能真正覆盖。
    fn p(rel: &str) -> PathBuf {
        let mut base = if cfg!(windows) {
            PathBuf::from("C:\\")
        } else {
            PathBuf::from("/")
        };
        for seg in rel.split('/') {
            base.push(seg);
        }
        base
    }

    #[test]
    fn test_disk_selection_inheritance_and_deselection() {
        let mut state = DiskSelectionState::new();
        let parent = p("Users/test/AppData/Roaming/TRAE SOLO CN");
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
        let parent = p("proj");
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
        state.toggle(&p("a"), 10);
        state.toggle(&p("b"), 25);
        assert_eq!(state.len(), 2);
        assert_eq!(state.total_size(), 35);

        state.toggle(&p("a"), 10);
        assert_eq!(state.len(), 1);
        assert_eq!(state.total_size(), 25);
    }

    /// 先勾子目录、再勾父目录：父项应当把子项收编，而不是两笔都记账。
    ///
    /// 这个数字会在**执行删除之前**显示给用户看，错了就是在误导人。
    #[test]
    fn selecting_a_parent_absorbs_already_selected_children() {
        let mut st = DiskSelectionState::new();
        let parent = p("a");
        let child = p("a/b");

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
        let a = p("a");
        let b = p("a/b");
        let c = p("a/b/c");

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
        let parent = p("a");
        let child = p("a/b");

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
        let parent = p("a");
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
        let root = p("a");
        state.toggle(&root, 10);
        assert_eq!(state.resolve_targets(), vec![root]);
    }

    // ---- 就地子树替换 API 的测试 ----

    /// 构造一棵测试树：
    /// ```text
    /// /root
    /// ├── a.txt (100)
    /// ├── proj
    /// │   ├── b.txt (200)
    /// │   └── src
    /// │       ├── c.txt (300)
    /// │       └── d.txt (400)
    /// └── other
    ///     └── e.txt (500)
    /// ```
    /// 根节点聚合 = 1500, proj 聚合 = 900, src 聚合 = 700, other 聚合 = 500
    #[cfg(not(windows))]
    fn build_test_tree() -> super::SizeTree {
        use super::{SizeTree, TreeIndexEntry};
        let vol = super::VolumeId::from_mount_point(PathBuf::from("/root"));
        let entries = vec![
            TreeIndexEntry {
                parent: 0,
                name: "/root".into(),
                is_dir: true,
                size: 0,
                used: true,
                mtime: 0,
            }, // 0
            TreeIndexEntry {
                parent: 0,
                name: "a.txt".into(),
                is_dir: false,
                size: 100,
                used: true,
                mtime: 0,
            }, // 1
            TreeIndexEntry {
                parent: 0,
                name: "proj".into(),
                is_dir: true,
                size: 0,
                used: true,
                mtime: 0,
            }, // 2
            TreeIndexEntry {
                parent: 2,
                name: "b.txt".into(),
                is_dir: false,
                size: 200,
                used: true,
                mtime: 0,
            }, // 3
            TreeIndexEntry {
                parent: 2,
                name: "src".into(),
                is_dir: true,
                size: 0,
                used: true,
                mtime: 0,
            }, // 4
            TreeIndexEntry {
                parent: 4,
                name: "c.txt".into(),
                is_dir: false,
                size: 300,
                used: true,
                mtime: 0,
            }, // 5
            TreeIndexEntry {
                parent: 4,
                name: "d.txt".into(),
                is_dir: false,
                size: 400,
                used: true,
                mtime: 0,
            }, // 6
            TreeIndexEntry {
                parent: 0,
                name: "other".into(),
                is_dir: true,
                size: 0,
                used: true,
                mtime: 0,
            }, // 7
            TreeIndexEntry {
                parent: 7,
                name: "e.txt".into(),
                is_dir: false,
                size: 500,
                used: true,
                mtime: 0,
            }, // 8
        ];
        SizeTree::from_compact(vol, entries)
    }

    #[cfg(not(windows))]
    #[test]
    fn test_remove_subtree_updates_ancestors() {
        let mut tree = build_test_tree();
        // 删除 proj 子树（idx=2），聚合大小 900
        tree.remove_subtree_inplace(2);

        // 根节点聚合应从 1500 减到 600
        assert_eq!(tree.size_of(tree.root()), 600);
        assert_eq!(tree.file_count_of(tree.root()), 2); // a.txt + e.txt

        // proj 节点应不再有效
        assert!(!tree.valid(2));
        // other 子树不受影响
        assert_eq!(tree.size_of(7), 500);
    }

    #[cfg(not(windows))]
    #[test]
    fn upsert_file_replaces_leaf_and_updates_ancestors() {
        let mut tree = build_test_tree();
        let file = PathBuf::from("/root/proj/b.txt");

        assert!(tree.upsert_file(&file, 250));
        tree.rebuild_child_arrays();

        let node = tree.find_node_by_path(&file).unwrap();
        assert_eq!(tree.size_of(node), 250);
        assert_eq!(tree.size_of(tree.root()), 1_550);
        assert_eq!(tree.file_count_of(tree.root()), 5);
        assert_eq!(
            tree.compact_entries().len(),
            9,
            "持久化时不应保留旧文件墓碑"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn test_remove_then_append_matches_recompute() {
        let mut tree = build_test_tree();
        let old_node = tree
            .find_node_by_path(&PathBuf::from("/root/proj"))
            .unwrap();

        // 删除旧 proj 子树
        tree.remove_subtree_inplace(old_node);

        // 构造新子树：proj 下只有一个 f.txt (999)
        use super::{SizeTree, TreeIndexEntry};
        let new_vol = super::VolumeId::from_mount_point(PathBuf::from("/root/proj"));
        let new_entries = vec![
            TreeIndexEntry {
                parent: 0,
                name: "/root/proj".into(),
                is_dir: true,
                size: 0,
                used: true,
                mtime: 0,
            },
            TreeIndexEntry {
                parent: 0,
                name: "f.txt".into(),
                is_dir: false,
                size: 999,
                used: true,
                mtime: 0,
            },
        ];
        let new_subtree = SizeTree::from_compact(new_vol, new_entries);

        // 找到 proj 的父节点（根节点）
        let parent_idx = tree.find_node_by_path(&PathBuf::from("/root")).unwrap();
        tree.append_subtree(parent_idx, &new_subtree, "proj");
        tree.rebuild_child_arrays();

        // 验证增量更新后的聚合大小
        let incremental_root_size = tree.size_of(tree.root());
        let incremental_root_files = tree.file_count_of(tree.root());

        // 用 recompute_aggregates 做全量重算，对比是否一致
        let mut recompute_tree = tree.clone();
        recompute_tree.recompute_aggregates();

        assert_eq!(
            incremental_root_size,
            recompute_tree.size_of(recompute_tree.root()),
            "增量更新与全量重算的根节点大小不一致"
        );
        assert_eq!(
            incremental_root_files,
            recompute_tree.file_count_of(recompute_tree.root()),
            "增量更新与全量重算的根节点文件数不一致"
        );

        // 根节点聚合 = 100 (a.txt) + 999 (f.txt) + 500 (e.txt) = 1599
        assert_eq!(incremental_root_size, 1599);
        assert_eq!(incremental_root_files, 3);

        // 新 proj 目录应能通过路径找到
        let new_proj = tree.find_node_by_path(&PathBuf::from("/root/proj"));
        assert!(new_proj.is_some(), "新 proj 目录应能通过路径定位");
        assert_eq!(tree.size_of(new_proj.unwrap()), 999);
    }

    #[cfg(not(windows))]
    #[test]
    fn test_remove_nonexistent_subtree_is_noop() {
        let mut tree = build_test_tree();
        let original_size = tree.size_of(tree.root());
        // 删除不存在的节点不应有任何影响
        tree.remove_subtree_inplace(999);
        assert_eq!(tree.size_of(tree.root()), original_size);
    }

    #[cfg(not(windows))]
    #[test]
    fn test_count_used_dirs_and_files() {
        let tree = build_test_tree();
        // 目录: /root, proj, src, other = 4
        assert_eq!(tree.count_used_dirs(), 4);
        // 文件: a.txt, b.txt, c.txt, d.txt, e.txt = 5
        assert_eq!(tree.count_used_files(), 5);
    }
}
