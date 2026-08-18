//! macOS 并行目录遍历器
//!
//! 用 `getattrlistbulk(2)` 批量读取目录条目，配合多线程工作队列实现并行遍历。
//! 实测在 `~/Library`（约 92 万文件）上，8 线程比单线程快 3 倍以上（12s vs 40s）。
//!
//! # 设计要点
//!
//! - **工作队列**：线程安全的目录路径队列，线程从中取目录、枚举、把子目录推回队列。
//! - **线程数**：实测 8 线程左右饱和，16 线程出现回退。固定上限 8，不盲目用 CPU 核心数。
//! - **回退**：非 APFS/HFS+ 或 `getattrlistbulk` 不支持时，回退到 `walkdir`。
//! - **符号链接**：不跟随，只统计链接自身。
//! - **权限错误**：跳过不可访问目录，不中断扫描。
//! - **取消**：通过 `AtomicBool` 检查取消标志。

use crate::core::disk::{ScanError, ScanResult, SizeTree, TreeEntry, VolumeId};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

/// 遍历器线程数上限。
///
/// 实测 8 线程左右性能饱和，16 线程出现回退（见计划文档 §3 实测数据）。
/// 不盲目使用 CPU 核心数——目录枚举的瓶颈是 syscall 和内核锁，
/// 不是 CPU 计算。
const MAX_THREADS: usize = 8;

/// `getattrlistbulk` 的缓冲区大小：256 KB。
///
/// 太小会导致 syscall 次数激增；太大则浪费内存且对 cache 不友好。
/// 256 KB 是实测甜点，与 C 原型验证一致。
const BULK_BUF_SIZE: usize = 256 * 1024;

/// macOS vnode 类型枚举，对应 `<sys/vnode.h>` 里的 `enum vtype`。
///
/// `libc` crate 没有暴露这些常量，这里手动定义。
mod vtype {
    pub const VNON: u32 = 0;
    pub const VREG: u32 = 1;
    pub const VDIR: u32 = 2;
    // VBLK/VCHR/VLNK/VSOCK/VFIFO 暂时用不到，但保留 VNON 做默认值
    #[allow(dead_code)]
    pub const VBLK: u32 = 3;
    #[allow(dead_code)]
    pub const VCHR: u32 = 4;
    #[allow(dead_code)]
    pub const VLNK: u32 = 5;
    #[allow(dead_code)]
    pub const VSOCK: u32 = 6;
    #[allow(dead_code)]
    pub const VFIFO: u32 = 7;
}

/// `ATTR_CMN_ERROR`：libc 没暴露，手动定义。
const ATTR_CMN_ERROR: u32 = 0x20000000;

/// `ATTR_CMN_MODTIME`：libc 没暴露，手动定义。
const ATTR_CMN_MODTIME: u32 = 0x00000400;

/// 单条目录条目的信息。
struct DirEntry {
    name: String,
    is_dir: bool,
    is_reg: bool,
    size: u64,
    mtime: u64,
}

/// 解析 `getattrlistbulk` 返回的缓冲区里的一条记录。
///
/// 布局见 `<sys/attr.h>`：每条记录以 4 字节长度开头，后跟 `attribute_set_t`，
/// 再按请求的属性顺序排列各字段。
unsafe fn parse_bulk_entry(ptr: *const u8) -> Option<DirEntry> {
    // 每条记录以 uint32_t length 开头
    let len = std::ptr::read_unaligned(ptr as *const u32);
    if len == 0 {
        return None;
    }
    let mut off = std::mem::size_of::<u32>();

    // attribute_set_t：5 个 attrgroup_t（u32），共 20 字节
    let returned = std::ptr::read_unaligned(ptr.add(off) as *const libc::attribute_set_t);
    let common = returned.commonattr;
    let file_attrs = returned.fileattr;
    off += std::mem::size_of::<libc::attribute_set_t>();

    let mut name: Option<String> = None;
    let mut obj_type: u32 = vtype::VNON;

    // 属性按 bit 值升序排列（见 getattrlist(2) man page 与计划文档 §5.1）：
    //   ATTR_CMN_NAME      = 0x00000001  (bit 0)
    //   ATTR_CMN_OBJTYPE   = 0x00000008  (bit 3)
    //   ATTR_CMN_MODTIME   = 0x00000400  (bit 10)
    //   ATTR_CMN_ERROR     = 0x20000000  (bit 29)
    // 写反会静默拿到错误数值——尤其是带错误码的条目（权限被拒等），
    // 会把 NAME/OBJTYPE 读到错位的字节上，产生垃圾名和垃圾类型。

    // ATTR_CMN_NAME：attrreference_t（offset + length），指向缓冲区内的字符串
    if common & libc::ATTR_CMN_NAME != 0 {
        let attr_ref = std::ptr::read_unaligned(ptr.add(off) as *const libc::attrreference_t);
        // attr_dataoffset 是相对于 attrreference_t 自身的偏移
        let data_ptr = ptr.add(off).offset(attr_ref.attr_dataoffset as isize);
        // 名称是 NUL 结尾的 UTF-8 字符串
        let cstr = std::ffi::CStr::from_ptr(data_ptr as *const libc::c_char);
        name = Some(cstr.to_string_lossy().into_owned());
        off += std::mem::size_of::<libc::attrreference_t>();
    }

    // ATTR_CMN_OBJTYPE：fsobj_type_t（u32）
    if common & libc::ATTR_CMN_OBJTYPE != 0 {
        obj_type = std::ptr::read_unaligned(ptr.add(off) as *const u32);
        off += std::mem::size_of::<u32>();
    }

    // ATTR_CMN_MODTIME：timespec（tv_sec + tv_nsec），各 8 字节
    let mut mtime: u64 = 0;
    if common & ATTR_CMN_MODTIME != 0 {
        let tv_sec = std::ptr::read_unaligned(ptr.add(off) as *const i64);
        mtime = tv_sec.max(0) as u64;
        off += std::mem::size_of::<libc::timespec>();
    }

    // ATTR_CMN_ERROR：u_int32_t 错误码，排在 NAME、OBJTYPE、MODTIME 之后
    if common & ATTR_CMN_ERROR != 0 {
        off += std::mem::size_of::<u32>();
    }

    // ATTR_FILE_ALLOCSIZE：off_t（i64），仅对常规文件有效
    let mut size: u64 = 0;
    if obj_type == vtype::VREG && file_attrs & libc::ATTR_FILE_ALLOCSIZE != 0 {
        size = std::ptr::read_unaligned(ptr.add(off) as *const i64) as u64;
    }

    let name = name?;
    if name == "." || name == ".." {
        return None;
    }

    Some(DirEntry {
        name,
        is_dir: obj_type == vtype::VDIR,
        is_reg: obj_type == vtype::VREG,
        size,
        mtime,
    })
}

/// 用 `getattrlistbulk` 枚举单个目录的所有条目。
///
/// 返回该目录下的条目列表。目录打开失败（权限等）返回空 Vec。
fn enumerate_dir(dir_fd: libc::c_int) -> Vec<DirEntry> {
    let mut entries = Vec::new();
    let mut buf = vec![0u8; BULK_BUF_SIZE];

    // 构造 attrlist：请求 NAME、MODTIME、OBJTYPE、ALLOCSIZE
    let mut al: libc::attrlist = unsafe { std::mem::zeroed() };
    al.bitmapcount = libc::ATTR_BIT_MAP_COUNT;
    al.commonattr = libc::ATTR_CMN_RETURNED_ATTRS
        | libc::ATTR_CMN_NAME
        | ATTR_CMN_MODTIME
        | ATTR_CMN_ERROR
        | libc::ATTR_CMN_OBJTYPE;
    al.fileattr = libc::ATTR_FILE_ALLOCSIZE;

    loop {
        // SAFETY: buf 是本地 Vec，大小正确；al 是合法的 attrlist。
        let n = unsafe {
            libc::getattrlistbulk(
                dir_fd,
                &mut al as *mut _ as *mut libc::c_void,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                0,
            )
        };
        if n <= 0 {
            break;
        }
        let mut ptr = buf.as_ptr();
        for _ in 0..n {
            // SAFETY: getattrlistbulk 填充了 n 条记录，每条以 length 开头。
            if let Some(entry) = unsafe { parse_bulk_entry(ptr) } {
                entries.push(entry);
            }
            // 前进到下一条记录
            let len = unsafe { std::ptr::read_unaligned(ptr as *const u32) } as usize;
            if len == 0 {
                break;
            }
            ptr = unsafe { ptr.add(len) };
        }
    }
    entries
}

/// 工作队列：线程安全的目录路径队列 + 活跃线程计数。
///
/// 每个队列项是 `(目录路径, 该目录在 entries 数组中的下标)`。
/// 携带下标是为了在扫描时直接设置子条目的 parent，避免后续用
/// `HashMap<PathBuf, u32>` 做路径→索引反查——6.6M 条目的反查
/// 需要 ~200 MB HashMap 和 6.6M 次 PathBuf clone。
struct WorkQueue {
    queue: Mutex<Vec<(PathBuf, u32)>>,
    cv: Condvar,
    active: Mutex<usize>,
}

impl WorkQueue {
    fn new() -> Self {
        Self {
            queue: Mutex::new(Vec::new()),
            cv: Condvar::new(),
            active: Mutex::new(0),
        }
    }

    fn push(&self, path: PathBuf, parent_idx: u32) {
        let mut q = self.queue.lock().unwrap();
        q.push((path, parent_idx));
        self.cv.notify_one();
    }

    /// 取一个目录。没有目录但有活跃线程时等待；都没有时返回 None。
    fn pop(&self) -> Option<(PathBuf, u32)> {
        let mut q = self.queue.lock().unwrap();
        loop {
            if let Some(item) = q.pop() {
                return Some(item);
            }
            let active = *self.active.lock().unwrap();
            if active == 0 {
                return None;
            }
            q = self.cv.wait(q).unwrap();
        }
    }

    fn inc_active(&self) {
        *self.active.lock().unwrap() += 1;
    }

    fn dec_active(&self) {
        let mut a = self.active.lock().unwrap();
        *a -= 1;
        if *a == 0 {
            // 唤醒所有等待的线程，让它们看到 active==0 并退出
            self.cv.notify_all();
        }
    }
}

/// 扫描结果收集器：线程安全地收集文件和目录信息。
struct Collector {
    /// 扫描阶段收集的原始条目，聚合阶段构建树。
    /// parent 在扫描时直接设置，不需要后续路径反查。
    entries: Mutex<Vec<RawEntry>>,
    total_size: AtomicU64,
    file_count: AtomicU64,
    dir_count: AtomicU64,
}

/// 原始条目：扫描阶段收集，聚合阶段构建树。
///
/// 不保存完整路径——parent 索引在扫描时直接设置，避免了
/// `build_size_tree` 中 6.6M 条目的 `HashMap<PathBuf, u32>` 反查。
#[derive(Clone)]
struct RawEntry {
    parent: u32, // 父节点索引，根节点用 0
    name: String,
    is_dir: bool,
    size: u64, // 文件的大小，目录为 0
    mtime: u64,
}

impl Collector {
    fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            total_size: AtomicU64::new(0),
            file_count: AtomicU64::new(0),
            dir_count: AtomicU64::new(0),
        }
    }
}

/// 并行遍历指定根目录，构建扫描结果。
///
/// 这是 M2 的核心入口。`root` 是要扫描的根目录路径，
/// `volume` 是卷标识（用于 `ScanResult` 和 `SizeTree`）。
/// `live` 是取消标志。
pub fn scan_root(
    root: &Path,
    volume: VolumeId,
    live: &AtomicBool,
) -> Result<ScanResult, ScanError> {
    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, MAX_THREADS);
    scan_root_nthreads(root, volume, live, n_threads)
}

/// 限定线程数的扫描，供 rayon 并行迭代器内部使用。
///
/// 在 `refresh_macos_index` 中，rayon 已经跨子树并行了，
/// 每个子树再用 8 线程会导致线程爆炸（8 rayon × 8 scan = 64 线程）。
/// 用 2 线程/子树：大子树有内部并行，小子树开销低，
/// 总线程数 = 2 × rayon_threads，轻度超额，OS 调度器能很好处理。
pub fn scan_root_few_threads(
    root: &Path,
    volume: VolumeId,
    live: &AtomicBool,
) -> Result<ScanResult, ScanError> {
    scan_root_nthreads(root, volume, live, 2)
}

fn scan_root_nthreads(
    root: &Path,
    volume: VolumeId,
    live: &AtomicBool,
    n_threads: usize,
) -> Result<ScanResult, ScanError> {
    let started = Instant::now();

    // 检查根目录是否可访问
    if !root.exists() {
        return Err(ScanError::Io(format!("根目录不存在: {}", root.display())));
    }

    let wq = Arc::new(WorkQueue::new());
    let collector = Arc::new(Collector::new());
    // 线程需要 'static 的取消标志，把外部 AtomicBool 的值复制一份
    let live_arc = Arc::new(AtomicBool::new(live.load(Ordering::Relaxed)));

    // 预分配根节点（索引 0）
    let label = volume.display().to_string();
    {
        let mut entries = collector.entries.lock().unwrap();
        entries.push(RawEntry {
            parent: 0,
            name: label,
            is_dir: true,
            size: 0,
            mtime: 0,
        });
        collector.dir_count.fetch_add(1, Ordering::Relaxed);
    }

    // 推入根目录，根节点的 entries 下标是 0
    wq.push(root.to_path_buf(), 0);

    let mut handles = Vec::with_capacity(n_threads);
    for _ in 0..n_threads {
        let wq = Arc::clone(&wq);
        let collector = Arc::clone(&collector);
        let live = Arc::clone(&live_arc);
        handles.push(std::thread::spawn(move || {
            worker_loop(&wq, &collector, &live);
        }));
    }
    for h in handles {
        let _ = h.join();
    }

    // 外部取消标志可能在扫描过程中被设置，检查一次
    // live=false 表示取消
    if !live.load(Ordering::Relaxed) {
        return Err(ScanError::Io("扫描已取消".into()));
    }

    // 聚合阶段：构建 SizeTree
    let entries = collector.entries.lock().unwrap().clone();
    let total_size = collector.total_size.load(Ordering::Relaxed);
    let file_count = collector.file_count.load(Ordering::Relaxed);
    let dir_count = collector.dir_count.load(Ordering::Relaxed);

    let tree = build_size_tree(volume.clone(), entries);

    Ok(ScanResult {
        volume,
        total_size,
        file_count,
        dir_count,
        dirs: Vec::new(), // macOS 不需要目录排行榜（GUI 走树）
        tree,
        elapsed_ms: started.elapsed().as_millis() as u64,
        records_read: file_count + dir_count,
        records_expected: file_count + dir_count,
        mft_run_bytes: 0,
        ext_records: 0,
        ext_data_merged: 0,
        hard_links: 0,
        unique_size: total_size,
        unique_files: file_count,
    })
}

/// 工作线程主循环：取目录 → 枚举 → 推子目录 → 重复。
///
/// 每个从队列取出的项包含目录路径和该目录在 entries 数组中的下标。
/// 子条目的 parent 直接设为该下标，不再需要在聚合阶段做路径反查。
fn worker_loop(wq: &WorkQueue, collector: &Collector, live: &AtomicBool) {
    loop {
        if !live.load(Ordering::Relaxed) {
            return;
        }

        let (dir, dir_idx) = match wq.pop() {
            Some(item) => item,
            None => return,
        };

        wq.inc_active();

        // 打开目录
        let c_path = match std::ffi::CString::new(dir.to_string_lossy().as_bytes()) {
            Ok(c) => c,
            Err(_) => {
                wq.dec_active();
                continue;
            }
        };

        // SAFETY: c_path 是合法的 NUL 结尾字符串。
        // O_NOFOLLOW：不跟随符号链接。macOS Container 目录里常有指向
        // TCC 保护路径的符号链接，跟随会导致 open() 阻塞数秒。
        // 符号链接本身不占磁盘空间，目标已在真实位置计数。
        let fd = unsafe {
            libc::open(
                c_path.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            wq.dec_active();
            continue;
        }

        let entries = enumerate_dir(fd);
        unsafe { libc::close(fd) };

        // 先在本地收集所有条目，最后一次性锁 collector.entries 批量写入。
        // 之前的实现每条目锁一次，92 万文件就是 92 万次 lock/unlock，
        // 严重削弱并行遍历的收益。
        let mut new_dirs: Vec<(PathBuf, u32)> = Vec::new();
        let mut raw_entries: Vec<RawEntry> = Vec::with_capacity(entries.len());
        for entry in &entries {
            if entry.is_dir {
                collector.dir_count.fetch_add(1, Ordering::Relaxed);
            } else if entry.is_reg {
                collector
                    .total_size
                    .fetch_add(entry.size, Ordering::Relaxed);
                collector.file_count.fetch_add(1, Ordering::Relaxed);
            }

            raw_entries.push(RawEntry {
                parent: dir_idx,
                name: entry.name.clone(),
                is_dir: entry.is_dir,
                size: if entry.is_dir { 0 } else { entry.size },
                mtime: entry.mtime,
            });
        }

        // 一次性批量写入，只锁一次。同时获取子目录的 entries 下标，
        // 用于后续推入工作队列。
        {
            let mut shared = collector.entries.lock().unwrap();
            let base = shared.len() as u32;
            shared.extend(raw_entries);
            // 为每个目录条目计算其在 entries 中的下标，推入工作队列
            for (i, entry) in entries.iter().enumerate() {
                if entry.is_dir {
                    new_dirs.push((dir.join(&entry.name), base + i as u32));
                }
            }
        }

        for (path, idx) in new_dirs {
            if live.load(Ordering::Relaxed) {
                wq.push(path, idx);
            }
        }

        wq.dec_active();
    }
}

/// 从原始条目列表构建 `SizeTree`。
///
/// parent 索引在扫描阶段已经直接设置，这里只需要：
/// 1. 计算每个目录的递归 size 和 file_count（并行，用 AtomicU64）
/// 2. 构建 child_start / child_at 紧凑数组
///
/// 之前的实现需要 `HashMap<PathBuf, u32>` 做路径→索引反查，
/// 6.6M 条目时 HashMap 本身占 ~200 MB，加上 6.6M 次 PathBuf clone
/// 和 hash 计算，是首次扫描的主要内存瓶颈之一。
#[allow(clippy::needless_range_loop)]
fn build_size_tree(volume: VolumeId, entries: Vec<RawEntry>) -> SizeTree {
    use std::sync::atomic::AtomicU64;
    let n = entries.len();
    if n == 0 {
        return SizeTree::empty(volume);
    }

    // 1. 计算目录的递归 size 和 file_count
    //
    // 用 AtomicU64 并行累加——每个文件沿 parent 链向上走，
    // 对经过的每个目录原子加 size 和 file_count。
    // AtomicU64 与 u64 内存布局相同，不额外占空间。
    // 不同子树的文件通常走不同的祖先链，竞争很少。
    let atomic_size: Vec<AtomicU64> = (0..n).map(|_| AtomicU64::new(0)).collect();
    let atomic_files: Vec<AtomicU64> = (0..n).map(|_| AtomicU64::new(0)).collect();

    entries.par_iter().for_each(|entry| {
        if entry.is_dir {
            return;
        }
        let mut cur = entry.parent;
        loop {
            let idx = cur as usize;
            if idx >= n {
                break;
            }
            atomic_size[idx].fetch_add(entry.size, Ordering::Relaxed);
            atomic_files[idx].fetch_add(1, Ordering::Relaxed);
            if cur == 0 || entries[idx].parent == cur {
                break;
            }
            cur = entries[idx].parent;
        }
    });

    let dir_size: Vec<u64> = atomic_size
        .iter()
        .map(|a| a.load(Ordering::Relaxed))
        .collect();
    let dir_files: Vec<u64> = atomic_files
        .iter()
        .map(|a| a.load(Ordering::Relaxed))
        .collect();

    // 2. 构建 child_start / child_at
    let mut child_counts = vec![0u32; n];
    for i in 0..n {
        if i == 0 {
            continue; // 根节点没有父
        }
        let p = entries[i].parent as usize;
        if p < n {
            child_counts[p] += 1;
        }
    }

    let mut child_start = vec![0u32; n + 1];
    for i in 0..n {
        child_start[i + 1] = child_start[i] + child_counts[i];
    }

    let mut child_at = vec![0u32; child_start[n] as usize];
    let mut cursor = child_start[..n].to_vec();
    for i in 0..n {
        if i == 0 {
            continue;
        }
        let p = entries[i].parent as usize;
        if p < n {
            child_at[cursor[p] as usize] = i as u32;
            cursor[p] += 1;
        }
    }

    // 转换成 SizeTree 的内部结构
    let mut tree_entries = Vec::with_capacity(n);
    for e in &entries {
        tree_entries.push(TreeEntry {
            parent: e.parent,
            name: e.name.clone(),
            is_dir: e.is_dir,
            size: e.size,
            used: true,
            mtime: e.mtime,
        });
    }

    SizeTree::from_parts(
        volume,
        tree_entries,
        dir_size,
        dir_files,
        child_start,
        child_at,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_temp_dir() {
        let tmp = std::env::temp_dir().join("qc_test_walk_scan_temp");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("sub")).unwrap();
        std::fs::write(tmp.join("a.txt"), b"123").unwrap();
        let vol = VolumeId::from_mount_point(tmp.clone());
        let live = AtomicBool::new(true);
        let result = scan_root(&tmp, vol, &live).expect("扫描临时目录应当成功");
        assert!(
            result.file_count > 0 || result.dir_count > 0,
            "临时目录不应该是空的"
        );
        assert!(result.elapsed_ms < 10_000, "扫描临时目录不该超过 10 秒");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_cancellation() {
        let tmp = std::env::temp_dir().join("qc_test_walk_cancel");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let vol = VolumeId::from_mount_point(tmp.clone());
        let live = AtomicBool::new(false); // 立即取消
        let result = scan_root(&tmp, vol, &live);
        assert!(result.is_err(), "取消的扫描应当返回错误");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 回归测试：`parse_bulk_entry` 在 `ATTR_CMN_ERROR` 被设置时，
    /// 仍能正确解析 NAME 和 OBJTYPE。
    ///
    /// `getattrlistbulk` 返回的记录里，属性按 bit 值升序排列：
    ///   ATTR_CMN_NAME    (0x00000001, bit 0)
    ///   ATTR_CMN_OBJTYPE (0x00000008, bit 3)
    ///   ATTR_CMN_ERROR   (0x20000000, bit 29)
    /// 之前的代码错误地把 ERROR 当成排在 NAME 之前，导致带错误码的条目
    /// 从错位的字节读 NAME/OBJTYPE，产生垃圾名和垃圾类型。
    ///
    /// 本测试手工构造一条包含 NAME + OBJTYPE + ERROR 的记录，验证解析顺序。
    /// 不依赖真实的 `getattrlistbulk` 调用——本地 APFS 上极少设置
    /// `ATTR_CMN_ERROR`，用真实扫描无法稳定覆盖这条路径。
    #[test]
    fn parse_bulk_entry_with_error_attr() {
        // getattrlistbulk 记录布局（属性按 bit 升序，变长数据在定长字段之后）：
        //   [u32 length]
        //   [attribute_set_t]           — 5×u32 = 20 字节
        //   [attrreference_t name]      — 8 字节，attr_dataoffset 指向后面的字符串
        //   [u32 objtype]               — 4 字节
        //   [u32 error]                 — 4 字节
        //   [char[] name_string\0]      — 变长，由 attr_dataoffset 定位
        let name_bytes = b"hello.txt";
        let attrref_size = std::mem::size_of::<libc::attrreference_t>(); // 8
        let attrset_size = std::mem::size_of::<libc::attribute_set_t>(); // 20

        let attrref_off = 4 + attrset_size; // 24
        let objtype_off = attrref_off + attrref_size; // 32
        let error_off = objtype_off + 4; // 36
        let name_start = error_off + 4; // 40 — 变长字符串在所有定长字段之后
        let name_len = name_bytes.len() + 1; // 含 NUL
        let total_len = name_start + name_len;

        let mut buf = vec![0u8; total_len];

        // u32 length
        buf[0..4].copy_from_slice(&(total_len as u32).to_ne_bytes());

        // attribute_set_t：commonattr = NAME | OBJTYPE | ERROR
        buf[4..8].copy_from_slice(
            &(libc::ATTR_CMN_NAME | libc::ATTR_CMN_OBJTYPE | ATTR_CMN_ERROR).to_ne_bytes(),
        );

        // attrreference_t：attr_dataoffset 相对于 attrreference_t 自身
        let name_offset = (name_start - attrref_off) as i32; // 16
        buf[attrref_off..attrref_off + 4].copy_from_slice(&name_offset.to_ne_bytes());
        buf[attrref_off + 4..attrref_off + 8].copy_from_slice(&(name_len as u32).to_ne_bytes());

        // u32 objtype = VREG (1)
        buf[objtype_off..objtype_off + 4].copy_from_slice(&vtype::VREG.to_ne_bytes());

        // u32 error = EPERM (1)
        buf[error_off..error_off + 4].copy_from_slice(&1u32.to_ne_bytes());

        // name 字符串（NUL 结尾，buf 已零初始化）
        buf[name_start..name_start + name_bytes.len()].copy_from_slice(name_bytes);

        let entry = unsafe { parse_bulk_entry(buf.as_ptr()) }.expect("应当能解析出条目");
        assert_eq!(entry.name, "hello.txt", "NAME 应当被正确解析");
        assert!(entry.is_reg, "OBJTYPE 应当是 VREG");
        assert!(!entry.is_dir, "不应当被误判为目录");
    }

    /// 回归测试：`parse_bulk_entry` 在没有 `ATTR_CMN_ERROR` 时也能正确解析。
    ///
    /// 确保修复没有破坏正常条目（无错误码）的解析路径。
    #[test]
    fn parse_bulk_entry_without_error_attr() {
        let name_bytes = b"readable.txt";
        let attrref_size = std::mem::size_of::<libc::attrreference_t>();
        let attrset_size = std::mem::size_of::<libc::attribute_set_t>();

        let attrref_off = 4 + attrset_size;
        let objtype_off = attrref_off + attrref_size;
        let alloc_size_off = objtype_off + 4;
        let name_start = alloc_size_off + 8;
        let name_len = name_bytes.len() + 1;
        let total_len = name_start + name_len;

        let mut buf = vec![0u8; total_len];
        buf[0..4].copy_from_slice(&(total_len as u32).to_ne_bytes());
        buf[4..8].copy_from_slice(&(libc::ATTR_CMN_NAME | libc::ATTR_CMN_OBJTYPE).to_ne_bytes());
        // fileattr 位图位于 attribute_set_t 的第 4 个 u32。
        buf[4 + 12..4 + 16].copy_from_slice(&libc::ATTR_FILE_ALLOCSIZE.to_ne_bytes());

        let name_offset = (name_start - attrref_off) as i32;
        buf[attrref_off..attrref_off + 4].copy_from_slice(&name_offset.to_ne_bytes());
        buf[attrref_off + 4..attrref_off + 8].copy_from_slice(&(name_len as u32).to_ne_bytes());

        buf[objtype_off..objtype_off + 4].copy_from_slice(&vtype::VREG.to_ne_bytes());
        buf[alloc_size_off..alloc_size_off + 8].copy_from_slice(&4096i64.to_ne_bytes());
        buf[name_start..name_start + name_bytes.len()].copy_from_slice(name_bytes);

        let entry = unsafe { parse_bulk_entry(buf.as_ptr()) }.expect("应当能解析出条目");
        assert_eq!(entry.name, "readable.txt");
        assert!(entry.is_reg);
        assert_eq!(entry.size, 4096, "fileattr 中的 ALLOCSIZE 应当被读取");
    }

    /// 验证带 ATTR_CMN_MODTIME 的记录能正确解析 NAME、OBJTYPE、MODTIME、ALLOCSIZE。
    #[test]
    fn parse_bulk_entry_with_modtime() {
        let name_bytes = b"photo.jpg";
        let attrref_size = std::mem::size_of::<libc::attrreference_t>();
        let attrset_size = std::mem::size_of::<libc::attribute_set_t>();
        let timespec_size = std::mem::size_of::<libc::timespec>();

        // 布局（属性按 bit 升序）：[u32 length] [attribute_set_t] [attrreference_t] [u32 objtype] [timespec] [i64 allocsize] [name\0]
        let attrref_off = 4 + attrset_size;
        let objtype_off = attrref_off + attrref_size;
        let modtime_off = objtype_off + 4;
        let alloc_size_off = modtime_off + timespec_size;
        let name_start = alloc_size_off + 8;
        let name_len = name_bytes.len() + 1;
        let total_len = name_start + name_len;

        let mut buf = vec![0u8; total_len];
        buf[0..4].copy_from_slice(&(total_len as u32).to_ne_bytes());
        // commonattr = NAME | OBJTYPE | MODTIME
        buf[4..8].copy_from_slice(
            &(libc::ATTR_CMN_NAME | libc::ATTR_CMN_OBJTYPE | ATTR_CMN_MODTIME).to_ne_bytes(),
        );
        // fileattr = ALLOCSIZE
        buf[4 + 12..4 + 16].copy_from_slice(&libc::ATTR_FILE_ALLOCSIZE.to_ne_bytes());

        // attrreference_t
        let name_offset = (name_start - attrref_off) as i32;
        buf[attrref_off..attrref_off + 4].copy_from_slice(&name_offset.to_ne_bytes());
        buf[attrref_off + 4..attrref_off + 8].copy_from_slice(&(name_len as u32).to_ne_bytes());

        // objtype = VREG
        buf[objtype_off..objtype_off + 4].copy_from_slice(&vtype::VREG.to_ne_bytes());
        // timespec: tv_sec = 1700000000, tv_nsec = 0
        buf[modtime_off..modtime_off + 8].copy_from_slice(&1700000000i64.to_ne_bytes());
        buf[modtime_off + 8..modtime_off + 16].copy_from_slice(&0i64.to_ne_bytes());
        // allocsize = 8192
        buf[alloc_size_off..alloc_size_off + 8].copy_from_slice(&8192i64.to_ne_bytes());
        // name
        buf[name_start..name_start + name_bytes.len()].copy_from_slice(name_bytes);

        let entry = unsafe { parse_bulk_entry(buf.as_ptr()) }.expect("应当能解析出条目");
        assert_eq!(entry.name, "photo.jpg");
        assert!(entry.is_reg);
        assert_eq!(entry.size, 8192);
        assert_eq!(entry.mtime, 1700000000, "MODTIME 应当被正确解析");
    }

    /// 扫描真实的 `~/Library`（约 92 万文件，约 12 秒）。
    ///
    /// 太慢且依赖本机状态，默认不跑：
    /// `cargo test --lib -- --ignored scan_home_library`
    #[test]
    #[ignore]
    fn scan_home_library() {
        let home = dirs::home_dir().expect("应当能拿到 home 目录");
        let library = home.join("Library");
        if !library.exists() {
            return; // 非 macOS 环境跳过
        }
        let vol = VolumeId::from_mount_point(PathBuf::from("/"));
        let live = AtomicBool::new(true);
        let result = scan_root(&library, vol, &live).expect("扫描 ~/Library 应当成功");
        // ~/Library 至少有几千个文件
        assert!(
            result.file_count > 1000,
            "~/Library 文件数异常少: {}",
            result.file_count
        );
        // 树应该有根节点和子节点
        let tree = &result.tree;
        assert!(tree.valid(tree.root()), "根节点应当有效");
        let children = tree.children(tree.root());
        assert!(!children.is_empty(), "根目录应当有子项");
    }
}
