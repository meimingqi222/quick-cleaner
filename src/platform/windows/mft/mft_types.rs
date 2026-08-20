//! MFT 类型定义：SizeTree / ScanResult / ScanError

use super::mft_parser::Entry;
use super::mft_scanner::resolve_path;
use std::collections::HashMap;

use crate::core::disk::VolumeId;

/// NTFS 把卷根目录固定放在 `$MFT` 的 5 号记录上。
/// 对外以 `core::disk::ROOT_NODE` 的名字导出，UI 层不该直接写字面量 5。
pub const ROOT_RECORD: u32 = 5;
pub(super) const MAX_DEPTH: usize = 256;
pub(super) const CHUNK_BYTES: usize = 8 * 1024 * 1024;

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

/// 扫描后保留下来的完整目录树，支持像 WizTree 那样逐层下钻。
///
/// 名字存在 `name_pool: Vec<u8>` 连续池里，`Entry` 只存
/// `(name_off, name_len)`，省掉每条记录一次 String 堆分配。
/// 6.3M 条目 × ~40 字节 allocator overhead ≈ 250MB。
#[derive(Clone)]
pub struct SizeTree {
    pub(super) volume: VolumeId,
    pub(super) entries: Vec<Entry>,
    pub(super) name_pool: Vec<u8>,
    pub(super) dir_size: Vec<u64>,
    pub(super) dir_files: Vec<u64>,
    pub(super) child_start: Vec<u32>,
    pub(super) child_at: Vec<u32>,
}

impl std::fmt::Debug for SizeTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SizeTree({}: {} 条记录)",
            self.volume,
            self.entries.len()
        )
    }
}

impl SizeTree {
    pub fn volume(&self) -> &VolumeId {
        &self.volume
    }

    pub fn root(&self) -> u32 {
        ROOT_RECORD
    }

    pub fn valid(&self, idx: u32) -> bool {
        let i = idx as usize;
        i < self.entries.len() && self.entries[i].used
    }

    pub fn is_dir(&self, idx: u32) -> bool {
        self.valid(idx) && self.entries[idx as usize].is_dir
    }

    /// 取节点名字的 `&str` 引用（零拷贝，直接从 name_pool 切）。
    fn entry_name_str(&self, idx: u32) -> &str {
        let e = &self.entries[idx as usize];
        let off = e.name_off as usize;
        let end = off + e.name_len as usize;
        std::str::from_utf8(&self.name_pool[off..end]).unwrap_or("")
    }

    pub fn name_of(&self, idx: u32) -> String {
        if idx == ROOT_RECORD {
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
        if idx == ROOT_RECORD || !self.valid(idx) {
            return None;
        }
        let p = self.entries[idx as usize].parent;
        if p == idx || !self.valid(p) {
            None
        } else {
            Some(p)
        }
    }

    /// 解析单个节点的完整路径。
    ///
    /// 每次调用都会新建一次性缓存，只适合零星查询。批量解析（例如渲染
    /// 一屏目录）务必用 [`SizeTree::path_of_with`] 复用同一个缓存，
    /// 否则每一行都要从头回溯到根。
    pub fn path_of(&self, idx: u32) -> String {
        let mut cache = HashMap::new();
        self.path_of_with(idx, &mut cache)
    }

    /// 复用调用方持有的缓存解析路径。同一批次里父链会被逐级记住。
    pub fn path_of_with(&self, idx: u32, cache: &mut HashMap<u32, String>) -> String {
        resolve_path(&self.entries, &self.name_pool, idx, &self.volume, cache)
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

    /// 该记录是否是 NTFS 自身的元数据，不该出现在用户可见的目录树里。
    ///
    /// 名称黑名单统一由 [`crate::core::safety`] 维护；这里只额外处理
    /// 「前 16 条记录里以 `$` 开头」这个 MFT 特有的判据。
    pub fn is_ntfs_system_meta(idx: u32, name: &str) -> bool {
        if idx < 16 && (name.starts_with('$') || name == ".") {
            return true;
        }
        crate::core::safety::is_ntfs_meta_name(name)
    }

    /// 子节点的原始下标切片，不分配、不排序。
    ///
    /// [`SizeTree::children`] 会克隆每个子节点的名字并按体积排序，适合渲染
    /// 一屏列表；全树遍历（如开发垃圾发现）必须用这个，否则光是构造
    /// `Vec<Node>` 就会淹没遍历本身。
    pub fn child_indices(&self, idx: u32) -> &[u32] {
        self.child_slice(idx)
    }

    /// 借用某条记录的名字，不复制。
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
            .filter(|&&c| self.valid(c) && !Self::is_ntfs_system_meta(c, self.entry_name_str(c)))
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

    /// 全盘最大的 n 个文件。
    ///
    /// 用定长小顶堆而不是「收集所有文件再排序」：C 盘的 MFT 常有上百万条
    /// 记录，全量 `Vec<(u64, u32)>` 光分配就是几十 MB，而这里只保留 n 条。
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
            // 堆已满时，先和当前最小值比一次，绝大多数记录到这里就被淘汰了，
            // 连元数据名字符串比较都省掉。
            if heap.len() == n && e.size <= heap.peek().map(|Reverse((s, _))| *s).unwrap_or(0) {
                continue;
            }
            if Self::is_ntfs_system_meta(i as u32, self.entry_name_str(i as u32)) {
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

    /// 根据路径解析出对应的树节点层级链（如 [5, 12, 45, 99]）
    pub fn find_path(&self, full_path: &std::path::Path) -> Vec<u32> {
        let mut path_indices = vec![self.root()];
        let comps: Vec<std::ffi::OsString> = full_path
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
            // 直接在 child_slice 上线性查找。旧实现调 children()，那会克隆
            // 每个子节点的名字再按体积排序，只为了取其中一个匹配项。
            let hit =
                self.child_slice(cur).iter().copied().find(|&c| {
                    self.valid(c) && self.entry_name_str(c).eq_ignore_ascii_case(&comp_str)
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

    /// 根据路径查找最终对应的节点索引
    pub fn find_node_by_path(&self, full_path: &std::path::Path) -> Option<u32> {
        let comps_count = full_path
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

    /// 全树子串/通配符搜索（类似 Everything）。
    ///
    /// 大小写不敏感，匹配节点名（不含路径）。命中后沿父链回溯拼完整路径。
    /// `max_results` 截断结果数。跳过 NTFS 系统元数据节点。
    ///
    /// 查询语义：
    /// - 空查询 → 返回全树按大小降序的前 `max_results` 项
    /// - 含 `*` / `?` → 通配符匹配（`*` 任意长度，`?` 单字符）
    /// - 其他 → 大小写不敏感子串匹配
    ///
    /// 通配符路径用 `NamePattern` 一次性解析 pattern，避免每个文件重复
    /// 解析；子串路径保留 `contains` 快路径。
    pub fn search(&self, query: &str, max_results: usize) -> Vec<crate::core::disk::SearchHit> {
        if max_results == 0 {
            return Vec::new();
        }
        use crate::core::disk::NamePattern;
        let pattern = NamePattern::parse(query);
        if matches!(pattern, NamePattern::Empty) {
            return self.search_top_by_size(max_results);
        }
        let mut cache = std::collections::HashMap::new();
        let mut hits = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            if !e.used {
                continue;
            }
            // 跳过 NTFS 系统元数据（$MFT、$LogFile 等）
            if Self::is_ntfs_system_meta(i as u32, self.entry_name_str(i as u32)) {
                continue;
            }
            let name = self.entry_name_str(i as u32);
            // 大小写不敏感：文件名小写化后与 pattern 比较
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
    /// 跳过 NTFS 系统元数据节点。只对最终选出的 N 条做路径回溯，
    /// 避免对百万级条目逐条回溯路径的 CPU 开销。
    fn search_top_by_size(&self, max_results: usize) -> Vec<crate::core::disk::SearchHit> {
        // 收集所有有效条目的 (size, idx)
        let mut sized: Vec<(u64, u32)> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(i, e)| {
                e.used && !Self::is_ntfs_system_meta(*i as u32, self.entry_name_str(*i as u32))
            })
            .map(|(i, e)| {
                let size = if e.is_dir {
                    self.dir_size[i]
                } else {
                    e.size
                };
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

        let mut cache = std::collections::HashMap::new();
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

    /// 递归遍历指定子树（带最大深度和目录过滤），收集所有符合条件的文件节点下标、体积与修改时间
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
                    if depth < max_depth && !is_declutter_ignored_dir_name(self.entry_name_str(c)) {
                        stack.push((c, depth + 1));
                    }
                } else if e.size >= min_size && e.size <= max_size {
                    out.push((c, e.size, e.mtime));
                }
            }
        }
        out
    }

    /// 从内存树中即时扣除并标记已删除节点（同时扣减所有祖先目录大小与文件数）
    pub fn remove_node(&mut self, idx: u32) {
        if !self.valid(idx) || idx == self.root() {
            return;
        }
        let size = self.size_of(idx);
        let files = self.file_count_of(idx);
        self.entries[idx as usize].used = false;

        // 沿父链向上一路扣减各级祖先目录的大小和计数
        let mut cur = self.entries[idx as usize].parent;
        let mut visited = 0;
        while cur != idx && self.valid(cur) && visited < MAX_DEPTH {
            visited += 1;
            let ci = cur as usize;
            if ci < self.dir_size.len() {
                self.dir_size[ci] = self.dir_size[ci].saturating_sub(size);
            }
            if ci < self.dir_files.len() {
                self.dir_files[ci] = self.dir_files[ci].saturating_sub(files);
            }
            if cur == self.root() {
                break;
            }
            let next_p = self.entries[ci].parent;
            if next_p == cur {
                break;
            }
            cur = next_p;
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
    /// 快速就地剔除被删除的文件或文件夹并同步总盘符统计
    pub fn remove_path(&mut self, path: &std::path::Path) {
        if let Some(idx) = self.tree.find_node_by_path(path) {
            let size = self.tree.size_of(idx);
            let files = self.tree.file_count_of(idx);
            self.tree.remove_node(idx);
            self.total_size = self.total_size.saturating_sub(size);
            self.file_count = self.file_count.saturating_sub(files);
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
            ScanError::AccessDenied => write!(f, "需要管理员权限才能读取 $MFT"),
            ScanError::NotNtfs => write!(f, "该卷不是 NTFS 或无法获取卷信息"),
            ScanError::Io(e) => write!(f, "读取失败：{e}"),
        }
    }
}

// 跨平台共享的目录过滤函数，定义在 core::disk 中。
pub use crate::core::disk::is_declutter_ignored_dir_name;

// ---------------------------------------------------------------------------
// Windows 原生结构与 FFI
// ---------------------------------------------------------------------------
