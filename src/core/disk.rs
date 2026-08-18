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

#[cfg(windows)]
pub use crate::platform::windows::mft::{
    DirUsage, Node, ScanError, ScanResult, SizeTree, ROOT_RECORD as ROOT_NODE,
};

#[cfg(not(windows))]
pub mod fallback {
    use super::VolumeId;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

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

    /// macOS / 非 Windows 平台的目录树。
    ///
    /// 与 Windows 的 `SizeTree` API 完全一致，但内部用紧凑数组存储，
    /// 由 `platform::macos::walk::build_size_tree` 构造。
    #[derive(Clone, Debug)]
    pub struct SizeTree {
        volume: VolumeId,
        entries: Vec<TreeEntry>,
        dir_size: Vec<u64>,
        dir_files: Vec<u64>,
        child_start: Vec<u32>,
        child_at: Vec<u32>,
    }

    /// SizeTree 的内部条目，与 Windows 的 `Entry` 等价。
    #[derive(Clone, Debug, Default)]
    pub struct TreeEntry {
        pub parent: u32,
        pub name: String,
        pub is_dir: bool,
        pub size: u64,
        pub used: bool,
        /// 文件最后修改时间（Unix 秒），目录为 0。
        pub mtime: u64,
    }

    /// 持久化索引使用的路径节点，适合测试和局部子树合并。
    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
    pub struct TreeSnapshotEntry {
        pub path: PathBuf,
        pub is_dir: bool,
        /// 文件为直接分配大小，目录为聚合后的实际占用。
        pub size: u64,
        /// 文件最后修改时间（Unix 秒），目录为 0。
        #[serde(default)]
        pub mtime: u64,
    }

    /// 紧凑持久化节点：只保存父节点下标和名称，不重复保存完整路径。
    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
    pub struct TreeIndexEntry {
        pub parent: u32,
        pub name: String,
        pub is_dir: bool,
        pub size: u64,
        pub used: bool,
        /// 文件最后修改时间（Unix 秒），目录为 0。
        #[serde(default)]
        pub mtime: u64,
    }

    impl Default for SizeTree {
        fn default() -> Self {
            Self::empty(VolumeId::from_mount_point(PathBuf::from("/")))
        }
    }

    impl SizeTree {
        /// 构造一棵空树（只有根节点占位）。
        pub fn empty(volume: VolumeId) -> Self {
            let label = volume.display().to_string();
            Self {
                volume,
                entries: vec![TreeEntry {
                    parent: 0,
                    name: label,
                    is_dir: true,
                    size: 0,
                    used: true,
                    mtime: 0,
                }],
                dir_size: vec![0],
                dir_files: vec![0],
                child_start: vec![0, 0],
                child_at: vec![],
            }
        }

        /// 从原始部件构造一棵树。仅供 `platform::macos::walk` 调用。
        pub fn from_parts(
            volume: VolumeId,
            entries: Vec<TreeEntry>,
            dir_size: Vec<u64>,
            dir_files: Vec<u64>,
            child_start: Vec<u32>,
            child_at: Vec<u32>,
        ) -> Self {
            Self {
                volume,
                entries,
                dir_size,
                dir_files,
                child_start,
                child_at,
            }
        }

        /// 把完整目录树转换成适合持久化的扁平索引。
        pub fn snapshot_entries(&self) -> Vec<TreeSnapshotEntry> {
            let mut cache = HashMap::new();
            self.entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.used)
                .map(|(idx, entry)| TreeSnapshotEntry {
                    path: PathBuf::from(self.path_of_with(idx as u32, &mut cache)),
                    is_dir: entry.is_dir,
                    size: self.size_of(idx as u32),
                    mtime: entry.mtime,
                })
                .collect()
        }

        /// 转成紧凑持久化格式，避免为每个节点复制完整路径。
        pub fn compact_entries(&self) -> Vec<TreeIndexEntry> {
            // 增量替换会把旧节点标成 unused 并在末尾追加新节点。若把这些
            // 墓碑也持久化，索引会在每轮保存后持续膨胀；日志中有效节点约
            // 1721 万，但缓存已经增长到 3161 万条。保存时过滤墓碑并重映射
            // parent，下次加载恢复成真正紧凑的树。
            let mut remap = vec![u32::MAX; self.entries.len()];
            let mut next = 0u32;
            for (index, entry) in self.entries.iter().enumerate() {
                if entry.used {
                    remap[index] = next;
                    next += 1;
                }
            }

            self.entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.used)
                .map(|(index, entry)| TreeIndexEntry {
                    parent: if index == ROOT_NODE as usize {
                        ROOT_NODE
                    } else {
                        remap[entry.parent as usize]
                    },
                    name: entry.name.clone(),
                    is_dir: entry.is_dir,
                    size: entry.size,
                    used: true,
                    mtime: entry.mtime,
                })
                .collect()
        }

        /// 从紧凑持久化节点重建运行时目录树。
        pub fn from_compact(volume: VolumeId, compact: Vec<TreeIndexEntry>) -> Self {
            let entries: Vec<TreeEntry> = compact
                .into_iter()
                .map(|entry| TreeEntry {
                    parent: entry.parent,
                    name: entry.name,
                    is_dir: entry.is_dir,
                    size: entry.size,
                    used: entry.used,
                    mtime: entry.mtime,
                })
                .collect();
            Self::build_from_entries(volume, entries)
        }

        /// 从持久化的扁平节点重建运行时目录树。
        pub fn from_snapshot(volume: VolumeId, mut snapshot: Vec<TreeSnapshotEntry>) -> Self {
            let root_path = volume.mount_point().to_path_buf();
            snapshot.retain(|entry| entry.path == root_path || entry.path.starts_with(&root_path));
            snapshot.sort_by_key(|entry| {
                entry
                    .path
                    .components()
                    .filter(|component| matches!(component, std::path::Component::Normal(_)))
                    .count()
            });

            let mut entries = vec![TreeEntry {
                parent: ROOT_NODE,
                name: volume.display().to_string(),
                is_dir: true,
                size: 0,
                used: true,
                mtime: 0,
            }];
            let mut path_to_idx = HashMap::new();
            path_to_idx.insert(root_path.clone(), ROOT_NODE);

            for entry in snapshot {
                if entry.path == root_path {
                    continue;
                }
                let Some(name) = entry.path.file_name() else {
                    continue;
                };
                let Some(parent_path) = entry.path.parent() else {
                    continue;
                };
                let Some(&parent) = path_to_idx.get(parent_path) else {
                    continue;
                };
                let idx = entries.len() as u32;
                entries.push(TreeEntry {
                    parent,
                    name: name.to_string_lossy().into_owned(),
                    is_dir: entry.is_dir,
                    size: if entry.is_dir { 0 } else { entry.size },
                    used: true,
                    mtime: entry.mtime,
                });
                path_to_idx.insert(entry.path, idx);
            }

            Self::build_from_entries(volume, entries)
        }

        fn build_from_entries(volume: VolumeId, entries: Vec<TreeEntry>) -> Self {
            let n = entries.len();
            let mut dir_size = vec![0u64; n];
            let mut dir_files = vec![0u64; n];
            for i in 0..n {
                if entries[i].is_dir {
                    continue;
                }
                let mut current = entries[i].parent;
                loop {
                    let parent = current as usize;
                    if parent >= n {
                        break;
                    }
                    dir_size[parent] += entries[i].size;
                    dir_files[parent] += 1;
                    if current == ROOT_NODE || entries[parent].parent == current {
                        break;
                    }
                    current = entries[parent].parent;
                }
            }

            let mut child_counts = vec![0u32; n];
            for entry in entries.iter().skip(1) {
                let parent = entry.parent as usize;
                if parent < n {
                    child_counts[parent] += 1;
                }
            }
            let mut child_start = vec![0u32; n + 1];
            for i in 0..n {
                child_start[i + 1] = child_start[i] + child_counts[i];
            }
            let mut child_at = vec![0u32; child_start[n] as usize];
            let mut cursor = child_start[..n].to_vec();
            for (idx, entry) in entries.iter().enumerate().skip(1) {
                let parent = entry.parent as usize;
                if parent < n {
                    child_at[cursor[parent] as usize] = idx as u32;
                    cursor[parent] += 1;
                }
            }

            Self::from_parts(volume, entries, dir_size, dir_files, child_start, child_at)
        }

        pub fn volume(&self) -> &VolumeId {
            &self.volume
        }

        pub fn root(&self) -> u32 {
            ROOT_NODE
        }

        pub fn valid(&self, idx: u32) -> bool {
            let i = idx as usize;
            i < self.entries.len() && self.entries[i].used
        }

        pub fn is_dir(&self, idx: u32) -> bool {
            self.valid(idx) && self.entries[idx as usize].is_dir
        }

        pub fn name_of(&self, idx: u32) -> String {
            if idx == ROOT_NODE {
                return self.volume.display().to_string();
            }
            if !self.valid(idx) {
                return String::new();
            }
            self.entries[idx as usize].name.clone()
        }

        pub fn size_of(&self, idx: u32) -> u64 {
            if !self.valid(idx) {
                return 0;
            }
            let e = &self.entries[idx as usize];
            if e.is_dir {
                self.dir_size[idx as usize]
            } else {
                e.size
            }
        }

        pub fn file_count_of(&self, idx: u32) -> u64 {
            if !self.valid(idx) {
                return 0;
            }
            if self.entries[idx as usize].is_dir {
                self.dir_files[idx as usize]
            } else {
                1
            }
        }

        pub fn parent_of(&self, idx: u32) -> Option<u32> {
            if idx == ROOT_NODE || !self.valid(idx) {
                return None;
            }
            let p = self.entries[idx as usize].parent;
            if p == idx || !self.valid(p) {
                None
            } else {
                Some(p)
            }
        }

        /// 局部移除子树：标记节点及所有后代为 unused，沿父链扣减聚合大小。
        ///
        /// 不释放数组内存——紧凑布局的随机访问是扫描性能的基础，
        /// 不能因为删一个目录就搬移整块下标。`used = false` 的条目
        /// 在 `children()` / `largest_files()` 等遍历里被自动跳过。
        pub fn remove_subtree(&mut self, idx: u32, removed_size: u64, removed_files: u64) {
            if !self.valid(idx) {
                return;
            }

            // 递归标记子树所有节点为 unused
            let mut stack = vec![idx];
            while let Some(cur) = stack.pop() {
                if !self.valid(cur) {
                    continue;
                }
                let i = cur as usize;
                self.entries[i].used = false;
                self.dir_size[i] = 0;
                self.dir_files[i] = 0;
                // 把子节点压栈继续处理
                for &child in self.child_slice(cur).iter() {
                    if self.valid(child) {
                        stack.push(child);
                    }
                }
            }

            // 沿父链扣减祖先目录的聚合大小和文件数
            let mut cur = self.entries[idx as usize].parent;
            loop {
                if !self.valid(cur) {
                    break;
                }
                let i = cur as usize;
                self.dir_size[i] = self.dir_size[i].saturating_sub(removed_size);
                self.dir_files[i] = self.dir_files[i].saturating_sub(removed_files);
                if cur == ROOT_NODE {
                    break;
                }
                cur = self.entries[i].parent;
            }
        }

        pub fn path_of(&self, idx: u32) -> String {
            let mut cache = HashMap::new();
            self.path_of_with(idx, &mut cache)
        }

        pub fn path_of_with(&self, idx: u32, cache: &mut HashMap<u32, String>) -> String {
            self.resolve_path(idx, cache)
        }

        fn resolve_path(&self, idx: u32, cache: &mut HashMap<u32, String>) -> String {
            if idx == ROOT_NODE {
                return self.volume().mount_point().display().to_string();
            }
            if let Some(hit) = cache.get(&idx) {
                return hit.clone();
            }
            if !self.valid(idx) {
                return String::new();
            }

            // 回溯父链
            let mut chain: Vec<u32> = Vec::new();
            let mut cur = idx;
            let mut base = String::new();
            let mut depth = 0;
            const MAX_DEPTH: usize = 64;

            loop {
                if cur == ROOT_NODE || depth > MAX_DEPTH {
                    base = self.volume().mount_point().display().to_string();
                    break;
                }
                if let Some(hit) = cache.get(&cur) {
                    base = hit.clone();
                    break;
                }
                let i = cur as usize;
                if i >= self.entries.len() || !self.entries[i].used {
                    break;
                }
                chain.push(cur);
                let next = self.entries[i].parent;
                if next == cur {
                    break;
                }
                cur = next;
                depth += 1;
            }

            let mut path = base;
            for &node in chain.iter().rev() {
                let name = &self.entries[node as usize].name;
                if !path.ends_with('/') {
                    path.push('/');
                }
                path.push_str(name);
                cache.insert(node, path.clone());
            }
            path
        }

        fn child_slice(&self, idx: u32) -> &[u32] {
            let i = idx as usize;
            if i + 1 >= self.child_start.len() {
                return &[];
            }
            let (a, b) = (
                self.child_start[i] as usize,
                self.child_start[i + 1] as usize,
            );
            &self.child_at[a..b]
        }

        fn own_size(&self, idx: u32) -> u64 {
            self.child_slice(idx)
                .iter()
                .filter(|&&c| self.valid(c) && !self.entries[c as usize].is_dir)
                .map(|&c| self.entries[c as usize].size)
                .sum()
        }

        pub fn child_indices(&self, idx: u32) -> &[u32] {
            self.child_slice(idx)
        }

        pub fn entry_name(&self, idx: u32) -> &str {
            if !self.valid(idx) {
                return "";
            }
            &self.entries[idx as usize].name
        }

        pub fn children(&self, idx: u32) -> Vec<Node> {
            let mut out: Vec<Node> = self
                .child_slice(idx)
                .iter()
                .filter(|&&c| self.valid(c))
                .map(|&c| {
                    let e = &self.entries[c as usize];
                    Node {
                        idx: c,
                        name: e.name.clone(),
                        is_dir: e.is_dir,
                        size: if e.is_dir {
                            self.dir_size[c as usize]
                        } else {
                            e.size
                        },
                        file_count: if e.is_dir {
                            self.dir_files[c as usize]
                        } else {
                            1
                        },
                        own_size: if e.is_dir { self.own_size(c) } else { e.size },
                    }
                })
                .collect();
            out.sort_unstable_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
            out
        }

        pub fn largest_files(&self, n: usize) -> Vec<Node> {
            use std::cmp::Reverse;
            use std::collections::BinaryHeap;

            if n == 0 {
                return Vec::new();
            }

            let mut heap: BinaryHeap<Reverse<(u64, u32)>> = BinaryHeap::with_capacity(n + 1);
            for (i, e) in self.entries.iter().enumerate() {
                if !e.used || e.is_dir || e.size == 0 {
                    continue;
                }
                if heap.len() == n && e.size <= heap.peek().map(|Reverse((s, _))| *s).unwrap_or(0) {
                    continue;
                }
                heap.push(Reverse((e.size, i as u32)));
                if heap.len() > n {
                    heap.pop();
                }
            }

            let mut files: Vec<(u64, u32)> = heap.into_iter().map(|Reverse(v)| v).collect();
            files.sort_unstable_by_key(|b| std::cmp::Reverse(b.0));
            files
                .into_iter()
                .map(|(size, i)| Node {
                    idx: i,
                    name: self.entries[i as usize].name.clone(),
                    is_dir: false,
                    size,
                    file_count: 1,
                    own_size: size,
                })
                .collect()
        }

        /// 递归遍历指定子树（带最大深度和目录过滤），收集所有符合条件的
        /// 文件节点下标、体积与修改时间。
        pub fn collect_subtree_files(
            &self,
            root_idx: u32,
            max_depth: usize,
            min_size: u64,
            max_size: u64,
        ) -> Vec<(u32, u64, u64)> {
            let mut out = Vec::new();
            let mut stack = vec![(root_idx, 0usize)];
            while let Some((cur, depth)) = stack.pop() {
                if !self.valid(cur) {
                    continue;
                }
                for &c in self.child_slice(cur) {
                    if !self.valid(c) {
                        continue;
                    }
                    let e = &self.entries[c as usize];
                    if e.is_dir {
                        if depth < max_depth && !super::is_declutter_ignored_dir_name(&e.name) {
                            stack.push((c, depth + 1));
                        }
                    } else if e.size >= min_size && e.size <= max_size {
                        out.push((c, e.size, e.mtime));
                    }
                }
            }
            out
        }

        pub fn find_path(&self, full_path: &Path) -> Vec<u32> {
            let mut path_indices = vec![self.root()];
            let relative = full_path
                .strip_prefix(self.volume.mount_point())
                .unwrap_or(full_path);
            let comps: Vec<std::ffi::OsString> = relative
                .components()
                .filter_map(|c| match c {
                    std::path::Component::Normal(s) => Some(s.to_os_string()),
                    _ => None,
                })
                .collect();

            let mut cur = self.root();
            for comp in comps {
                let comp_str = comp.to_string_lossy();
                if comp_str.is_empty() {
                    continue;
                }
                let hit = self.child_slice(cur).iter().copied().find(|&c| {
                    self.valid(c) && {
                        // macOS 文件系统大小写不敏感（APFS 默认），
                        // 用 eq_ignore_ascii_case 匹配，避免 Devin/devin 查不到。
                        #[cfg(not(windows))]
                        {
                            self.entries[c as usize]
                                .name
                                .eq_ignore_ascii_case(&comp_str)
                        }
                        #[cfg(windows)]
                        {
                            self.entries[c as usize].name == comp_str
                        }
                    }
                });
                match hit {
                    Some(idx) => {
                        cur = idx;
                        path_indices.push(cur);
                    }
                    None => break,
                }
            }
            path_indices
        }

        pub fn find_node_by_path(&self, full_path: &Path) -> Option<u32> {
            let relative = full_path
                .strip_prefix(self.volume.mount_point())
                .unwrap_or(full_path);
            let comps_count = relative
                .components()
                .filter(|c| matches!(c, std::path::Component::Normal(_)))
                .count();
            let chain = self.find_path(full_path);
            if comps_count > 0 && chain.len() == comps_count + 1 {
                chain.last().copied()
            } else if comps_count == 0 {
                Some(self.root())
            } else {
                None
            }
        }

        pub fn remove_node(&mut self, _idx: u32) {
            // macOS 上目前不实现就地删除——清理走的是 cleaner 模块
        }

        // ---- 就地子树替换 API（增量索引更新用） ----

        /// 就地标记子树为未使用，并沿父链减去对应的聚合大小和文件数。
        ///
        /// 不修改 `entries` 数组的大小，只标记 `used = false`。
        /// CSR 子数组在后续 `rebuild_child_arrays` 调用时统一重建。
        pub fn remove_subtree_inplace(&mut self, idx: u32) {
            if !self.valid(idx) {
                return;
            }
            let (size, files) = self.subtree_totals(idx);
            let children: Vec<u32> = self.child_slice(idx).to_vec();
            self.entries[idx as usize].used = false;
            for child in children {
                self.mark_unused_recursive(child);
            }
            // 沿父链减去被移除子树的大小和文件数
            let mut cur = self.entries[idx as usize].parent;
            loop {
                if cur == idx || !self.valid(cur) {
                    break;
                }
                let i = cur as usize;
                if self.entries[i].is_dir {
                    self.dir_size[i] = self.dir_size[i].saturating_sub(size);
                    self.dir_files[i] = self.dir_files[i].saturating_sub(files);
                }
                if cur == ROOT_NODE {
                    break;
                }
                cur = self.entries[i].parent;
            }
        }

        /// 以当前文件系统大小新增或替换单个文件，并同步更新所有祖先聚合值。
        ///
        /// FSEvents 开启 FileEvents 后会给出精确文件路径。文件内容变化不应
        /// 退化成重扫其整个父目录（例如 `~/work/.DS_Store` 会让 300 万节点
        /// 的工作区被完整遍历）。父目录不在索引中时返回 false，由调用方
        /// 回退到目录子树扫描。
        pub fn upsert_file(&mut self, path: &Path, size: u64) -> bool {
            self.upsert_file_with_mtime(path, size, 0)
        }

        /// 与 [`upsert_file`] 相同，但同时设置 mtime。
        pub fn upsert_file_with_mtime(&mut self, path: &Path, size: u64, mtime: u64) -> bool {
            let Some(parent_path) = path.parent() else {
                return false;
            };
            let Some(parent) = self.find_node_by_path(parent_path) else {
                return false;
            };
            if !self.is_dir(parent) {
                return false;
            }
            if let Some(existing) = self.find_node_by_path(path) {
                self.remove_subtree_inplace(existing);
            }
            let Some(name) = path.file_name() else {
                return false;
            };

            self.entries.push(TreeEntry {
                parent,
                name: name.to_string_lossy().into_owned(),
                is_dir: false,
                size,
                used: true,
                mtime,
            });
            self.dir_size.push(0);
            self.dir_files.push(0);

            let mut cur = parent;
            loop {
                let i = cur as usize;
                self.dir_size[i] = self.dir_size[i].saturating_add(size);
                self.dir_files[i] = self.dir_files[i].saturating_add(1);
                if cur == ROOT_NODE {
                    break;
                }
                cur = self.entries[i].parent;
            }
            true
        }

        fn mark_unused_recursive(&mut self, idx: u32) {
            if !self.valid(idx) {
                return;
            }
            let children: Vec<u32> = self.child_slice(idx).to_vec();
            self.entries[idx as usize].used = false;
            for child in children {
                self.mark_unused_recursive(child);
            }
        }

        fn subtree_totals(&self, idx: u32) -> (u64, u64) {
            if !self.valid(idx) {
                return (0, 0);
            }
            if self.entries[idx as usize].is_dir {
                (self.dir_size[idx as usize], self.dir_files[idx as usize])
            } else {
                (self.entries[idx as usize].size, 1)
            }
        }

        /// 在指定父节点下追加一棵子树的所有节点。
        ///
        /// 子树的根节点（idx 0）映射为 `parent_idx` 的子节点。
        /// `root_name` 覆盖子树根节点的名称（因为子树是用完整路径作为
        /// volume label 扫描的，名称是完整路径，需要替换为目录名）。
        ///
        /// 调用后需调用 `rebuild_child_arrays` 重建 CSR 索引。
        pub fn append_subtree(&mut self, parent_idx: u32, subtree: &SizeTree, root_name: &str) {
            if !self.valid(parent_idx) || !self.entries[parent_idx as usize].is_dir {
                return;
            }
            let base = self.entries.len() as u32;
            let (sub_total_size, sub_total_files) = subtree.subtree_totals(subtree.root());

            self.entries.reserve(subtree.entries.len());
            self.dir_size.reserve(subtree.entries.len());
            self.dir_files.reserve(subtree.entries.len());

            for (i, entry) in subtree.entries.iter().enumerate() {
                if !entry.used {
                    continue;
                }
                let new_parent = if i == 0 {
                    parent_idx
                } else {
                    base + entry.parent
                };
                let name = if i == 0 {
                    root_name.to_string()
                } else {
                    entry.name.clone()
                };
                let new_dir_size = if entry.is_dir { subtree.dir_size[i] } else { 0 };
                let new_dir_files = if entry.is_dir {
                    subtree.dir_files[i]
                } else {
                    0
                };
                self.entries.push(TreeEntry {
                    parent: new_parent,
                    name,
                    is_dir: entry.is_dir,
                    size: entry.size,
                    used: true,
                    mtime: entry.mtime,
                });
                self.dir_size.push(new_dir_size);
                self.dir_files.push(new_dir_files);
            }

            // 沿父链加上新子树的聚合大小
            let mut cur = parent_idx;
            loop {
                if !self.valid(cur) {
                    break;
                }
                let i = cur as usize;
                if self.entries[i].is_dir {
                    self.dir_size[i] += sub_total_size;
                    self.dir_files[i] += sub_total_files;
                }
                if cur == ROOT_NODE {
                    break;
                }
                cur = self.entries[i].parent;
            }
        }

        /// 从 entries 数组重建 CSR 子节点索引。
        /// 在完成所有 `append_subtree` / `remove_subtree_inplace` 操作后调用一次。
        pub fn rebuild_child_arrays(&mut self) {
            let n = self.entries.len();
            let mut child_counts = vec![0u32; n];
            for (i, entry) in self.entries.iter().enumerate() {
                if i == 0 || !entry.used {
                    continue;
                }
                let p = entry.parent as usize;
                if p < n && self.entries[p].used {
                    child_counts[p] += 1;
                }
            }
            let mut child_start = vec![0u32; n + 1];
            for i in 0..n {
                child_start[i + 1] = child_start[i] + child_counts[i];
            }
            let mut child_at = vec![0u32; child_start[n] as usize];
            let mut cursor = child_start[..n].to_vec();
            for (i, entry) in self.entries.iter().enumerate() {
                if i == 0 || !entry.used {
                    continue;
                }
                let p = entry.parent as usize;
                if p < n && self.entries[p].used {
                    child_at[cursor[p] as usize] = i as u32;
                    cursor[p] += 1;
                }
            }
            self.child_start = child_start;
            self.child_at = child_at;
        }

        /// 统计当前已使用（`used = true`）的目录节点数。
        pub fn count_used_dirs(&self) -> u64 {
            self.entries.iter().filter(|e| e.used && e.is_dir).count() as u64
        }

        /// 统计当前已使用（`used = true`）的文件节点数。
        pub fn count_used_files(&self) -> u64 {
            self.entries.iter().filter(|e| e.used && !e.is_dir).count() as u64
        }

        /// 从头重新计算所有目录的聚合大小和文件数。
        /// 仅供测试验证增量更新正确性时使用。
        pub fn recompute_aggregates(&mut self) {
            let n = self.entries.len();
            self.dir_size = vec![0u64; n];
            self.dir_files = vec![0u64; n];
            for i in 0..n {
                if !self.entries[i].used || self.entries[i].is_dir {
                    continue;
                }
                let mut cur = self.entries[i].parent;
                loop {
                    let idx = cur as usize;
                    if idx >= n || !self.entries[idx].used {
                        break;
                    }
                    self.dir_size[idx] += self.entries[i].size;
                    self.dir_files[idx] += 1;
                    if cur == ROOT_NODE {
                        break;
                    }
                    cur = self.entries[idx].parent;
                }
            }
        }
    }

    #[derive(Clone, Debug)]
    pub struct ScanResult {
        pub volume: VolumeId,
        pub total_size: u64,
        pub file_count: u64,
        pub dir_count: u64,
        pub dirs: Vec<DirUsage>,
        pub tree: SizeTree,
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

    impl ScanResult {
        /// 从树中局部移除指定路径，扣减祖先目录的大小和文件数。
        ///
        /// 不重扫整棵树——删除的影响是局部的，只需标记子树为 unused
        /// 并沿父链扣减聚合值。UI 立即看到更新，无需等全量重扫。
        pub fn remove_path(&mut self, path: &Path) {
            if let Some(idx) = self.tree.find_node_by_path(path) {
                let removed_size = self.tree.size_of(idx);
                let removed_files = self.tree.file_count_of(idx);
                self.tree.remove_subtree(idx, removed_size, removed_files);
                // 总量也同步扣减
                self.total_size = self.total_size.saturating_sub(removed_size);
                self.file_count = self.file_count.saturating_sub(removed_files);
            }
        }
    }

    #[derive(Debug)]
    pub enum ScanError {
        AccessDenied,
        NotNtfs,
        Io(String),
    }

    impl std::fmt::Display for ScanError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                ScanError::AccessDenied => write!(f, "需要管理员权限"),
                ScanError::NotNtfs => write!(f, "不是 NTFS 卷"),
                ScanError::Io(e) => write!(f, "IO 错误: {e}"),
            }
        }
    }
}

#[cfg(not(windows))]
pub use fallback::*;

use std::collections::{HashMap, HashSet};

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
    fn build_test_tree() -> super::fallback::SizeTree {
        use super::fallback::{SizeTree, TreeIndexEntry};
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
        use super::fallback::{SizeTree, TreeIndexEntry};
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
