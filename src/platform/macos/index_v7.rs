//! v7 索引文件格式：布局、header、输出映射与名字池。
//!
//! # 为什么单独一个模块
//!
//! 写 v7 的地方有三处，各有各的来源：`SizeTree::write_v7` 的原地快路径
//! （全量扫描、无墓碑，数组可以整块拷）、`SizeTree::write_v7_streaming`
//! （有墓碑或挂着 mmap 主体，需要压实）、以及 `walk::build_size_tree_streaming`
//! （从溢写文件直接落盘，内存里从来没有过完整的树）。三者的**遍历方式**
//! 确实不同，但落到磁盘上的字节布局是同一套。
//!
//! 之前三处各算一遍偏移、各写一遍 header 的 0..96 字节、各自手搓一个
//! munmap 的 Drop guard、各自实现一遍名字 intern。而加载端
//! （`disk_tree::map_index_fd`）会校验 CSR 两段偏移的 4 字节对齐——写端
//! 有三份就意味着改布局时漏改一份，产出的索引要么加载失败，要么把未对齐
//! 的字节按 `&[u32]` 取用（UB）。
//!
//! 所以偏移只在 [`V7Layout`] 里算一次，header 只在 [`V7Header::write_into`]
//! 里写一次，映射生命周期只由 [`MmapOut`] 管一次。
//!
//! # 文件布局
//!
//! ```text
//! header(128B) | mount | label | ...  entries | CSR child_start | CSR child_at ... | names
//! ```
//!
//! 名字池的位置有两种，见 [`V7Layout::names_inline`] 与
//! [`V7Layout::names_trailing`]。

use super::disk_tree::TreeEntry;
use std::collections::HashMap;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

pub(crate) const INDEX_V7_MAGIC: &[u8; 8] = b"QCIDXV07";
pub(crate) const INDEX_V7_HEADER: usize = 128;

/// 一条 `TreeEntry` 在文件里的字节数。
pub(crate) const ENTRY_SIZE: usize = std::mem::size_of::<TreeEntry>();

/// 脏页上限：写大索引时每攒这么多就 `MS_ASYNC` 一次，避免 footprint 冲高。
const MSYNC_CHUNK: usize = 32 * 1024 * 1024;

// ---------------------------------------------------------------- 校验和

pub(crate) fn fnv1a64_bytes(data: &[u8]) -> u64 {
    fnv1a64_update(0xcbf29ce484222325u64, data)
}

fn fnv1a64_update(mut hash: u64, data: &[u8]) -> u64 {
    for byte in data {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

/// base 校验覆盖完整文件；checksum 字段自身按 8 个零字节计算。
pub(crate) fn index_checksum_bytes(bytes: &[u8]) -> u64 {
    if bytes.len() < 80 {
        return 0;
    }
    let mut hash = fnv1a64_bytes(&bytes[..72]);
    hash = fnv1a64_update(hash, &[0; 8]);
    fnv1a64_update(hash, &bytes[80..])
}

/// delta 校验覆盖 header 与 payload；checksum 字段自身按零处理。
pub(crate) fn delta_checksum(header: &[u8], payload: &[u8]) -> u64 {
    if header.len() < 108 {
        return 0;
    }
    let mut hash = fnv1a64_bytes(&header[..100]);
    hash = fnv1a64_update(hash, &[0; 8]);
    hash = fnv1a64_update(hash, &header[108..]);
    fnv1a64_update(hash, payload)
}

/// 算完整个文件后回填 72..80 的全文件 checksum。
pub(crate) fn finalize_checksum(buf: &mut [u8], len: usize) {
    let sum = index_checksum_bytes(&buf[..len]);
    buf[72..80].copy_from_slice(&sum.to_le_bytes());
}

pub(crate) fn entries_as_bytes(entries: &[TreeEntry]) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(
            entries.as_ptr() as *const u8,
            std::mem::size_of_val(entries),
        )
    }
}

// ------------------------------------------------------------- 堆名字池

/// 往堆池尾部追加一条名字（2 字节小端长度前缀 + 字节），返回偏移。
pub(crate) fn push_name(pool: &mut Vec<u8>, name: &[u8]) -> u32 {
    let off = pool.len() as u32;
    let len = name.len().min(u16::MAX as usize) as u16;
    pool.extend_from_slice(&len.to_le_bytes());
    pool.extend_from_slice(&name[..len as usize]);
    off
}

pub(crate) fn pool_bytes(pool: &[u8], off: u32) -> &[u8] {
    let i = off as usize;
    if i + 2 > pool.len() {
        return b"";
    }
    let len = u16::from_le_bytes([pool[i], pool[i + 1]]) as usize;
    let start = i + 2;
    let end = start.saturating_add(len).min(pool.len());
    &pool[start..end]
}

pub(crate) fn pool_str(pool: &[u8], off: u32) -> &str {
    std::str::from_utf8(pool_bytes(pool, off)).unwrap_or("")
}

/// 名字 intern：把相同文件名合成池里的一条。
pub(crate) struct NameInterner {
    pool: Vec<u8>,
    map: HashMap<u64, u32>,
}

impl NameInterner {
    pub(crate) fn with_capacity(n: usize) -> Self {
        Self {
            pool: Vec::with_capacity(n.saturating_mul(12)),
            map: HashMap::with_capacity(n / 4),
        }
    }

    pub(crate) fn intern(&mut self, name: &[u8]) -> u32 {
        let h = fnv1a64_bytes(name);
        if let Some(&off) = self.map.get(&h) {
            // 哈希碰撞防护：字节一致才算命中
            if pool_bytes(&self.pool, off) == name {
                return off;
            }
        }
        let off = push_name(&mut self.pool, name);
        self.map.insert(h, off);
        off
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        let mut pool = self.pool;
        pool.shrink_to_fit();
        pool
    }
}

// ----------------------------------------------------------------- 布局

/// v7 文件里各段的字节偏移。
///
/// 加载端按 `&[u32]` 取用 CSR 两段、按 `&[TreeEntry]` 取用条目段，所以
/// 这里的对齐不是风格问题：`ent_off` 必须 8 对齐，`cs_off` / `ca_off`
/// 必须 4 对齐，否则加载器会（正确地）拒绝这个文件。
#[derive(Debug, Clone, Copy)]
pub(crate) struct V7Layout {
    pub n: usize,
    pub name_off: usize,
    pub ent_off: usize,
    pub cs_off: usize,
    pub ca_off: usize,
    pub ca_len: usize,
    /// 文件总长；名字池在尾部时这是**预留**长度，写完要截断到实际值。
    pub len: usize,
}

#[inline]
fn align8(x: usize) -> usize {
    (x + 7) & !7
}

impl V7Layout {
    /// 名字池在条目之前：`header | mount/label | names | entries | CSR`。
    ///
    /// 原地快路径用这套——池已经是现成的 `Vec<u8>`，长度确定，直接摆前面。
    pub(crate) fn names_inline(
        n: usize,
        mount_len: usize,
        label_len: usize,
        pool_len: usize,
        ca_len: usize,
    ) -> Self {
        let name_off = align8(INDEX_V7_HEADER + mount_len + label_len);
        let ent_off = align8(name_off + pool_len);
        let cs_off = ent_off + n * ENTRY_SIZE;
        let ca_off = cs_off + (n + 1) * 4;
        Self {
            n,
            name_off,
            ent_off,
            cs_off,
            ca_off,
            ca_len,
            len: ca_off + ca_len * 4,
        }
        .checked()
    }

    /// 名字池在最后：`header | mount/label | entries | CSR | names(预留)`。
    ///
    /// 两条流式路径用这套——写之前只知道池的**上限**，边写边 intern，
    /// 写完把预留的尾巴截掉，所以池只能放在文件末尾。
    pub(crate) fn names_trailing(
        n: usize,
        mount_len: usize,
        label_len: usize,
        ca_len: usize,
        pool_reserve: usize,
    ) -> Self {
        let ent_off = align8(INDEX_V7_HEADER + mount_len + label_len);
        let cs_off = ent_off + n * ENTRY_SIZE;
        let ca_off = cs_off + (n + 1) * 4;
        let pool_off = ca_off + ca_len * 4;
        Self {
            n,
            name_off: pool_off,
            ent_off,
            cs_off,
            ca_off,
            ca_len,
            len: pool_off.saturating_add(pool_reserve),
        }
        .checked()
    }

    fn checked(self) -> Self {
        debug_assert_eq!(self.ent_off % 8, 0, "条目段必须 8 字节对齐");
        debug_assert_eq!(self.cs_off % 4, 0, "child_start 必须 4 字节对齐");
        debug_assert_eq!(self.ca_off % 4, 0, "child_at 必须 4 字节对齐");
        self
    }

    /// 写入 mount / label 两段变长字符串（紧跟在 header 之后）。
    pub(crate) fn write_names(&self, buf: &mut [u8], mount: &[u8], label: &[u8]) {
        let m = INDEX_V7_HEADER;
        buf[m..m + mount.len()].copy_from_slice(mount);
        buf[m + mount.len()..m + mount.len() + label.len()].copy_from_slice(label);
    }

    /// 把 CSR 前缀和写进 `child_start`（末位是 `ca_len` 哨兵）。
    pub(crate) fn write_child_start(&self, buf: &mut [u8], start: &[u32]) {
        debug_assert_eq!(start.len(), self.n);
        for (i, s) in start.iter().enumerate() {
            let o = self.cs_off + i * 4;
            buf[o..o + 4].copy_from_slice(&s.to_le_bytes());
        }
        let o = self.cs_off + self.n * 4;
        buf[o..o + 4].copy_from_slice(&(self.ca_len as u32).to_le_bytes());
    }

    /// 流式写入时把 `child` 挂到 `parent` 名下。
    ///
    /// `child_start` 此刻被当成游标用：读出当前写位、写进 `child_at`、游标 +1。
    /// 全部挂完之后调用方要用真正的前缀和重写一遍 `child_start`。
    pub(crate) fn push_child(&self, buf: &mut [u8], parent: u32, child: u32) {
        let co4 = self.cs_off + parent as usize * 4;
        let cur = u32::from_le_bytes(buf[co4..co4 + 4].try_into().unwrap()) as usize;
        let co = self.ca_off + cur * 4;
        buf[co..co + 4].copy_from_slice(&child.to_le_bytes());
        buf[co4..co4 + 4].copy_from_slice(&((cur + 1) as u32).to_le_bytes());
    }

    /// 第 `i` 条目在文件里的字节区间。
    #[inline]
    pub(crate) fn entry_at(&self, i: usize) -> std::ops::Range<usize> {
        let o = self.ent_off + i * ENTRY_SIZE;
        o..o + ENTRY_SIZE
    }
}

// --------------------------------------------------------------- header

/// header 的 0..96 字节。96..128 是保留区，留零。
pub(crate) struct V7Header<'a> {
    pub layout: &'a V7Layout,
    pub mount: &'a [u8],
    pub label: &'a [u8],
    /// 名字池的**实际**字节数（流式路径下小于预留量）。
    pub name_len: usize,
    pub file_count: u64,
    pub dir_count: u64,
    pub total_size: u64,
    pub last_event_id: u64,
    pub scanned_at: u64,
}

impl V7Header<'_> {
    pub(crate) fn write_into(&self, buf: &mut [u8]) {
        let l = self.layout;
        buf[0..8].copy_from_slice(INDEX_V7_MAGIC);
        buf[8..12].copy_from_slice(&7u32.to_le_bytes());
        buf[12..16].copy_from_slice(&(l.n as u32).to_le_bytes());
        buf[16..20].copy_from_slice(&(self.name_len as u32).to_le_bytes());
        buf[20..24].copy_from_slice(&(l.ca_len as u32).to_le_bytes());
        buf[24..28].copy_from_slice(&(self.mount.len() as u32).to_le_bytes());
        buf[28..32].copy_from_slice(&(self.label.len() as u32).to_le_bytes());
        buf[32..40].copy_from_slice(&self.file_count.to_le_bytes());
        buf[40..48].copy_from_slice(&self.dir_count.to_le_bytes());
        buf[48..56].copy_from_slice(&self.total_size.to_le_bytes());
        buf[56..64].copy_from_slice(&self.last_event_id.to_le_bytes());
        buf[64..72].copy_from_slice(&self.scanned_at.to_le_bytes());
        // 72..80 是 checksum，等全文写完由 finalize_checksum 回填
        buf[80..84].copy_from_slice(&(l.name_off as u32).to_le_bytes());
        buf[84..88].copy_from_slice(&(l.ent_off as u32).to_le_bytes());
        buf[88..92].copy_from_slice(&(l.cs_off as u32).to_le_bytes());
        buf[92..96].copy_from_slice(&(l.ca_off as u32).to_le_bytes());
        l.write_names(buf, self.mount, self.label);
    }
}

/// 当前时刻的 Unix 秒，写进 header 的 `scanned_at`。
pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ----------------------------------------------------------- 输出映射

/// 写 v7 用的 `MAP_SHARED` 输出映射。
///
/// 三个 writer 之前各有一份手写的 munmap guard，walk 那条还额外挂了一个
/// 删文件的 `OutputCleanup`。这里统一：Drop 先 `MADV_DONTNEED` 把脏页还
/// 给内核再 `munmap`（否则 physical footprint 会在高位驻留），然后删掉
/// 这个路径。
///
/// **Drop 无条件删文件**是刻意的：三个 writer 写的都是临时路径，成功时
/// 要么已经 `rename_to` 走了（删除落空，无害），要么调用方拿它建完映射
/// 后自己删（unlink 之后映射页依然有效）。中途出错则正好不留残骸。
pub(crate) struct MmapOut {
    ptr: *mut u8,
    len: usize,
    file: std::fs::File,
    path: PathBuf,
}

impl MmapOut {
    /// 建一个 `len` 字节的文件并整体映射进来。
    pub(crate) fn create(path: PathBuf, len: usize) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        file.set_len(len as u64)?;
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            let err = std::io::Error::last_os_error();
            let _ = std::fs::remove_file(&path);
            return Err(err);
        }
        Ok(Self {
            ptr: ptr as *mut u8,
            len,
            file,
            path,
        })
    }

    /// 映射首地址。拿它做 `msync` 不会借住整个映射，写循环期间也能调。
    #[inline]
    pub(crate) fn ptr(&self) -> *mut u8 {
        self.ptr
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    /// 落盘并按 `len` 截断（尾部预留的名字池空间还回去）。
    ///
    /// 截断必须用建映射时那个可写句柄——`File::open` 拿到的只读句柄
    /// `set_len` 会 EINVAL。
    pub(crate) fn commit(&mut self, len: usize) -> std::io::Result<()> {
        let sync = unsafe { libc::msync(self.ptr as *mut libc::c_void, len, libc::MS_SYNC) };
        self.unmap();
        if sync != 0 {
            return Err(std::io::Error::last_os_error());
        }
        self.file.set_len(len as u64)
    }

    /// commit 之后把文件挪到最终位置。
    pub(crate) fn rename_to(&self, target: &std::path::Path) -> std::io::Result<()> {
        std::fs::rename(&self.path, target)
    }

    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn unmap(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                // 先把脏页还给内核再 munmap，避免 footprint 高位驻留
                libc::madvise(self.ptr as *mut libc::c_void, self.len, libc::MADV_DONTNEED);
                libc::munmap(self.ptr as *mut libc::c_void, self.len);
            }
            self.ptr = std::ptr::null_mut();
        }
    }
}

impl Drop for MmapOut {
    fn drop(&mut self) {
        self.unmap();
        let _ = std::fs::remove_file(&self.path);
    }
}

// ------------------------------------------------------- 映射内名字池

/// 边写边 intern 到映射页里的名字池。
///
/// 两条流式路径都不能先把池物化在堆上——16M 条目的池是百 MB 级，而流式
/// 写的全部意义就是不物化。所以池直接写进输出映射，`cursor` 是文件内的
/// 绝对偏移，条目里存的是相对 `pool_off` 的偏移。
pub(crate) struct MmapPool {
    pool_off: usize,
    cursor: usize,
    synced: usize,
    map: HashMap<u64, u32>,
}

impl MmapPool {
    pub(crate) fn new(pool_off: usize, n: usize) -> Self {
        Self {
            pool_off,
            cursor: pool_off,
            synced: 0,
            map: HashMap::with_capacity(n / 4),
        }
    }

    /// 返回名字在池内的相对偏移；重名复用已有的那条。
    pub(crate) fn intern(&mut self, buf: &mut [u8], name: &[u8]) -> u32 {
        let h = fnv1a64_bytes(name);
        if let Some(&off) = self.map.get(&h) {
            // 哈希碰撞防护：字节一致才算命中
            let p = self.pool_off + off as usize;
            let len = u16::from_le_bytes([buf[p], buf[p + 1]]) as usize;
            if len == name.len() && &buf[p + 2..p + 2 + len] == name {
                return off;
            }
        }
        let off = (self.cursor - self.pool_off) as u32;
        buf[self.cursor..self.cursor + 2].copy_from_slice(&(name.len() as u16).to_le_bytes());
        self.cursor += 2;
        buf[self.cursor..self.cursor + name.len()].copy_from_slice(name);
        self.cursor += name.len();
        self.map.insert(h, off);
        off
    }

    /// 攒够 [`MSYNC_CHUNK`] 就异步刷一次，控制脏页上限。
    pub(crate) fn maybe_flush(&mut self, ptr: *mut u8) {
        if self.cursor - self.synced > MSYNC_CHUNK {
            unsafe {
                libc::msync(ptr as *mut libc::c_void, self.cursor, libc::MS_ASYNC);
            }
            self.synced = self.cursor;
        }
    }

    /// 已写到的文件内绝对偏移——也就是文件的实际长度。
    #[inline]
    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    /// 名字池的实际字节数。
    #[inline]
    pub(crate) fn name_len(&self) -> usize {
        self.cursor - self.pool_off
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 加载端按 `&[u32]` / `&[TreeEntry]` 直接取用这几段，偏移没对齐就是 UB。
    /// mount/label 是变长的，正好能把对齐算错。
    #[test]
    fn layouts_are_aligned_for_any_string_length() {
        for mount_len in 0..24usize {
            for label_len in 0..24usize {
                let inline = V7Layout::names_inline(7, mount_len, label_len, 13, 5);
                assert_eq!(inline.ent_off % 8, 0);
                assert_eq!(inline.cs_off % 4, 0);
                assert_eq!(inline.ca_off % 4, 0);
                assert!(inline.name_off + 13 <= inline.ent_off);

                let trailing = V7Layout::names_trailing(7, mount_len, label_len, 5, 13);
                assert_eq!(trailing.ent_off % 8, 0);
                assert_eq!(trailing.cs_off % 4, 0);
                assert_eq!(trailing.ca_off % 4, 0);
                assert_eq!(trailing.name_off, trailing.ca_off + 5 * 4);
            }
        }
    }

    #[test]
    fn interner_dedups_and_round_trips() {
        let mut it = NameInterner::with_capacity(4);
        let a = it.intern(b"Library");
        let b = it.intern(b"Caches");
        assert_eq!(it.intern(b"Library"), a);
        let pool = it.finish();
        assert_eq!(pool_str(&pool, a), "Library");
        assert_eq!(pool_str(&pool, b), "Caches");
    }

    /// 映射内池和堆池写的是同一种编码，条目里的偏移可以互换解释。
    #[test]
    fn mmap_pool_matches_heap_pool_encoding() {
        let pool_off = 16usize;
        let mut buf = vec![0u8; 256];
        let mut mp = MmapPool::new(pool_off, 4);
        let a = mp.intern(&mut buf, b"Library");
        let b = mp.intern(&mut buf, b"Caches");
        assert_eq!(mp.intern(&mut buf, b"Library"), a);

        let names = &buf[pool_off..mp.cursor()];
        assert_eq!(pool_str(names, a), "Library");
        assert_eq!(pool_str(names, b), "Caches");
        assert_eq!(mp.name_len(), names.len());
    }
}
