//! macOS 磁盘空间分析后备实现（SizeTree / Node / ScanResult）

use super::index_v7::{
    delta_checksum, entries_as_bytes, finalize_checksum, index_checksum_bytes, pool_str,
    push_name, MmapOut, MmapPool, NameInterner, V7Header, V7Layout, INDEX_V7_HEADER,
    INDEX_V7_MAGIC,
};
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

/// `parent_bits` 布局：低 29 位父下标，bit 29 保留，bit 30 使用中，bit 31 目录。
const PARENT_MASK: u32 = 0x1FFF_FFFF;
const USED_BIT: u32 = 1 << 30;
const DIR_BIT: u32 = 1 << 31;

/// macOS / 非 Windows 平台的目录树。
///
/// 与 Windows 的 `SizeTree` API 完全一致，但内部用紧凑数组存储。
///
/// 每条 `TreeEntry` 固定 24 字节：聚合 size / file_count 直接写在条目上，
/// 不再为每个节点另挂 `dir_size` / `dir_files` 两根 u64 数组
/// （16M 条目时那两根就要 256MB）。名字 intern 进 `name_pool`，
/// 条目只存偏移；池里每条名字带 2 字节小端长度前缀。
///
/// mmap 模式下，磁盘上的 v7 主体通过 `Arc` 共享且**永不被写穿**：
/// 所有对主体节点的修改（墓碑、聚合值更新）都记进 `overrides` 这份
/// 显式 delta，追加节点进 `entries`/`name_pool`，子节点挂到
/// `extra_child`。因此 `Clone` 只需 `Arc::clone` + 复制几份小 delta——
/// 既便宜，又保证已修改状态不会被克隆丢弃。
pub struct SizeTree {
    volume: VolumeId,
    /// 堆模式：全量扫描结果。mmap 模式：增量追加的节点。
    entries: Vec<TreeEntry>,
    name_pool: Vec<u8>,
    child_start: Vec<u32>,
    child_at: Vec<u32>,
    /// 从磁盘 mmap 的不可变主体，多份克隆共享同一映射。
    mapped: Option<std::sync::Arc<MappedIndex>>,
    /// mmap 主体节点的显式修改（下标 → 覆盖值）。堆树恒为空。
    overrides: OverrideMap,
    extra_child: HashMap<u32, Vec<u32>>,
}

/// 只读 mmap 映射。写入从不落到这页内存上——见 [`SizeTree::overrides`]。
/// Unix 语义下 fd 关闭后映射仍然有效，因此不保留 File 句柄。
struct MappedIndex {
    ptr: *mut u8,
    len: usize,
    n: usize,
    name_off: usize,
    name_len: usize,
    ent_off: usize,
    cs_off: usize,
    ca_off: usize,
    ca_len: usize,
}

unsafe impl Send for MappedIndex {}
unsafe impl Sync for MappedIndex {}

impl Drop for MappedIndex {
    fn drop(&mut self) {
        if !self.ptr.is_null() && self.len > 0 {
            unsafe {
                libc::munmap(self.ptr as *mut libc::c_void, self.len);
            }
        }
        self.ptr = std::ptr::null_mut();
    }
}

impl MappedIndex {
    fn entries(&self) -> &[TreeEntry] {
        unsafe {
            std::slice::from_raw_parts(self.ptr.add(self.ent_off) as *const TreeEntry, self.n)
        }
    }

    /// v7 header 里存的全文件 checksum，delta 文件用它绑定 base 版本。
    fn stored_checksum(&self) -> u64 {
        let bytes = unsafe { std::slice::from_raw_parts(self.ptr, INDEX_V7_HEADER.min(self.len)) };
        u64::from_le_bytes(bytes[72..80].try_into().unwrap_or([0; 8]))
    }

    fn names(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr.add(self.name_off), self.name_len) }
    }

    fn child_slice(&self, idx: u32) -> &[u32] {
        let i = idx as usize;
        let start = self.u32s(self.cs_off, self.n + 1);
        if i + 1 >= start.len() {
            return &[];
        }
        let a = start[i] as usize;
        let b = start[i + 1] as usize;
        let at = self.u32s(self.ca_off, self.ca_len);
        if b > at.len() || a > b {
            return &[];
        }
        &at[a..b]
    }

    /// # Safety 前提
    ///
    /// `off` 的 4 字节对齐和 `off + count * 4 <= len` 都由 `map_index_fd`
    /// 的头部校验保证，映射基址本身是页对齐的。调用方只传 `cs_off` / `ca_off`。
    fn u32s(&self, off: usize, count: usize) -> &[u32] {
        unsafe { std::slice::from_raw_parts(self.ptr.add(off) as *const u32, count) }
    }
}

/// SizeTree 的内部条目。`#[repr(C)]` 保证 24 字节，可直接按字节落盘。
///
/// - `parent_bits`：父下标 + is_dir / used 标志
/// - `name_off`：指向 name_pool 中 `[u16 le len][bytes]` 的偏移
/// - `size`：文件为自身大小，目录为聚合大小
/// - `mtime`：文件 Unix 秒；目录为 0
/// - `file_count`：文件为 1，目录为聚合文件数
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct TreeEntry {
    parent_bits: u32,
    name_off: u32,
    pub size: u64,
    pub mtime: u32,
    pub file_count: u32,
}

impl TreeEntry {
    pub fn new(
        parent: u32,
        name_off: u32,
        is_dir: bool,
        size: u64,
        mtime: u64,
        file_count: u32,
    ) -> Self {
        debug_assert!(parent <= PARENT_MASK);
        let mut parent_bits = parent & PARENT_MASK;
        parent_bits |= USED_BIT;
        if is_dir {
            parent_bits |= DIR_BIT;
        }
        Self {
            parent_bits,
            name_off,
            size,
            mtime: mtime.min(u32::MAX as u64) as u32,
            file_count,
        }
    }

    #[inline]
    pub fn parent(&self) -> u32 {
        self.parent_bits & PARENT_MASK
    }

    #[inline]
    pub fn is_dir(&self) -> bool {
        self.parent_bits & DIR_BIT != 0
    }

    #[inline]
    pub fn used(&self) -> bool {
        self.parent_bits & USED_BIT != 0
    }

    #[inline]
    pub fn set_used(&mut self, used: bool) {
        if used {
            self.parent_bits |= USED_BIT;
        } else {
            self.parent_bits &= !USED_BIT;
        }
    }

    #[inline]
    pub fn set_parent(&mut self, parent: u32) {
        self.parent_bits = (self.parent_bits & !PARENT_MASK) | (parent & PARENT_MASK);
    }

    /// 从 24 字节小端布局解析（v7 文件 / 溢写流式构建共用）。
    pub(crate) fn from_bytes(b: &[u8]) -> Self {
        Self {
            parent_bits: u32::from_le_bytes(b[0..4].try_into().unwrap()),
            name_off: u32::from_le_bytes(b[4..8].try_into().unwrap()),
            size: u64::from_le_bytes(b[8..16].try_into().unwrap()),
            mtime: u32::from_le_bytes(b[16..20].try_into().unwrap()),
            file_count: u32::from_le_bytes(b[20..24].try_into().unwrap()),
        }
    }

    /// 写入 24 字节小端布局。
    pub(crate) fn write_bytes_to(&self, out: &mut [u8]) {
        out[0..4].copy_from_slice(&self.parent_bits.to_le_bytes());
        out[4..8].copy_from_slice(&self.name_off.to_le_bytes());
        out[8..16].copy_from_slice(&self.size.to_le_bytes());
        out[16..20].copy_from_slice(&self.mtime.to_le_bytes());
        out[20..24].copy_from_slice(&self.file_count.to_le_bytes());
    }

    /// 替换目录聚合值（流式构建的聚合传播用）。
    pub(crate) fn with_totals(mut self, size: u64, file_count: u32) -> Self {
        self.size = size;
        self.file_count = file_count;
        self
    }
}

const _: () = assert!(std::mem::size_of::<TreeEntry>() == 24);
const _: () = assert!(std::mem::align_of::<TreeEntry>() == 8);

fn map_index_file(path: &Path, verify: bool) -> Option<MappedIndex> {
    let file = std::fs::File::open(path).ok()?;
    map_index_fd(file, verify)
}

fn map_index_fd(file: std::fs::File, verify: bool) -> Option<MappedIndex> {
    use std::os::unix::io::AsRawFd;
    let len = file.metadata().ok()?.len() as usize;
    if len < INDEX_V7_HEADER {
        return None;
    }
    let fd = file.as_raw_fd();
    // 只读映射：增量修改走显式 delta，绝不写进映射页。
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            fd,
            0,
        )
    };
    if ptr == libc::MAP_FAILED {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len) };
    if &bytes[0..8] != INDEX_V7_MAGIC {
        unsafe { libc::munmap(ptr, len) };
        return None;
    }
    let version = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
    if version != 7 {
        unsafe { libc::munmap(ptr, len) };
        return None;
    }
    let n = u32::from_le_bytes(bytes[12..16].try_into().ok()?) as usize;
    let name_len = u32::from_le_bytes(bytes[16..20].try_into().ok()?) as usize;
    let ca_len = u32::from_le_bytes(bytes[20..24].try_into().ok()?) as usize;
    let name_off = u32::from_le_bytes(bytes[80..84].try_into().ok()?) as usize;
    let ent_off = u32::from_le_bytes(bytes[84..88].try_into().ok()?) as usize;
    let cs_off = u32::from_le_bytes(bytes[88..92].try_into().ok()?) as usize;
    let ca_off = u32::from_le_bytes(bytes[92..96].try_into().ok()?) as usize;
    if n == 0
        || name_off.saturating_add(name_len) > len
        || ent_off.saturating_add(n * std::mem::size_of::<TreeEntry>()) > len
        || cs_off.saturating_add((n + 1) * 4) > len
        || ca_off.saturating_add(ca_len * 4) > len
        || !ent_off.is_multiple_of(std::mem::align_of::<TreeEntry>())
        // CSR 的两段都按 `&[u32]` 取用（见 `MappedIndex::u32s`），偏移必须
        // 4 字节对齐。写入端产出的偏移天然对齐，但这个函数的职责就是校验
        // 不可信的磁盘数据——被截断或被改过的索引能构造出未对齐切片，那是
        // UB，不是「读到脏数据」。长度校验挡不住它，得单独校验对齐。
        || !cs_off.is_multiple_of(4)
        || !ca_off.is_multiple_of(4)
    {
        unsafe { libc::munmap(ptr, len) };
        return None;
    }
    let stored = u64::from_le_bytes(bytes[72..80].try_into().ok()?);
    if verify {
        if stored == 0 {
            unsafe { libc::munmap(ptr, len) };
            return None;
        }
        let sum = index_checksum_bytes(bytes);
        if sum != stored {
            unsafe { libc::munmap(ptr, len) };
            return None;
        }
    }
    let mapped = MappedIndex {
        ptr: ptr as *mut u8,
        len,
        n,
        name_off,
        name_len,
        ent_off,
        cs_off,
        ca_off,
        ca_len,
    };
    if verify && !validate_mapped(&mapped) {
        return None;
    }
    if verify {
        release_mapped_pages(&mapped);
    }
    Some(mapped)
}

/// 结构校验：根节点、父下标无环、父节点是目录、名字偏移、CSR 与 header 统计。
fn validate_mapped(m: &MappedIndex) -> bool {
    let ents = m.entries();
    let names = m.names();
    if ents.is_empty() {
        return false;
    }
    let root = ents[0];
    if !root.used() || !root.is_dir() || root.parent() != 0 {
        return false;
    }
    let mut files = 0u64;
    let mut dirs = 0u64;
    for (i, e) in ents.iter().enumerate() {
        if !e.used() {
            return false;
        }
        if e.is_dir() {
            dirs += 1;
        } else {
            files += 1;
        }
        if i > 0 {
            let p = e.parent() as usize;
            if p >= i || p >= ents.len() || !ents[p].used() || !ents[p].is_dir() {
                return false;
            }
        }
        let off = e.name_off as usize;
        if off.saturating_add(2) > names.len() {
            return false;
        }
        let len = u16::from_le_bytes([names[off], names[off + 1]]) as usize;
        if off.saturating_add(2).saturating_add(len) > names.len() {
            return false;
        }
        if std::str::from_utf8(&names[off + 2..off + 2 + len]).is_err() {
            return false;
        }
    }
    let child_start = m.u32s(m.cs_off, m.n + 1);
    if child_start.first().copied() != Some(0)
        || child_start.last().copied() != Some(m.ca_len as u32)
        || child_start.windows(2).any(|pair| pair[0] > pair[1])
    {
        return false;
    }
    let mut seen = vec![false; m.n];
    seen[0] = true;
    for i in 0..m.n {
        for &c in m.child_slice(i as u32) {
            let ci = c as usize;
            if ci == 0
                || ci >= m.n
                || ci == i
                || ents[ci].parent() != i as u32
                || std::mem::replace(&mut seen[ci], true)
            {
                return false;
            }
        }
    }
    if seen.iter().any(|present| !present) {
        return false;
    }
    let header = unsafe { std::slice::from_raw_parts(m.ptr, INDEX_V7_HEADER.min(m.len)) };
    if header.len() < 56 {
        return false;
    }
    let hdr_files = u64::from_le_bytes(header[32..40].try_into().unwrap_or([0; 8]));
    let hdr_dirs = u64::from_le_bytes(header[40..48].try_into().unwrap_or([0; 8]));
    hdr_files == files && hdr_dirs == dirs
}

fn release_mapped_pages(m: &MappedIndex) {
    unsafe {
        libc::posix_madvise(m.ptr as *mut libc::c_void, m.len, libc::POSIX_MADV_DONTNEED);
        libc::madvise(m.ptr as *mut libc::c_void, m.len, libc::MADV_FREE);
    }
}

/// overrides 的快速哈希。
///
/// `slot()` 是搜索 / top-N / 计数等全树遍历的热路径，每个节点都要查一次
/// 覆盖表；默认 SipHash 的常数在千万级条目上是数百毫秒的主线程开销，
/// 单 u32 键用乘法散列（斐波那契）足够。
#[derive(Default)]
struct FastU32Hasher(u64);

impl std::hash::Hasher for FastU32Hasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3);
        }
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.0 = u64::from(i).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

type OverrideMap = HashMap<u32, TreeEntry, std::hash::BuildHasherDefault<FastU32Hasher>>;

fn build_csr(entries: &[TreeEntry]) -> (Vec<u32>, Vec<u32>) {
    let n = entries.len();
    let mut child_counts = vec![0u32; n];
    for entry in entries.iter().skip(1) {
        if !entry.used() {
            continue;
        }
        let parent = entry.parent() as usize;
        if parent < n && entries[parent].used() {
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
        if !entry.used() {
            continue;
        }
        let parent = entry.parent() as usize;
        if parent < n && entries[parent].used() {
            child_at[cursor[parent] as usize] = idx as u32;
            cursor[parent] += 1;
        }
    }
    (child_start, child_at)
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

impl Clone for SizeTree {
    /// 克隆 = `Arc::clone` 不可变 mmap 主体 + 复制显式 delta。
    ///
    /// 之前实现对同一 inode 重新 mmap，只能看到原文件内容，原映射上的
    /// MAP_PRIVATE COW 修改（墓碑、目录聚合更新、祖先链变化）全部丢失，
    /// 已删除节点会被"复活"。现在修改都在 delta 结构里，克隆天然保留。
    fn clone(&self) -> Self {
        Self {
            volume: self.volume.clone(),
            entries: self.entries.clone(),
            name_pool: self.name_pool.clone(),
            child_start: self.child_start.clone(),
            child_at: self.child_at.clone(),
            mapped: self.mapped.clone(),
            overrides: self.overrides.clone(),
            extra_child: self.extra_child.clone(),
        }
    }
}

impl std::fmt::Debug for SizeTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SizeTree({}: {} 条)", self.volume, self.n())
    }
}

impl SizeTree {
    /// 构造一棵空树（只有根节点占位）。
    pub fn empty(volume: VolumeId) -> Self {
        let label = volume.display().to_string();
        let mut name_pool = Vec::new();
        let name_off = push_name(&mut name_pool, label.as_bytes());
        Self {
            volume,
            entries: vec![TreeEntry::new(0, name_off, true, 0, 0, 0)],
            name_pool,
            child_start: vec![0, 0],
            child_at: vec![],
            mapped: None,
            overrides: OverrideMap::default(),
            extra_child: HashMap::new(),
        }
    }

    /// 从原始部件构造一棵树。仅供 `platform::macos::walk` 调用。
    ///
    /// `entries` 必须已经写好目录聚合值和 intern 过的 `name_off`。
    pub fn from_parts(
        volume: VolumeId,
        mut entries: Vec<TreeEntry>,
        mut name_pool: Vec<u8>,
        child_start: Vec<u32>,
        child_at: Vec<u32>,
    ) -> Self {
        entries.shrink_to_fit();
        name_pool.shrink_to_fit();
        Self {
            volume,
            entries,
            name_pool,
            child_start,
            child_at,
            mapped: None,
            overrides: OverrideMap::default(),
            extra_child: HashMap::new(),
        }
    }

    fn n(&self) -> usize {
        match &self.mapped {
            Some(m) => m.n + self.entries.len(),
            None => self.entries.len(),
        }
    }

    fn slot(&self, i: usize) -> TreeEntry {
        if let Some(m) = &self.mapped {
            if i < m.n {
                // 空表快速跳过：全树遍历里这一分支每节点都走，
                // 不能对空 HashMap 做无谓的哈希探测
                if !self.overrides.is_empty() {
                    if let Some(e) = self.overrides.get(&(i as u32)) {
                        return *e;
                    }
                }
                return m.entries()[i];
            }
            return self.entries[i - m.n];
        }
        self.entries[i]
    }

    /// 读改写一个槽位。
    ///
    /// mmap 主体是只读共享映射，修改落入 `overrides` 这份显式 delta；
    /// 堆树直接原地改。所有原先通过 `slot_mut` 的写入都必须走这里，
    /// 否则克隆会丢状态。
    fn update_slot(&mut self, i: usize, f: impl FnOnce(&mut TreeEntry)) {
        if let Some(m) = &self.mapped {
            if i < m.n {
                let mut e = if self.overrides.is_empty() {
                    m.entries()[i]
                } else {
                    self.overrides
                        .get(&(i as u32))
                        .copied()
                        .unwrap_or_else(|| m.entries()[i])
                };
                f(&mut e);
                self.overrides.insert(i as u32, e);
                return;
            }
            f(&mut self.entries[i - m.n]);
            return;
        }
        f(&mut self.entries[i]);
    }

    /// 运行时占用的堆字节数（entries + name_pool + CSR + delta），不含 Vec 容量余量。
    /// mmap 主体按映射长度计，OS 可在空闲时丢掉干净页。
    pub fn memory_bytes(&self) -> usize {
        let mapped = self.mapped.as_ref().map(|m| m.len).unwrap_or(0);
        let overlay_children = self
            .extra_child
            .values()
            .map(|children| children.capacity() * std::mem::size_of::<u32>())
            .sum::<usize>();
        let extra = self.entries.len() * std::mem::size_of::<TreeEntry>()
            + self.name_pool.len()
            + self.child_start.len() * 4
            + self.child_at.len() * 4
            + self.overrides.len()
                * (std::mem::size_of::<TreeEntry>() + std::mem::size_of::<u32>() * 2)
            + overlay_children;
        mapped + extra
    }

    /// 是否挂在 mmap 主体上。
    pub fn has_mapped_base(&self) -> bool {
        self.mapped.is_some()
    }

    /// 显式 delta 的规模（追加节点 + 覆盖节点 + overlay 子项数）。
    /// 持久化层用它决定是写小 delta 文件还是触发全量压实。
    pub fn delta_len(&self) -> usize {
        self.entries.len()
            + self.overrides.len()
            + self.extra_child.values().map(Vec::len).sum::<usize>()
    }

    /// delta 中追加的节点数。
    pub fn entries_delta_len(&self) -> usize {
        self.entries.len()
    }

    /// delta 中覆盖的主体节点数（墓碑 + 聚合更新）。
    pub fn overrides_delta_len(&self) -> usize {
        self.overrides.len()
    }

    /// 仅供性能基准：清空显式 delta（不影响 mmap 主体）。
    pub fn clear_delta_for_bench(&mut self) {
        self.entries.clear();
        self.name_pool.clear();
        self.overrides.clear();
        self.extra_child.clear();
    }

    /// 仅供性能基准：把主体节点标记为覆盖，制造非空 overrides。
    pub fn bench_mark_override(&mut self, idx: u32) {
        if let Some(m) = &self.mapped {
            if (idx as usize) < m.n {
                let e = m.entries()[idx as usize];
                self.overrides.insert(idx, e);
            }
        }
    }

    pub fn entry_count(&self) -> usize {
        self.n()
    }

    /// 把名字追加到池里（不 intern，给增量更新用）。
    fn pool_push(&mut self, name: &str) -> u32 {
        push_name(&mut self.name_pool, name.as_bytes())
    }

    /// 取节点名字的 `&str` 引用（零拷贝，直接从 name_pool 切）。
    fn entry_name_str(&self, idx: u32) -> &str {
        let i = idx as usize;
        if let Some(m) = &self.mapped {
            if i < m.n {
                return pool_str(m.names(), m.entries()[i].name_off);
            }
            return pool_str(&self.name_pool, self.entries[i - m.n].name_off);
        }
        pool_str(&self.name_pool, self.entries[i].name_off)
    }

    /// 转成紧凑持久化格式，避免为每个节点复制完整路径。
    ///
    /// 测试和局部子树构造还走这条路径；整盘索引落盘走 v7 writer
    /// （`index_v7`），不再为 1600 万节点各分配一个 `String`。
    #[allow(clippy::needless_range_loop)]
    pub fn compact_entries(&self) -> Vec<TreeIndexEntry> {
        // 增量替换会把旧节点标成 unused 并在末尾追加新节点。若把这些
        // 墓碑也持久化，索引会在每轮保存后持续膨胀；日志中有效节点约
        // 1721 万，但缓存已经增长到 3161 万条。保存时过滤墓碑并重映射
        // parent，下次加载恢复成真正紧凑的树。
        let n = self.n();
        let mut remap = vec![u32::MAX; n];
        let mut next = 0u32;
        for index in 0..n {
            if self.slot(index).used() {
                remap[index] = next;
                next += 1;
            }
        }

        (0..n)
            .filter(|&index| self.slot(index).used())
            .map(|index| {
                let entry = self.slot(index);
                TreeIndexEntry {
                    parent: if index == ROOT_NODE as usize {
                        ROOT_NODE
                    } else {
                        remap[entry.parent() as usize]
                    },
                    name: self.entry_name_str(index as u32).to_string(),
                    is_dir: entry.is_dir(),
                    size: if entry.is_dir() { 0 } else { entry.size },
                    used: true,
                    mtime: entry.mtime as u64,
                }
            })
            .collect()
    }

    /// 从 POD 条目 + 名字池重建运行时目录树（CSR 从 parent 重建）。
    pub fn from_packed(volume: VolumeId, name_pool: Vec<u8>, entries: Vec<TreeEntry>) -> Self {
        Self::build_from_entries_with_pool(volume, entries, name_pool)
    }

    /// 从 v7 mmap 文件构造树。不把 24B 条目复制进 Vec。
    pub fn from_mapped(volume: VolumeId, path: &Path) -> Option<Self> {
        let mapped = map_index_file(path, true)?;
        if mapped.n == 0 {
            return None;
        }
        let root = mapped.entries()[0];
        if !root.used() || !root.is_dir() {
            return None;
        }
        Some(Self {
            volume,
            entries: Vec::new(),
            name_pool: Vec::new(),
            child_start: Vec::new(),
            child_at: Vec::new(),
            mapped: Some(std::sync::Arc::new(mapped)),
            overrides: OverrideMap::default(),
            extra_child: HashMap::new(),
        })
    }

    fn can_write_inplace(&self) -> bool {
        self.mapped.is_none()
            && self.extra_child.is_empty()
            && self.child_start.len() == self.entries.len() + 1
            && self.entries.iter().all(|e| e.used())
    }

    /// 写出 v7 未压缩 mmap 文件：header + entries + CSR + names。
    ///
    /// 全量扫描结果（无墓碑、无 overlay）直接写现有数组。其余情况走
    /// [`Self::write_v7_streaming`] 流式压实——不再物化完整的新
    /// entries/pool/CSR 堆数组。
    pub fn write_v7(&self, path: &Path, meta: IndexMeta) -> std::io::Result<()> {
        if self.can_write_inplace() {
            return write_v7_file(
                path,
                &meta,
                &self.name_pool,
                &self.entries,
                &self.child_start,
                &self.child_at,
            );
        }
        self.write_v7_streaming(path, &meta)
    }

    /// 流式压实写出。
    ///
    /// 之前的实现一次性物化 remap、新 TreeEntry
    /// 数组、名字 interner、新名字池和 CSR（16M 条目合计 600MB+），
    /// 保存期间 physical footprint 冲到 1.3GiB，释放后 allocator 仍保留
    /// 800MB+，稳态卡在 964MiB。现在除了 remap 和子计数两块 O(n) u32
    /// 暂存（16M 条目约 124MB，写完即释放），条目、CSR、名字池全部
    /// 直接写进输出文件的 MAP_SHARED 映射，并周期性 msync 控制脏页上限。
    ///
    /// 文件布局：header | mount/label | entries | CSR | names。
    /// 名字不重新 intern（省掉百 MB 级 HashMap），池按实际上限预留、
    /// 写完截断，因此放在文件末尾。
    fn write_v7_streaming(&self, path: &Path, meta: &IndexMeta) -> std::io::Result<()> {
        let n = self.n();

        // Pass A：有效节点数、旧→新下标映射、每个新节点的子节点数。
        // parent 恒小于自身下标，所以处理到 i 时 remap[parent] 已就绪，
        // 不需要第二遍。
        let mut remap = vec![u32::MAX; n];
        let mut start = vec![0u32; n]; // 先当子计数，再原地转成前缀和
        let mut used = 0usize;
        for i in 0..n {
            let e = self.slot(i);
            if !e.used() {
                continue;
            }
            remap[i] = used as u32;
            used += 1;
            if i > 0 {
                let p = remap[e.parent() as usize];
                if p != u32::MAX {
                    start[p as usize] += 1;
                }
            }
        }
        let mut ca_len = 0usize;
        for s in start[..used].iter_mut() {
            let c = *s;
            *s = ca_len as u32;
            ca_len += c as usize;
        }

        let mount = meta.mount.as_bytes();
        let label = meta.label.as_bytes();
        // 池上限 = base 池 + delta 池；名字仍要 intern，否则重名会让实际
        // 写入超过这个上限。
        let pool_upper = self
            .mapped
            .as_ref()
            .map(|m| m.name_len)
            .unwrap_or(0)
            .saturating_add(self.name_pool.len());
        let layout =
            V7Layout::names_trailing(used, mount.len(), label.len(), ca_len, pool_upper);

        let tmp = path.with_extension("bin.tmp");
        let mut out = MmapOut::create(tmp, layout.len)?;
        let ptr = out.ptr();
        let buf = out.as_mut_slice();

        // child_start 先填前缀和，Pass B 期间把它当游标递增，结束后重写。
        layout.write_child_start(buf, &start[..used]);

        // Pass B：条目 + 名字 + child_at，一次遍历完成。
        let mut pool = MmapPool::new(layout.name_off, used);
        let mut j = 0usize;
        for i in 0..n {
            let e = self.slot(i);
            if !e.used() {
                continue;
            }
            let name_off = pool.intern(buf, self.entry_name_str(i as u32).as_bytes());
            let parent = if i == 0 {
                0
            } else {
                remap[e.parent() as usize]
            };
            TreeEntry::new(
                parent,
                name_off,
                e.is_dir(),
                e.size,
                e.mtime as u64,
                e.file_count,
            )
            .write_bytes_to(&mut buf[layout.entry_at(j)]);

            if j > 0 {
                layout.push_child(buf, parent, j as u32);
            }
            j += 1;

            pool.maybe_flush(ptr);
        }

        // 重写 child_start（Pass B 里被当成游标改掉了）
        layout.write_child_start(buf, &start[..used]);
        drop(start);
        drop(remap);

        V7Header {
            layout: &layout,
            mount,
            label,
            name_len: pool.name_len(),
            file_count: meta.file_count,
            dir_count: meta.dir_count,
            total_size: meta.total_size,
            last_event_id: meta.last_event_id,
            scanned_at: meta.scanned_at,
        }
        .write_into(buf);
        finalize_checksum(buf, pool.cursor());

        out.commit(pool.cursor())?;
        out.rename_to(path)
    }

    pub fn mapped_header_stats(&self) -> Option<(u64, u64, u64, u64)> {
        let m = self.mapped.as_ref()?;
        let b = unsafe { std::slice::from_raw_parts(m.ptr, INDEX_V7_HEADER.min(m.len)) };
        if b.len() < 64 {
            return None;
        }
        Some((
            u64::from_le_bytes(b[32..40].try_into().ok()?),
            u64::from_le_bytes(b[40..48].try_into().ok()?),
            u64::from_le_bytes(b[48..56].try_into().ok()?),
            u64::from_le_bytes(b[56..64].try_into().ok()?),
        ))
    }

    /// mmap 主体文件里存的全文件 checksum，delta 用它绑定 base 版本。
    pub fn base_checksum(&self) -> Option<u64> {
        self.mapped.as_ref().map(|m| m.stored_checksum())
    }

    /// 把显式 delta（追加节点、名字、覆盖节点、overlay 子表）序列化到
    /// `path`，tmp+rename 原子替换。增量保存只写这个小文件，不再每次
    /// 压实重写整个 587MB 的 base。
    pub fn write_delta(&self, path: &Path, meta: &DeltaMeta) -> std::io::Result<()> {
        let mut payload: Vec<u8> = Vec::new();
        payload.extend_from_slice(entries_as_bytes(&self.entries));
        payload.extend_from_slice(&self.name_pool);
        let mut overrides: Vec<(&u32, &TreeEntry)> = self.overrides.iter().collect();
        overrides.sort_by_key(|(idx, _)| **idx);
        for (&idx, e) in overrides {
            payload.extend_from_slice(&idx.to_le_bytes());
            payload.extend_from_slice(entries_as_bytes(std::slice::from_ref(e)));
        }
        // extra_child：parent 有序，保证字节稳定
        let mut extras: Vec<(&u32, &Vec<u32>)> = self.extra_child.iter().collect();
        extras.sort_by_key(|(p, _)| **p);
        for (&parent, kids) in extras {
            payload.extend_from_slice(&parent.to_le_bytes());
            payload.extend_from_slice(&(kids.len() as u32).to_le_bytes());
            for &k in kids {
                payload.extend_from_slice(&k.to_le_bytes());
            }
        }

        let tmp = path.with_extension("delta.tmp");
        let mut out = vec![0u8; DELTA_HEADER];
        out[0..8].copy_from_slice(DELTA_MAGIC);
        out[8..12].copy_from_slice(&1u32.to_le_bytes());
        out[12..20].copy_from_slice(&meta.base_checksum.to_le_bytes());
        out[20..28].copy_from_slice(&meta.last_event_id.to_le_bytes());
        out[28..36].copy_from_slice(&meta.scanned_at.to_le_bytes());
        out[36..44].copy_from_slice(&meta.file_count.to_le_bytes());
        out[44..52].copy_from_slice(&meta.dir_count.to_le_bytes());
        out[52..60].copy_from_slice(&meta.total_size.to_le_bytes());
        out[60..68].copy_from_slice(&(self.entries.len() as u64).to_le_bytes());
        out[68..76].copy_from_slice(&(self.name_pool.len() as u64).to_le_bytes());
        out[76..84].copy_from_slice(&(self.overrides.len() as u64).to_le_bytes());
        out[84..92].copy_from_slice(&(self.extra_child.len() as u64).to_le_bytes());
        out[92..100].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        let checksum = delta_checksum(&out, &payload);
        out[100..108].copy_from_slice(&checksum.to_le_bytes());
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&out)?;
        f.write_all(&payload)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    }

    /// 从 v7 base + delta 文件构造树。
    ///
    /// delta 缺失、损坏或与 base 版本不匹配（base 已被压实重写）时，
    /// 返回纯 base 树和 `None`——调用方据此回退到 header 统计。
    pub fn from_mapped_with_delta(
        volume: VolumeId,
        base_path: &Path,
        delta_path: &Path,
    ) -> Option<(Self, Option<DeltaMeta>)> {
        let mapped = map_index_file(base_path, true)?;
        if mapped.n == 0 {
            return None;
        }
        let root = mapped.entries()[0];
        if !root.used() || !root.is_dir() {
            return None;
        }
        let base_checksum = mapped.stored_checksum();
        let mut tree = Self {
            volume,
            entries: Vec::new(),
            name_pool: Vec::new(),
            child_start: Vec::new(),
            child_at: Vec::new(),
            mapped: Some(std::sync::Arc::new(mapped)),
            overrides: OverrideMap::default(),
            extra_child: HashMap::new(),
        };
        let meta = parse_delta_file(delta_path, base_checksum).and_then(|(meta, payload)| {
            if tree.apply_delta_payload(&payload, &meta).is_ok() {
                Some(meta)
            } else {
                tree.entries.clear();
                tree.name_pool.clear();
                tree.overrides.clear();
                tree.extra_child.clear();
                None
            }
        });
        Some((tree, meta))
    }

    /// 把 delta payload 应用到当前树（仅当挂在 mmap 主体上时合法）。
    fn apply_delta_payload(
        &mut self,
        payload: &[u8],
        meta: &DeltaMeta,
    ) -> Result<(), std::io::Error> {
        if self.mapped.is_none() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "delta 只能应用到 mmap 主体",
            ));
        }
        let mut cur = 0usize;
        let mut take = |len: usize| -> Result<&[u8], std::io::Error> {
            let end = cur.checked_add(len).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "delta 长度溢出")
            })?;
            if end > payload.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "delta payload 截断",
                ));
            }
            let s = &payload[cur..end];
            cur = end;
            Ok(s)
        };

        let n_entries = usize::try_from(meta.n_entries).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "delta 节点数溢出")
        })?;
        let base_n = self.mapped.as_ref().map(|mapped| mapped.n).unwrap_or(0);
        if base_n
            .checked_add(n_entries)
            .is_none_or(|total| total > PARENT_MASK as usize + 1)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "delta 节点总数超出父下标范围",
            ));
        }
        let entries_bytes = n_entries.checked_mul(24).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "delta 节点区长度溢出")
        })?;
        let raw = take(entries_bytes)?;
        self.entries.reserve(n_entries);
        for k in 0..n_entries {
            let b = &raw[k * 24..k * 24 + 24];
            self.entries.push(TreeEntry {
                parent_bits: u32::from_le_bytes(b[0..4].try_into().unwrap()),
                name_off: u32::from_le_bytes(b[4..8].try_into().unwrap()),
                size: u64::from_le_bytes(b[8..16].try_into().unwrap()),
                mtime: u32::from_le_bytes(b[16..20].try_into().unwrap()),
                file_count: u32::from_le_bytes(b[20..24].try_into().unwrap()),
            });
        }

        let pool_len = usize::try_from(meta.pool_len).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "delta 名字池长度溢出")
        })?;
        self.name_pool = take(pool_len)?.to_vec();
        for entry in &self.entries {
            let off = entry.name_off as usize;
            if off + 2 > self.name_pool.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "delta 追加节点名字偏移越界",
                ));
            }
            let len = u16::from_le_bytes([self.name_pool[off], self.name_pool[off + 1]]) as usize;
            let end = off
                .checked_add(2)
                .and_then(|start| start.checked_add(len))
                .filter(|&end| end <= self.name_pool.len())
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "delta 追加节点名字越界")
                })?;
            if std::str::from_utf8(&self.name_pool[off + 2..end]).is_err() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "delta 追加节点名字不是 UTF-8",
                ));
            }
        }

        for _ in 0..meta.n_overrides {
            let ib = take(4)?;
            let idx = u32::from_le_bytes(ib.try_into().unwrap()) as usize;
            let b = take(24)?;
            if idx >= self.mapped.as_ref().map(|m| m.n).unwrap_or(0) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "delta override 下标越界",
                ));
            }
            let entry = TreeEntry {
                parent_bits: u32::from_le_bytes(b[0..4].try_into().unwrap()),
                name_off: u32::from_le_bytes(b[4..8].try_into().unwrap()),
                size: u64::from_le_bytes(b[8..16].try_into().unwrap()),
                mtime: u32::from_le_bytes(b[16..20].try_into().unwrap()),
                file_count: u32::from_le_bytes(b[20..24].try_into().unwrap()),
            };
            let original = self.mapped.as_ref().unwrap().entries()[idx];
            if entry.parent() != original.parent() || entry.name_off != original.name_off {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "delta override 不得修改父节点或名字",
                ));
            }
            let previous = self.overrides.insert(idx as u32, entry);
            if previous.is_some() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "delta override 下标重复",
                ));
            }
        }

        for _ in 0..meta.n_extra_parents {
            let pb = take(4)?;
            let parent = u32::from_le_bytes(pb.try_into().unwrap());
            let cb = take(4)?;
            let count = u32::from_le_bytes(cb.try_into().unwrap()) as usize;
            if parent as usize >= self.n()
                || count > self.n()
                || !self.slot(parent as usize).used()
                || !self.slot(parent as usize).is_dir()
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "delta overlay 父节点或子节点数异常",
                ));
            }
            let child_bytes = count.checked_mul(4).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "delta overlay 长度溢出")
            })?;
            let kb = take(child_bytes)?;
            let kids: Vec<u32> = kb
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_le_bytes(*c))
                .collect();
            let mut unique = std::collections::HashSet::with_capacity(kids.len());
            if kids.iter().any(|&kid| {
                kid as usize >= self.n()
                    || self.slot(kid as usize).parent() != parent
                    || !unique.insert(kid)
            }) || self
                .mapped
                .as_ref()
                .filter(|mapped| (parent as usize) < mapped.n)
                .is_some_and(|mapped| {
                    mapped
                        .child_slice(parent)
                        .iter()
                        .any(|&kid| self.slot(kid as usize).used() && !unique.contains(&kid))
                })
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "delta overlay 子节点越界、重复或父节点不一致",
                ));
            }
            if self.extra_child.insert(parent, kids).is_some() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "delta overlay 父节点重复",
                ));
            }
        }
        if cur != payload.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "delta payload 有多余字节",
            ));
        }
        let root = self.slot(0);
        if !root.used() || !root.is_dir() || root.parent() != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "delta 破坏了根节点",
            ));
        }
        for index in 1..self.n() {
            let entry = self.slot(index);
            if !entry.used() {
                continue;
            }
            let parent = entry.parent() as usize;
            if parent >= index {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "delta 节点父下标无效",
                ));
            }
            let parent_entry = self.slot(parent);
            if !parent_entry.used() || !parent_entry.is_dir() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "delta 节点父节点无效",
                ));
            }
            if index >= base_n
                && !self
                    .extra_child
                    .get(&(parent as u32))
                    .is_some_and(|children| children.contains(&(index as u32)))
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "delta 追加节点未挂入父节点 overlay",
                ));
            }
        }
        Ok(())
    }
}

/// 落盘头里的卷统计。
pub struct IndexMeta {
    pub mount: String,
    pub label: String,
    pub file_count: u64,
    pub dir_count: u64,
    pub total_size: u64,
    pub last_event_id: u64,
    pub scanned_at: u64,
}

pub const DELTA_MAGIC: &[u8; 8] = b"QCDLTV01";
const DELTA_HEADER: usize = 128;

/// delta 文件头信息。加载时同时用于恢复 ScanResult 统计——
/// base 的 v7 header 在多轮 delta 之后已经过期。
#[derive(Clone, Copy, Debug)]
pub struct DeltaMeta {
    /// 绑定的 base 版本（v7 全文件 checksum）。base 被压实重写后
    /// 旧 delta 一律作废。
    pub base_checksum: u64,
    pub last_event_id: u64,
    pub scanned_at: u64,
    pub file_count: u64,
    pub dir_count: u64,
    pub total_size: u64,
    // ---- 以下字段由 write_delta 填写，描述 payload 布局 ----
    pub n_entries: u64,
    pub pool_len: u64,
    pub n_overrides: u64,
    pub n_extra_parents: u64,
}

/// 读取并校验 delta 文件。magic、版本、payload checksum、与 base 的
/// 绑定关系任一不符都返回 None（调用方回退纯 base）。
fn parse_delta_file(path: &Path, base_checksum: u64) -> Option<(DeltaMeta, Vec<u8>)> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let file_len = usize::try_from(file.metadata().ok()?.len()).ok()?;
    let mut header = [0u8; DELTA_HEADER];
    file.read_exact(&mut header).ok()?;
    if header[0..8] != *DELTA_MAGIC || u32::from_le_bytes(header[8..12].try_into().ok()?) != 1 {
        return None;
    }
    let meta = DeltaMeta {
        base_checksum: u64::from_le_bytes(header[12..20].try_into().ok()?),
        last_event_id: u64::from_le_bytes(header[20..28].try_into().ok()?),
        scanned_at: u64::from_le_bytes(header[28..36].try_into().ok()?),
        file_count: u64::from_le_bytes(header[36..44].try_into().ok()?),
        dir_count: u64::from_le_bytes(header[44..52].try_into().ok()?),
        total_size: u64::from_le_bytes(header[52..60].try_into().ok()?),
        n_entries: u64::from_le_bytes(header[60..68].try_into().ok()?),
        pool_len: u64::from_le_bytes(header[68..76].try_into().ok()?),
        n_overrides: u64::from_le_bytes(header[76..84].try_into().ok()?),
        n_extra_parents: u64::from_le_bytes(header[84..92].try_into().ok()?),
    };
    if meta.base_checksum != base_checksum {
        return None;
    }
    let payload_len = usize::try_from(u64::from_le_bytes(header[92..100].try_into().ok()?)).ok()?;
    let stored_sum = u64::from_le_bytes(header[100..108].try_into().ok()?);
    if file_len != DELTA_HEADER.checked_add(payload_len)? {
        return None;
    }
    let mut payload = vec![0u8; payload_len];
    file.read_exact(&mut payload).ok()?;
    if delta_checksum(&header, &payload) != stored_sum {
        return None;
    }
    Some((meta, payload))
}

fn write_v7_file(
    path: &Path,
    meta: &IndexMeta,
    name_pool: &[u8],
    entries: &[TreeEntry],
    child_start: &[u32],
    child_at: &[u32],
) -> std::io::Result<()> {
    let n = entries.len();
    if child_start.len() != n + 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "CSR child_start 长度与节点数不一致",
        ));
    }
    let mount = meta.mount.as_bytes();
    let label = meta.label.as_bytes();
    let layout = V7Layout::names_inline(n, mount.len(), label.len(), name_pool.len(), child_at.len());

    let tmp = path.with_extension("bin.tmp");
    let mut out = MmapOut::create(tmp, layout.len)?;
    let buf = out.as_mut_slice();

    V7Header {
        layout: &layout,
        mount,
        label,
        name_len: name_pool.len(),
        file_count: meta.file_count,
        dir_count: meta.dir_count,
        total_size: meta.total_size,
        last_event_id: meta.last_event_id,
        scanned_at: meta.scanned_at,
    }
    .write_into(buf);

    buf[layout.name_off..layout.name_off + name_pool.len()].copy_from_slice(name_pool);
    let eb = entries_as_bytes(entries);
    buf[layout.ent_off..layout.ent_off + eb.len()].copy_from_slice(eb);
    // child_start 传进来时已是最终前缀和，末位哨兵由 layout 补
    layout.write_child_start(buf, &child_start[..n]);
    for (i, v) in child_at.iter().enumerate() {
        let o = layout.ca_off + i * 4;
        buf[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    finalize_checksum(buf, layout.len);

    out.commit(layout.len)?;
    out.rename_to(path)
}

impl SizeTree {
    /// 从紧凑持久化节点重建运行时目录树。
    pub fn from_compact(volume: VolumeId, compact: Vec<TreeIndexEntry>) -> Self {
        let n = compact.len();
        let mut intern = NameInterner::with_capacity(n);
        let mut entries = Vec::with_capacity(n);
        for entry in compact {
            let name_off = intern.intern(entry.name.as_bytes());
            entries.push(TreeEntry::new(
                entry.parent,
                name_off,
                entry.is_dir,
                if entry.is_dir { 0 } else { entry.size },
                entry.mtime,
                if entry.is_dir { 0 } else { 1 },
            ));
        }
        Self::build_from_entries_with_pool(volume, entries, intern.finish())
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
        let mut intern = NameInterner::with_capacity(n);
        let mut entries = Vec::with_capacity(n);
        for (parent, name, is_dir, size, mtime) in raw {
            let name_off = intern.intern(name.as_bytes());
            entries.push(TreeEntry::new(
                parent,
                name_off,
                is_dir,
                if is_dir { 0 } else { size },
                mtime,
                if is_dir { 0 } else { 1 },
            ));
        }
        Self::build_from_entries_with_pool(volume, entries, intern.finish())
    }

    /// 从已填好 name_pool 的 entries 构建 CSR 索引，并重算目录聚合。
    fn build_from_entries_with_pool(
        volume: VolumeId,
        mut entries: Vec<TreeEntry>,
        name_pool: Vec<u8>,
    ) -> Self {
        let n = entries.len();
        for entry in entries.iter_mut() {
            if entry.used() && entry.is_dir() {
                entry.size = 0;
                entry.file_count = 0;
            }
        }
        for i in 0..n {
            if !entries[i].used() || entries[i].is_dir() {
                continue;
            }
            let add_size = entries[i].size;
            let add_files = 1u64;
            let mut current = entries[i].parent();
            loop {
                let parent = current as usize;
                if parent >= n || !entries[parent].used() {
                    break;
                }
                entries[parent].size = entries[parent].size.saturating_add(add_size);
                entries[parent].file_count =
                    entries[parent].file_count.saturating_add(add_files as u32);
                if current == ROOT_NODE || entries[parent].parent() == current {
                    break;
                }
                current = entries[parent].parent();
            }
        }

        let (child_start, child_at) = build_csr(&entries);
        Self::from_parts(volume, entries, name_pool, child_start, child_at)
    }

    pub fn volume(&self) -> &VolumeId {
        &self.volume
    }

    pub fn root(&self) -> u32 {
        ROOT_NODE
    }

    pub fn valid(&self, idx: u32) -> bool {
        let i = idx as usize;
        i < self.n() && self.slot(i).used()
    }

    pub fn is_dir(&self, idx: u32) -> bool {
        self.valid(idx) && self.slot(idx as usize).is_dir()
    }

    /// 更新目录的聚合体积，并沿父链加减差值。
    pub fn set_dir_totals(&mut self, idx: u32, size: u64, files: u64) {
        if !self.valid(idx) || !self.slot(idx as usize).is_dir() {
            return;
        }
        let old_size = self.slot(idx as usize).size;
        let old_files = self.slot(idx as usize).file_count as u64;
        self.update_slot(idx as usize, |e| {
            e.size = size;
            e.file_count = files.min(u32::MAX as u64) as u32;
        });
        if size == old_size && files == old_files {
            return;
        }
        let mut cur = self.slot(idx as usize).parent();
        loop {
            if !self.valid(cur) {
                break;
            }
            let i = cur as usize;
            self.update_slot(i, |e| {
                if size >= old_size {
                    e.size = e.size.saturating_add(size - old_size);
                } else {
                    e.size = e.size.saturating_sub(old_size - size);
                }
                if files >= old_files {
                    e.file_count = e
                        .file_count
                        .saturating_add((files - old_files).min(u32::MAX as u64) as u32);
                } else {
                    e.file_count = e
                        .file_count
                        .saturating_sub((old_files - files).min(u32::MAX as u64) as u32);
                }
            });
            if cur == ROOT_NODE {
                break;
            }
            cur = self.slot(i).parent();
        }
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
        self.slot(idx as usize).size
    }

    pub fn file_count_of(&self, idx: u32) -> u64 {
        if !self.valid(idx) {
            return 0;
        }
        let e = self.slot(idx as usize);
        if e.is_dir() {
            e.file_count as u64
        } else {
            1
        }
    }

    pub fn parent_of(&self, idx: u32) -> Option<u32> {
        if idx == ROOT_NODE || !self.valid(idx) {
            return None;
        }
        let p = self.slot(idx as usize).parent();
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
            self.update_slot(i, |e| {
                e.set_used(false);
                if e.is_dir() {
                    e.size = 0;
                    e.file_count = 0;
                }
            });
            // 把子节点压栈继续处理
            for &child in self.child_slice(cur).iter() {
                if self.valid(child) {
                    stack.push(child);
                }
            }
        }

        // 沿父链扣减祖先目录的聚合大小和文件数
        let mut cur = self.slot(idx as usize).parent();
        loop {
            if !self.valid(cur) {
                break;
            }
            let i = cur as usize;
            self.update_slot(i, |e| {
                e.size = e.size.saturating_sub(removed_size);
                e.file_count = e
                    .file_count
                    .saturating_sub(removed_files.min(u32::MAX as u64) as u32);
            });
            if cur == ROOT_NODE {
                break;
            }
            cur = self.slot(i).parent();
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

        // 回溯父链。只有走到根或命中缓存才算解析成功；父链断裂（槽位失效、
        // 自环、深度超限）一律返回空串，与 `!self.valid(idx)` 同一档处理。
        // 以前这些情况会拿挂载点当前缀，把残缺的链拼成一条看着合法、实则
        // 指向别处的路径——磁盘透镜会拿它去删文件。
        let mut chain: Vec<u32> = Vec::new();
        let mut cur = idx;
        let mut base = None;
        let mut depth = 0;
        // PATH_MAX 是 1024 字节，每层至少占「一个字符 + 分隔符」，
        // 512 层足够覆盖任何真实路径，同时仍能兜住父链成环。
        const MAX_DEPTH: usize = 512;

        loop {
            if cur == ROOT_NODE {
                base = Some(self.volume().mount_point().display().to_string());
                break;
            }
            if depth > MAX_DEPTH {
                break;
            }
            if let Some(hit) = cache.get(&cur) {
                base = Some(hit.clone());
                break;
            }
            let i = cur as usize;
            if i >= self.n() || !self.slot(i).used() {
                break;
            }
            chain.push(cur);
            let next = self.slot(i).parent();
            if next == cur {
                break;
            }
            cur = next;
            depth += 1;
        }

        let Some(base) = base else {
            return String::new();
        };

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
        if let Some(v) = self.extra_child.get(&idx) {
            return v;
        }
        if let Some(m) = &self.mapped {
            if (idx as usize) < m.n {
                return m.child_slice(idx);
            }
            return &[];
        }
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

    /// mmap 主体的子列表不可原地插入。第一次给某个父节点加 overlay
    /// 孩子时，把原 CSR 复制出来再追加。
    fn overlay_link_child(&mut self, parent: u32, child: u32) {
        if self.mapped.is_none() {
            return;
        }
        if let Some(kids) = self.extra_child.get_mut(&parent) {
            kids.push(child);
            return;
        }
        let mut kids = self.child_slice(parent).to_vec();
        kids.push(child);
        self.extra_child.insert(parent, kids);
    }

    fn own_size(&self, idx: u32) -> u64 {
        self.child_slice(idx)
            .iter()
            .filter(|&&c| self.valid(c) && !self.slot(c as usize).is_dir())
            .map(|&c| self.slot(c as usize).size)
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
                let e = self.slot(c as usize);
                Node {
                    idx: c,
                    name: self.entry_name_str(c).to_string(),
                    is_dir: e.is_dir(),
                    size: e.size,
                    file_count: if e.is_dir() { e.file_count as u64 } else { 1 },
                    own_size: if e.is_dir() { self.own_size(c) } else { e.size },
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
        for i in 0..self.n() {
            let e = self.slot(i);
            if !e.used() || e.is_dir() || e.size == 0 {
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
                let e = self.slot(c as usize);
                if e.is_dir() {
                    if depth < max_depth
                        && !crate::core::disk::is_declutter_ignored_dir_name(self.entry_name_str(c))
                    {
                        stack.push((c, depth + 1));
                    }
                } else if e.size >= min_size && e.size <= max_size {
                    out.push((c, e.size, e.mtime as u64));
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
        let generation = std::sync::atomic::AtomicU64::new(0);
        self.search_cancellable(query, max_results, &generation, 0)
    }

    pub fn search_cancellable(
        &self,
        query: &str,
        max_results: usize,
        generation: &std::sync::atomic::AtomicU64,
        expected_generation: u64,
    ) -> Vec<crate::core::disk::SearchHit> {
        if max_results == 0 {
            return Vec::new();
        }
        use crate::core::disk::NamePattern;
        let pattern = NamePattern::parse(query);
        if matches!(pattern, NamePattern::Empty) {
            return self.search_top_by_size(max_results, generation, expected_generation);
        }
        let mut cache = HashMap::new();
        let mut hits = Vec::new();
        for i in 0..self.n() {
            if i % 4096 == 0
                && generation.load(std::sync::atomic::Ordering::Relaxed) != expected_generation
            {
                return Vec::new();
            }
            let e = self.slot(i);
            if !e.used() {
                continue;
            }
            let name = self.entry_name_str(i as u32);
            if !pattern.matches_raw(name) {
                continue;
            }
            let path = self.path_of_with(i as u32, &mut cache);
            hits.push(crate::core::disk::SearchHit {
                path,
                name: name.to_string(),
                is_dir: e.is_dir(),
                size: e.size,
                mtime: e.mtime as u64,
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
    fn search_top_by_size(
        &self,
        max_results: usize,
        generation: &std::sync::atomic::AtomicU64,
        expected_generation: u64,
    ) -> Vec<crate::core::disk::SearchHit> {
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;

        // 只保留当前最大的 N 项。旧实现先为全部节点构造 `(size, idx)`
        // 向量，16M 节点会瞬间分配约 250MB，并让 allocator 长期保留大块。
        let mut heap: BinaryHeap<Reverse<(u64, u32)>> = BinaryHeap::with_capacity(max_results + 1);
        for i in 0..self.n() {
            if i % 4096 == 0
                && generation.load(std::sync::atomic::Ordering::Relaxed) != expected_generation
            {
                return Vec::new();
            }
            let entry = self.slot(i);
            if !entry.used() {
                continue;
            }
            if heap.len() == max_results
                && entry.size <= heap.peek().map(|Reverse((size, _))| *size).unwrap_or(0)
            {
                continue;
            }
            heap.push(Reverse((entry.size, i as u32)));
            if heap.len() > max_results {
                heap.pop();
            }
        }
        let mut sized: Vec<(u64, u32)> = heap.into_iter().map(|Reverse(item)| item).collect();
        sized.sort_unstable_by_key(|item| Reverse(item.0));

        let mut cache = HashMap::new();
        let mut hits = Vec::with_capacity(sized.len());
        for (size, idx) in sized {
            let e = self.slot(idx as usize);
            let name = self.entry_name_str(idx).to_string();
            let path = self.path_of_with(idx, &mut cache);
            hits.push(crate::core::disk::SearchHit {
                path,
                name,
                is_dir: e.is_dir(),
                size,
                mtime: e.mtime as u64,
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
        self.update_slot(idx as usize, |e| e.set_used(false));
        for child in children {
            self.mark_unused_recursive(child);
        }
        // 沿父链减去被移除子树的大小和文件数
        let mut cur = self.slot(idx as usize).parent();
        loop {
            if cur == idx || !self.valid(cur) {
                break;
            }
            let i = cur as usize;
            self.update_slot(i, |e| {
                if e.is_dir() {
                    e.size = e.size.saturating_sub(size);
                    e.file_count = e
                        .file_count
                        .saturating_sub(files.min(u32::MAX as u64) as u32);
                }
            });
            if cur == ROOT_NODE {
                break;
            }
            cur = self.slot(i).parent();
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
        let name_off = self.pool_push(&name_str);

        self.entries
            .push(TreeEntry::new(parent, name_off, false, size, mtime, 1));
        let new_idx = (self.n() - 1) as u32;
        self.overlay_link_child(parent, new_idx);

        let mut cur = parent;
        loop {
            let i = cur as usize;
            self.update_slot(i, |e| {
                e.size = e.size.saturating_add(size);
                e.file_count = e.file_count.saturating_add(1);
            });
            if cur == ROOT_NODE {
                break;
            }
            cur = self.slot(i).parent();
        }
        true
    }

    fn mark_unused_recursive(&mut self, idx: u32) {
        if !self.valid(idx) {
            return;
        }
        let children: Vec<u32> = self.child_slice(idx).to_vec();
        self.update_slot(idx as usize, |e| e.set_used(false));
        for child in children {
            self.mark_unused_recursive(child);
        }
    }

    fn subtree_totals(&self, idx: u32) -> (u64, u64) {
        if !self.valid(idx) {
            return (0, 0);
        }
        let e = self.slot(idx as usize);
        if e.is_dir() {
            (e.size, e.file_count as u64)
        } else {
            (e.size, 1)
        }
    }

    /// 在指定父节点下追加一棵子树的所有节点。
    ///
    /// 子树的根节点（idx 0）映射为 `parent_idx` 的子节点。
    /// `root_name` 覆盖子树根节点的名称（因为子树是用完整路径作为
    /// volume label 扫描的，名称是完整路径，需要替换为目录名）。
    ///
    /// 调用后需调用 `rebuild_child_arrays` 重建 CSR 索引（堆上主体）；
    /// mmap 主体走 `extra_child` overlay，不重建整棵 CSR。
    pub fn append_subtree(&mut self, parent_idx: u32, subtree: &SizeTree, root_name: &str) {
        if !self.valid(parent_idx) || !self.slot(parent_idx as usize).is_dir() {
            return;
        }
        let (sub_total_size, sub_total_files) = subtree.subtree_totals(subtree.root());
        let sub_n = subtree.n();
        self.entries.reserve(sub_n);
        let mut map = vec![u32::MAX; sub_n];

        for i in 0..sub_n {
            let entry = subtree.slot(i);
            if !entry.used() {
                continue;
            }
            let new_idx = self.n() as u32;
            let new_parent = if i == 0 {
                parent_idx
            } else {
                map[entry.parent() as usize]
            };
            if i > 0 && new_parent == u32::MAX {
                continue;
            }
            let name = if i == 0 {
                root_name
            } else {
                subtree.entry_name_str(i as u32)
            };
            let name_off = self.pool_push(name);
            self.entries.push(TreeEntry::new(
                new_parent,
                name_off,
                entry.is_dir(),
                entry.size,
                entry.mtime as u64,
                entry.file_count,
            ));
            map[i] = new_idx;
            self.overlay_link_child(new_parent, new_idx);
        }

        // 沿父链加上新子树的聚合大小
        let mut cur = parent_idx;
        loop {
            if !self.valid(cur) {
                break;
            }
            let i = cur as usize;
            self.update_slot(i, |e| {
                if e.is_dir() {
                    e.size = e.size.saturating_add(sub_total_size);
                    e.file_count = e
                        .file_count
                        .saturating_add(sub_total_files.min(u32::MAX as u64) as u32);
                }
            });
            if cur == ROOT_NODE {
                break;
            }
            cur = self.slot(i).parent();
        }
    }

    /// 从 entries 数组重建 CSR 子节点索引。
    /// 在完成所有 `append_subtree` / `remove_subtree_inplace` 操作后调用一次。
    pub fn rebuild_child_arrays(&mut self) {
        if self.mapped.is_some() {
            return;
        }
        let (child_start, child_at) = build_csr(&self.entries);
        self.child_start = child_start;
        self.child_at = child_at;
    }

    /// 统计当前已使用（`used = true`）的目录节点数。
    pub fn count_used_dirs(&self) -> u64 {
        (0..self.n())
            .filter(|&i| {
                let e = self.slot(i);
                e.used() && e.is_dir()
            })
            .count() as u64
    }

    /// 统计当前已使用（`used = true`）的文件节点数。
    pub fn count_used_files(&self) -> u64 {
        (0..self.n())
            .filter(|&i| {
                let e = self.slot(i);
                e.used() && !e.is_dir()
            })
            .count() as u64
    }

    /// 从头重新计算所有目录的聚合大小和文件数。
    /// 仅供测试验证增量更新正确性时使用。
    pub fn recompute_aggregates(&mut self) {
        let n = self.n();
        for i in 0..n {
            let e = self.slot(i);
            if e.used() && e.is_dir() && (e.size != 0 || e.file_count != 0) {
                self.update_slot(i, |d| {
                    d.size = 0;
                    d.file_count = 0;
                });
            }
        }
        for i in 0..n {
            let e = self.slot(i);
            if !e.used() || e.is_dir() {
                continue;
            }
            let add_size = e.size;
            let add_files = 1u32;
            let mut cur = e.parent();
            loop {
                let idx = cur as usize;
                if idx >= n || !self.slot(idx).used() {
                    break;
                }
                self.update_slot(idx, |parent| {
                    parent.size = parent.size.saturating_add(add_size);
                    parent.file_count = parent.file_count.saturating_add(add_files);
                });
                if cur == ROOT_NODE {
                    break;
                }
                cur = self.slot(idx).parent();
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

#[cfg(test)]
mod resolve_path_tests {
    use super::*;

    fn entry(parent: u32, name: &str, is_dir: bool) -> TreeIndexEntry {
        TreeIndexEntry {
            parent,
            name: name.to_string(),
            is_dir,
            size: 0,
            used: true,
            mtime: 0,
        }
    }

    fn tree(entries: Vec<TreeIndexEntry>) -> SizeTree {
        SizeTree::from_compact(VolumeId::from_mount_point(PathBuf::from("/")), entries)
    }

    #[test]
    fn resolves_normal_chain() {
        let t = tree(vec![
            entry(ROOT_NODE, "/", true),
            entry(ROOT_NODE, "Users", true),
            entry(1, "me", true),
            entry(2, "a.txt", false),
        ]);
        assert_eq!(t.path_of(3), "/Users/me/a.txt");
    }

    /// 父链断裂时必须返回空串，而不是拿挂载点当前缀拼出一条
    /// 看着合法、实则指向别处的路径——上层会拿它去删文件。
    #[test]
    fn self_referencing_parent_yields_empty() {
        // 索引 1 的父指向自己：既到不了根，也不该伪装成 /orphan
        let t = tree(vec![entry(ROOT_NODE, "/", true), entry(1, "orphan", true)]);
        assert_eq!(t.path_of(1), "");
    }

    #[test]
    fn over_deep_chain_yields_empty() {
        let mut entries = vec![entry(ROOT_NODE, "/", true)];
        // 造一条超过 MAX_DEPTH(512) 的链
        for i in 1..=600u32 {
            entries.push(entry(i - 1, &format!("d{i}"), true));
        }
        let t = tree(entries);
        assert_eq!(t.path_of(600), "");
        // 深度以内的仍然正常解析
        assert!(t.path_of(10).ends_with("/d10"));
    }
}

#[cfg(test)]
mod v7_header_validation_tests {
    use super::*;

    fn meta() -> IndexMeta {
        IndexMeta {
            mount: "/".into(),
            label: "/".into(),
            file_count: 1,
            dir_count: 1,
            total_size: 0,
            last_event_id: 0,
            scanned_at: 0,
        }
    }

    fn sample_tree() -> SizeTree {
        let e = |parent: u32, name: &str, is_dir: bool| TreeIndexEntry {
            parent,
            name: name.to_string(),
            is_dir,
            size: if is_dir { 0 } else { 16 },
            used: true,
            mtime: 0,
        };
        SizeTree::from_compact(
            VolumeId::from_mount_point(PathBuf::from("/")),
            vec![
                e(ROOT_NODE, "/", true),
                e(ROOT_NODE, "Users", true),
                e(1, "a.txt", false),
            ],
        )
    }

    /// 两条写盘路径必须产出语义相同的索引。
    ///
    /// 布局不同（原地把名字池摆在条目之前，流式摆在文件末尾再截断），
    /// 所以不能比字节；但节点数、路径、体积、header 统计必须一致——
    /// 它们本来就是同一棵树的两种落盘方式，差异只该出现在偏移上。
    #[test]
    fn inplace_and_streaming_writers_agree() {
        let dir = std::env::temp_dir().join(format!("{}_{}", "qc_v7_equiv", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let tree = sample_tree();
        assert!(
            tree.can_write_inplace(),
            "样本树必须走得通原地路径，否则这个测试比的是同一条路径"
        );

        // header 里的文件/目录数要和实际条目对得上，否则加载校验会拒收
        // （`meta()` 那份 1/1 是给不做 verify 的用例准备的）。
        let real_meta = || IndexMeta {
            file_count: 1,
            dir_count: 2,
            total_size: 16,
            ..meta()
        };
        let a = dir.join("inplace.bin");
        let b = dir.join("streaming.bin");
        tree.write_v7(&a, real_meta()).expect("原地写应成功");
        tree.write_v7_streaming(&b, &real_meta())
            .expect("流式写应成功");

        let vol = VolumeId::from_mount_point(PathBuf::from("/"));
        let la = SizeTree::from_mapped(vol.clone(), &a).expect("原地索引应可加载");
        let lb = SizeTree::from_mapped(vol, &b).expect("流式索引应可加载");

        assert_eq!(la.n(), lb.n());
        assert_eq!(la.mapped_header_stats(), lb.mapped_header_stats());
        for i in 0..la.n() as u32 {
            assert_eq!(la.path_of(i), lb.path_of(i), "节点 {i} 的路径不一致");
            assert_eq!(la.size_of(i), lb.size_of(i), "节点 {i} 的体积不一致");
            assert_eq!(
                la.file_count_of(i),
                lb.file_count_of(i),
                "节点 {i} 的文件数不一致"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// CSR 的两段偏移会被当成 `&[u32]` 取用，未对齐就是 UB。
    /// 长度校验挡不住它：把偏移 +1 仍然落在文件内，只是不再 4 字节对齐。
    /// 加载器的职责是校验不可信的磁盘数据，必须拒绝这种文件而不是照单全收。
    #[test]
    fn misaligned_csr_offsets_are_rejected() {
        let dir = std::env::temp_dir().join(format!("{}_{}", "qc_v7_align", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("index.bin");
        sample_tree().write_v7(&path, meta()).expect("写索引应成功");

        // 控制组：没改过的文件能正常加载
        assert!(
            map_index_file(&path, false).is_some(),
            "原始索引应当可加载，否则下面的断言证明不了是对齐拦下的"
        );

        // 88..92 是 cs_off，92..96 是 ca_off
        for field in [88usize, 92] {
            let mut bytes = std::fs::read(&path).unwrap();
            let cur = u32::from_le_bytes(bytes[field..field + 4].try_into().unwrap());
            bytes[field..field + 4].copy_from_slice(&(cur + 1).to_le_bytes());
            let bad = dir.join(format!("bad-{field}.bin"));
            std::fs::write(&bad, &bytes).unwrap();

            assert!(
                map_index_file(&bad, false).is_none(),
                "偏移字段 {field} 未按 4 字节对齐时必须拒绝加载"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
