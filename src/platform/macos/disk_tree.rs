//! macOS 磁盘空间分析后备实现（SizeTree / Node / ScanResult）

use crate::core::disk::VolumeId;
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
///
/// 名字存在 `name_pool: Vec<u8>` 连续池里，`TreeEntry` 只存
/// `(name_off, name_len)`，省掉每条记录一次 String 堆分配。
/// 6.6M 条目 × ~40 字节 allocator overhead ≈ 260MB。
#[derive(Clone, Debug)]
pub struct SizeTree {
    volume: VolumeId,
    entries: Vec<TreeEntry>,
    name_pool: Vec<u8>,
    dir_size: Vec<u64>,
    dir_files: Vec<u64>,
    child_start: Vec<u32>,
    child_at: Vec<u32>,
}

/// SizeTree 的内部条目，与 Windows 的 `Entry` 等价。
///
/// 名字不存 `String` 而存 `(name_off, name_len)`，指向
/// `SizeTree::name_pool` 中的 UTF-8 字节区间。定长 36 字节，
/// vs 原 `String` 版本 52 字节 + 堆分配。
#[derive(Clone, Debug, Default)]
pub struct TreeEntry {
    pub parent: u32,
    pub name_off: u32,
    pub name_len: u16,
    pub is_dir: bool,
    pub used: bool,
    pub size: u64,
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
        let mut name_pool = Vec::new();
        let name_off = name_pool.len() as u32;
        name_pool.extend_from_slice(label.as_bytes());
        Self {
            volume,
            entries: vec![TreeEntry {
                parent: 0,
                name_off,
                name_len: label.len() as u16,
                is_dir: true,
                size: 0,
                used: true,
                mtime: 0,
            }],
            name_pool,
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
        name_pool: Vec<u8>,
        dir_size: Vec<u64>,
        dir_files: Vec<u64>,
        child_start: Vec<u32>,
        child_at: Vec<u32>,
    ) -> Self {
        Self {
            volume,
            entries,
            name_pool,
            dir_size,
            dir_files,
            child_start,
            child_at,
        }
    }

    /// 把名字追加到池里，返回 `(name_off, name_len)`。
    /// 供 `build_from_entries` / `append_subtree` 等构造路径使用。
    fn pool_push(&mut self, name: &str) -> (u32, u16) {
        let off = self.name_pool.len() as u32;
        self.name_pool.extend_from_slice(name.as_bytes());
        (off, name.len() as u16)
    }

    /// 取节点名字的 `&str` 引用（零拷贝，直接从 name_pool 切）。
    fn entry_name_str(&self, idx: u32) -> &str {
        let e = &self.entries[idx as usize];
        let off = e.name_off as usize;
        let end = off + e.name_len as usize;
        std::str::from_utf8(&self.name_pool[off..end]).unwrap_or("")
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
                name: self.entry_name_str(index as u32).to_string(),
                is_dir: entry.is_dir,
                size: entry.size,
                used: true,
                mtime: entry.mtime,
            })
            .collect()
    }

    /// 从紧凑持久化节点重建运行时目录树。
    pub fn from_compact(volume: VolumeId, compact: Vec<TreeIndexEntry>) -> Self {
        let n = compact.len();
        let mut entries = Vec::with_capacity(n);
        let mut name_pool = Vec::with_capacity(n * 16);
        for entry in compact {
            let name_off = name_pool.len() as u32;
            name_pool.extend_from_slice(entry.name.as_bytes());
            entries.push(TreeEntry {
                parent: entry.parent,
                name_off,
                name_len: entry.name.len() as u16,
                is_dir: entry.is_dir,
                size: entry.size,
                used: entry.used,
                mtime: entry.mtime,
            });
        }
        Self::build_from_entries_with_pool(volume, entries, name_pool)
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

        // 先用 (parent, name, is_dir, size, mtime) 中间结构，最后灌入 name_pool
        let mut raw: Vec<(u32, String, bool, u64, u64)> = Vec::with_capacity(snapshot.len() + 1);
        raw.push((ROOT_NODE, volume.display().to_string(), true, 0, 0));

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
            let idx = raw.len() as u32;
            raw.push((
                parent,
                name.to_string_lossy().into_owned(),
                entry.is_dir,
                if entry.is_dir { 0 } else { entry.size },
                entry.mtime,
            ));
            path_to_idx.insert(entry.path, idx);
        }

        Self::build_from_raw(volume, raw)
    }

    /// 从 `(parent, name, is_dir, size, mtime)` 列表构建树。
    /// `from_snapshot` 和 `from_compact` 的共用后端。
    fn build_from_raw(volume: VolumeId, raw: Vec<(u32, String, bool, u64, u64)>) -> Self {
        let n = raw.len();
        let mut entries = Vec::with_capacity(n);
        let mut name_pool = Vec::with_capacity(n * 16);
        for (parent, name, is_dir, size, mtime) in raw {
            let name_off = name_pool.len() as u32;
            name_pool.extend_from_slice(name.as_bytes());
            entries.push(TreeEntry {
                parent,
                name_off,
                name_len: name.len() as u16,
                is_dir,
                size,
                used: true,
                mtime,
            });
        }
        Self::build_from_entries_with_pool(volume, entries, name_pool)
    }

    /// 从已填好 name_pool 的 entries 构建 CSR 索引和聚合数组。
    fn build_from_entries_with_pool(
        volume: VolumeId,
        entries: Vec<TreeEntry>,
        name_pool: Vec<u8>,
    ) -> Self {
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

        Self::from_parts(
            volume,
            entries,
            name_pool,
            dir_size,
            dir_files,
            child_start,
            child_at,
        )
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
        self.entry_name_str(idx).to_string()
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
            let name = self.entry_name_str(node);
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
        self.entry_name_str(idx)
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
                    name: self.entry_name_str(c).to_string(),
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
                name: self.entry_name_str(i).to_string(),
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
                    if depth < max_depth
                        && !crate::core::disk::is_declutter_ignored_dir_name(self.entry_name_str(c))
                    {
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
                    let name = self.entry_name_str(c);
                    // macOS 文件系统大小写不敏感（APFS 默认），
                    // 用 eq_ignore_ascii_case 匹配，避免 Devin/devin 查不到。
                    #[cfg(not(windows))]
                    {
                        name.eq_ignore_ascii_case(&comp_str)
                    }
                    #[cfg(windows)]
                    {
                        name == comp_str.as_ref()
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

    /// 全树子串/通配符搜索（类似 Everything）。
    ///
    /// 大小写不敏感，匹配节点名（不含路径）。命中后沿父链回溯拼完整路径。
    /// `max_results` 截断结果数，避免命中太多时路径回溯吃 CPU。
    ///
    /// 查询语义：
    /// - 空查询 → 返回全树按大小降序的前 `max_results` 项
    /// - 含 `*` / `?` → 通配符匹配（`*` 任意长度，`?` 单字符）
    /// - 其他 → 大小写不敏感子串匹配
    pub fn search(&self, query: &str, max_results: usize) -> Vec<crate::core::disk::SearchHit> {
        if max_results == 0 {
            return Vec::new();
        }
        use crate::core::disk::NamePattern;
        let pattern = NamePattern::parse(query);
        if matches!(pattern, NamePattern::Empty) {
            return self.search_top_by_size(max_results);
        }
        let mut cache = HashMap::new();
        let mut hits = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            if !e.used {
                continue;
            }
            let name = self.entry_name_str(i as u32);
            let name_lower = name.to_ascii_lowercase();
            if !pattern.matches(&name_lower) {
                continue;
            }
            let path = self.path_of_with(i as u32, &mut cache);
            let size = if e.is_dir { self.dir_size[i] } else { e.size };
            hits.push(crate::core::disk::SearchHit {
                path,
                name: name.to_string(),
                is_dir: e.is_dir,
                size,
                mtime: e.mtime,
            });
            if hits.len() >= max_results {
                break;
            }
        }
        // 按大小降序，让大文件/大目录排前面
        hits.sort_unstable_by_key(|b| std::cmp::Reverse(b.size));
        hits
    }

    /// 空查询时返回全树按大小降序的前 `max_results` 项。
    ///
    /// 只对最终选出的 N 条做路径回溯，避免对百万级条目逐条回溯路径的
    /// CPU 开销。
    fn search_top_by_size(&self, max_results: usize) -> Vec<crate::core::disk::SearchHit> {
        let mut sized: Vec<(u64, u32)> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.used)
            .map(|(i, e)| {
                let size = if e.is_dir { self.dir_size[i] } else { e.size };
                (size, i as u32)
            })
            .collect();

        if sized.is_empty() {
            return Vec::new();
        }

        // 用 select_nth_unstable_by 在 O(n) 内切出最大的 max_results 个，
        // 再只对这 N 个排序。比全量 sort 快一个量级。
        if sized.len() > max_results {
            sized.select_nth_unstable_by(max_results - 1, |a, b| b.0.cmp(&a.0));
            sized.truncate(max_results);
        }
        sized.sort_unstable_by(|a, b| b.0.cmp(&a.0));

        let mut cache = HashMap::new();
        let mut hits = Vec::with_capacity(sized.len());
        for (size, idx) in sized {
            let e = &self.entries[idx as usize];
            let name = self.entry_name_str(idx).to_string();
            let path = self.path_of_with(idx, &mut cache);
            hits.push(crate::core::disk::SearchHit {
                path,
                name,
                is_dir: e.is_dir,
                size,
                mtime: e.mtime,
            });
        }
        hits
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
        let name_str = name.to_string_lossy();
        let (name_off, name_len) = self.pool_push(&name_str);

        self.entries.push(TreeEntry {
            parent,
            name_off,
            name_len,
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
                root_name
            } else {
                subtree.entry_name_str(i as u32)
            };
            let (name_off, name_len) = self.pool_push(name);
            let new_dir_size = if entry.is_dir { subtree.dir_size[i] } else { 0 };
            let new_dir_files = if entry.is_dir {
                subtree.dir_files[i]
            } else {
                0
            };
            self.entries.push(TreeEntry {
                parent: new_parent,
                name_off,
                name_len,
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
