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

use super::index_v7::{
    finalize_checksum, now_secs, MmapOut, MmapPool, NameInterner, V7Header, V7Layout,
};
use crate::core::disk::{ScanError, ScanResult, SizeTree, TreeEntry, VolumeId};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
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

/// 超过该条目数后，扫描原始数据转溢写临时文件，构建阶段流式写进
/// v7 文件映射。400k 条以内内存路径更省事（约 20MB 峰值）。
const SPILL_THRESHOLD: usize = 400_000;

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
/// 每个扫描项携带该目录在 entries 数组中的下标，以便直接设置 parent。
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

    fn push(&self, path: PathBuf, dir_idx: u32) {
        let mut q = self.queue.lock().unwrap();
        q.push((path, dir_idx));
        self.cv.notify_one();
    }

    /// 取一个任务。队列空但有活跃线程时等待；都没有时返回 None。
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
///
/// 小扫描（< [`SPILL_THRESHOLD`] 条）留在内存；超过阈值后整体转溢写：
/// 原始条目顺序追加进临时文件，构建阶段流式读出、直接写进 v7 文件映射。
/// 全量构建不再同时保留 RawEntry 数组和最终完整树——16M 条目时那两块
/// 加上 intern 表和 CSR 就是 1GB+ 的峰值，正是首次全量扫描 footprint
/// 超标的来源。
struct Collector {
    state: Mutex<CollectState>,
    total_size: AtomicU64,
    file_count: AtomicU64,
    dir_count: AtomicU64,
    /// 溢写一旦写失败（磁盘满等）置位，扫描以错误收场。
    failed: AtomicBool,
}

struct CollectState {
    entries: Vec<RawEntry>,
    names: Vec<u8>,
    spill: Option<SpillState>,
}

/// 内存模式缓冲：条目数组 + 名字池。
struct ScanBuf {
    entries: Vec<RawEntry>,
    names: Vec<u8>,
}

/// 溢写记录：`[parent u32][is_dir u8][mtime u32][size u64][name_len u16][name]`。
/// 顺序追加、顺序读回，构建阶段不需要随机访问。
struct SpillState {
    writer: std::io::BufWriter<std::fs::File>,
    path: PathBuf,
    n_entries: u64,
    /// 名字字节总量（含长度前缀），供流式构建预留名字池区域。
    name_bytes: u64,
}

impl SpillState {
    fn create() -> std::io::Result<Self> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "qc-spill-{}-{}.bin",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        Ok(Self {
            writer: std::io::BufWriter::with_capacity(1 << 20, file),
            path,
            n_entries: 0,
            name_bytes: 0,
        })
    }

    fn append(&mut self, r: &RawEntry, names: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        let name = &names[r.name_off as usize..r.name_off as usize + r.name_len as usize];
        self.writer.write_all(&r.parent.to_le_bytes())?;
        self.writer.write_all(&[u8::from(r.is_dir)])?;
        self.writer.write_all(&r.mtime.to_le_bytes())?;
        self.writer.write_all(&r.size.to_le_bytes())?;
        self.writer.write_all(&(name.len() as u16).to_le_bytes())?;
        self.writer.write_all(name)?;
        self.n_entries += 1;
        self.name_bytes += 2 + name.len() as u64;
        Ok(())
    }

    fn dump(&mut self, entries: &[RawEntry], names: &[u8]) -> std::io::Result<()> {
        for r in entries {
            self.append(r, names)?;
        }
        Ok(())
    }

    /// 结束溢写：flush 并交回流式构建所需的定位信息。
    fn finish(mut self) -> std::io::Result<(PathBuf, u64, u64)> {
        use std::io::Write;
        if let Err(error) = self.writer.flush() {
            let path = self.path.clone();
            drop(self.writer);
            let _ = std::fs::remove_file(path);
            return Err(error);
        }
        let n = self.n_entries;
        let nb = self.name_bytes;
        drop(self.writer);
        Ok((self.path, n, nb))
    }
}

/// 扫描结束后的原始数据：内存缓冲或溢写文件。
enum Collected {
    Mem(ScanBuf),
    Spill {
        path: PathBuf,
        n: u64,
        name_bytes: u64,
    },
}

impl Collector {
    fn new() -> Self {
        Self {
            state: Mutex::new(CollectState {
                entries: Vec::new(),
                names: Vec::new(),
                spill: None,
            }),
            total_size: AtomicU64::new(0),
            file_count: AtomicU64::new(0),
            dir_count: AtomicU64::new(0),
            failed: AtomicBool::new(false),
        }
    }

    /// 提交一批条目，返回本批首条目的全局下标。
    ///
    /// 批内 `raws` 的 name_off 是相对 `names` 的局部偏移；内存模式下这里
    /// 统一加上池基址，溢写模式下名字随记录一起落盘。
    fn commit(&self, raws: &mut Vec<RawEntry>, names: &[u8]) -> u32 {
        let mut st = self.state.lock().unwrap();
        if st.spill.is_none() && st.entries.len() + raws.len() > SPILL_THRESHOLD {
            match SpillState::create() {
                Ok(mut sp) => match sp.dump(&st.entries, &st.names) {
                    Ok(()) => {
                        crate::log!(
                            "扫描超过 {} 条，原始条目转溢写 {}",
                            SPILL_THRESHOLD,
                            sp.path.display()
                        );
                        st.spill = Some(sp);
                        st.entries = Vec::new();
                        st.names = Vec::new();
                    }
                    Err(e) => {
                        crate::log!("扫描溢写初始化失败（{}），继续走内存路径", e);
                        self.failed.store(true, Ordering::Relaxed);
                    }
                },
                Err(e) => {
                    crate::log!("创建扫描溢写文件失败（{}），继续走内存路径", e);
                }
            }
        }
        if let Some(sp) = st.spill.as_mut() {
            let base = sp.n_entries as u32;
            for r in raws.iter() {
                if let Err(e) = sp.append(r, names) {
                    crate::log!("扫描溢写失败（{}）", e);
                    self.failed.store(true, Ordering::Relaxed);
                    break;
                }
            }
            base
        } else {
            let names_base = st.names.len() as u32;
            st.names.extend_from_slice(names);
            let entry_base = st.entries.len() as u32;
            for r in raws.iter_mut() {
                r.name_off += names_base;
            }
            st.entries.append(raws);
            entry_base
        }
    }

    /// 扫描结束：取出溢写信息或内存缓冲。
    fn finish(self) -> std::io::Result<Collected> {
        let mut st = self.state.into_inner().unwrap_or_else(|e| e.into_inner());
        if let Some(sp) = st.spill.take() {
            let (path, n, name_bytes) = sp.finish()?;
            return Ok(Collected::Spill {
                path,
                n,
                name_bytes,
            });
        }
        Ok(Collected::Mem(ScanBuf {
            entries: st.entries,
            names: st.names,
        }))
    }
}

/// 原始条目：扫描阶段收集，聚合阶段构建树。
///
/// 名字写在 `ScanBuf::names` 连续池里，这里只存偏移，避免 1600 万个
/// `String` 堆分配。
struct RawEntry {
    parent: u32,
    name_off: u32,
    name_len: u16,
    is_dir: bool,
    size: u64,
    mtime: u32,
}

/// 并行遍历指定根目录，构建扫描结果。
///
/// 这是 M2 的核心入口。`root` 是要扫描的根目录路径，
/// `volume` 是卷标识（用于 `ScanResult` 和 `SizeTree`）。
/// `live` 是取消标志。
/// 数据卷在合成根下的镜像挂载点（firmlink 的另一端）。
const DATA_VOLUME_MIRROR: &str = "/System/Volumes/Data";

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
    scan_root_inner(root, volume, live, 2, None).map(|(scan, _)| scan)
}

/// 全量扫描并直接流式落盘到 `index_path`（大扫描零中间堆数组）。
///
/// 返回 `(scan, persisted)`：`persisted = true` 时索引文件已写好、
/// 树已挂在该文件的 mmap 上，调用方只需记录事件水位；`false` 时
/// （小扫描或流式构建失败回退）由调用方照常走 `save_index`。
pub fn scan_root_persisted(
    root: &Path,
    volume: VolumeId,
    live: &AtomicBool,
    index_path: &Path,
    last_event_id: u64,
) -> Result<(ScanResult, bool), ScanError> {
    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, MAX_THREADS);
    scan_root_inner(
        root,
        volume,
        live,
        n_threads,
        Some((index_path.to_path_buf(), last_event_id)),
    )
}

fn scan_root_nthreads(
    root: &Path,
    volume: VolumeId,
    live: &AtomicBool,
    n_threads: usize,
) -> Result<ScanResult, ScanError> {
    scan_root_inner(root, volume, live, n_threads, None).map(|(scan, _)| scan)
}

fn scan_root_inner(
    root: &Path,
    volume: VolumeId,
    live: &AtomicBool,
    n_threads: usize,
    persist: Option<(PathBuf, u64)>,
) -> Result<(ScanResult, bool), ScanError> {
    let started = Instant::now();

    // 检查根目录是否可访问
    if !root.exists() {
        return Err(ScanError::Io(format!("根目录不存在: {}", root.display())));
    }

    let wq = WorkQueue::new();
    let collector = Collector::new();

    // 预分配根节点（索引 0）
    let label = volume.display().to_string();
    {
        let mut st = collector.state.lock().unwrap();
        let name_off = st.names.len() as u32;
        st.names.extend_from_slice(label.as_bytes());
        st.entries.push(RawEntry {
            parent: 0,
            name_off,
            name_len: label.len() as u16,
            is_dir: true,
            size: 0,
            mtime: 0,
        });
        collector.dir_count.fetch_add(1, Ordering::Relaxed);
    }

    // 推入根目录，根节点的 entries 下标是 0
    wq.push(root.to_path_buf(), 0);

    // scoped threads：工作线程直接借用外部取消标志。之前把初始值复制进
    // 新的 AtomicBool，扫描开始后外部置 false 工作线程根本看不到，
    // 全盘扫描无法及时取消。
    std::thread::scope(|s| {
        for _ in 0..n_threads {
            let wq = &wq;
            let collector = &collector;
            s.spawn(move || worker_loop(wq, collector, live));
        }
    });

    let failed = collector.failed.load(Ordering::Relaxed);
    let cancelled = !live.load(Ordering::Relaxed);
    let total_size = collector.total_size.load(Ordering::Relaxed);
    let file_count = collector.file_count.load(Ordering::Relaxed);
    let dir_count = collector.dir_count.load(Ordering::Relaxed);
    let collected = collector
        .finish()
        .map_err(|error| ScanError::Io(format!("扫描溢写收尾失败: {error}")))?;
    if failed || cancelled {
        if let Collected::Spill { path, .. } = collected {
            let _ = std::fs::remove_file(path);
        }
        return Err(ScanError::Io(if cancelled {
            "扫描已取消".into()
        } else {
            "扫描临时文件写入失败".into()
        }));
    }

    let (tree, persisted) = match collected {
        Collected::Mem(buf) => (build_size_tree(volume.clone(), buf), false),
        Collected::Spill {
            path,
            n,
            name_bytes,
        } => {
            let build = StreamingBuild {
                spill_path: &path,
                n,
                name_bytes,
                persist: persist.as_ref().map(|(p, _)| p.as_path()),
                last_event_id: persist.as_ref().map(|(_, id)| *id).unwrap_or(0),
                live,
            };
            match build_size_tree_streaming(volume.clone(), build) {
                Ok(tree) => {
                    let _ = std::fs::remove_file(&path);
                    (tree, persist.is_some())
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                    let _ = std::fs::remove_file(&path);
                    return Err(ScanError::Io("扫描已取消".into()));
                }
                Err(e) => {
                    crate::log!("流式构建失败（{}），回退内存构建", e);
                    match read_spill_to_buf(&path, n, live) {
                        Ok(buf) => {
                            let _ = std::fs::remove_file(&path);
                            (build_size_tree(volume.clone(), buf), false)
                        }
                        Err(e2) => {
                            let _ = std::fs::remove_file(&path);
                            return Err(ScanError::Io(format!("读取扫描溢写失败: {e2}")));
                        }
                    }
                }
            }
        }
    };
    let records = tree.entry_count() as u64;
    crate::log!(
        "SizeTree 构建完成：{} 条，约 {:.1} MB（entries + names + CSR）",
        records,
        tree.memory_bytes() as f64 / (1024.0 * 1024.0)
    );

    Ok((
        ScanResult {
            volume,
            total_size,
            file_count,
            dir_count,
            dirs: Vec::new(), // macOS 不需要目录排行榜（GUI 走树）
            tree,
            elapsed_ms: started.elapsed().as_millis() as u64,
            records_read: records,
            records_expected: records,
            mft_run_bytes: 0,
            ext_records: 0,
            ext_data_merged: 0,
            hard_links: 0,
            unique_size: total_size,
            unique_files: file_count,
        },
        persisted,
    ))
}

/// 一批枚举结果的整理产物。
struct PreparedBatch {
    entries: Vec<RawEntry>,
    names: Vec<u8>,
    /// 待入队子目录（路径 + 在 `entries` 里的位置），调用方加上 commit
    /// 返回的 `entry_base` 才是全局下标。
    subdirs: Vec<(PathBuf, u32)>,
    dir_count: u64,
    file_count: u64,
    total_size: u64,
}

/// 把一个目录枚举出的一批条目整理成提交格式（纯函数，便于单测）。
///
/// 数据卷镜像在这层剪掉：不进条目、不进队列、不进统计——收录它等于
/// 把整棵用户树索引两遍。**子目录下标必须是它在 `entries` 里的实际
/// 位置**：剪掉条目后这个位置和 enumerate 的下标不再一致，用错会把
/// 整棵子树嫁接到无关条目上——父链出现"文件下挂目录"，加载校验
/// 拒绝，索引每次启动都被判无效而全量重建。
fn prepare_batch(dir: &Path, dir_idx: u32, entries: Vec<DirEntry>) -> PreparedBatch {
    let mut out = PreparedBatch {
        entries: Vec::with_capacity(entries.len()),
        names: Vec::new(),
        subdirs: Vec::new(),
        dir_count: 0,
        file_count: 0,
        total_size: 0,
    };
    for entry in entries {
        // 数据卷镜像整条剪掉：/ 是合成根，/Users、/Applications 等经
        // firmlink 已在顶层收录过一遍，/System/Volumes/Data 下是同一
        // 批文件的第二份入口。Preboot / VM / Update 是真正独立的内容，
        // 不受影响。旧索引里已存在的镜像子树由 load_index 的自愈移除。
        if entry.is_dir && dir.join(&entry.name).as_path() == Path::new(DATA_VOLUME_MIRROR) {
            continue;
        }
        if entry.is_dir {
            out.dir_count += 1;
        } else if entry.is_reg {
            out.total_size += entry.size;
            out.file_count += 1;
        }
        let name_off = out.names.len() as u32;
        out.names.extend_from_slice(entry.name.as_bytes());
        if entry.is_dir {
            out.subdirs
                .push((dir.join(&entry.name), out.entries.len() as u32));
        }
        out.entries.push(RawEntry {
            parent: dir_idx,
            name_off,
            name_len: entry.name.len() as u16,
            is_dir: entry.is_dir,
            size: if entry.is_dir { 0 } else { entry.size },
            mtime: entry.mtime.min(u32::MAX as u64) as u32,
        });
    }
    out
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

        let c_path = match std::ffi::CString::new(dir.to_string_lossy().as_bytes()) {
            Ok(c) => c,
            Err(_) => {
                wq.dec_active();
                continue;
            }
        };
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

        let batch = prepare_batch(&dir, dir_idx, entries);
        collector.dir_count.fetch_add(batch.dir_count, Ordering::Relaxed);
        collector
            .file_count
            .fetch_add(batch.file_count, Ordering::Relaxed);
        collector
            .total_size
            .fetch_add(batch.total_size, Ordering::Relaxed);

        // 提交批数据（内存或溢写），拿到本批首条目的全局下标
        let mut raw_entries = batch.entries;
        let local_names = batch.names;
        let entry_base = collector.commit(&mut raw_entries, &local_names);
        for (path, rel) in batch.subdirs {
            if live.load(Ordering::Relaxed) {
                wq.push(path, entry_base + rel);
            }
        }

        wq.dec_active();
    }
}

/// 从原始条目列表构建 `SizeTree`。
///
/// parent 索引在扫描阶段已经直接设置，这里 intern 名字、写入聚合、建 CSR。
fn build_size_tree(volume: VolumeId, buf: ScanBuf) -> SizeTree {
    let n = buf.entries.len();
    if n == 0 {
        return SizeTree::empty(volume);
    }

    let mut intern = NameInterner::with_capacity(n);
    let mut tree_entries = Vec::with_capacity(n);

    for e in &buf.entries {
        let start = e.name_off as usize;
        let end = start
            .saturating_add(e.name_len as usize)
            .min(buf.names.len());
        let name_off = intern.intern(&buf.names[start..end]);
        tree_entries.push(TreeEntry::new(
            e.parent,
            name_off,
            e.is_dir,
            e.size,
            e.mtime as u64,
            if e.is_dir { 0 } else { 1 },
        ));
    }
    let name_pool = intern.finish();
    drop(buf);

    SizeTree::from_packed(volume, name_pool, tree_entries)
}

/// 溢写记录的定长部分：parent + is_dir + mtime + size + name_len。
struct SpillRec {
    parent: u32,
    is_dir: bool,
    mtime: u32,
    size: u64,
}

/// 顺序读一条溢写记录，名字写入 `name_buf`。EOF 返回 None。
fn read_spill_rec(
    r: &mut impl std::io::Read,
    name_buf: &mut Vec<u8>,
) -> std::io::Result<Option<SpillRec>> {
    let mut hdr = [0u8; 19];
    match r.read_exact(&mut hdr) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u16::from_le_bytes([hdr[17], hdr[18]]) as usize;
    name_buf.clear();
    name_buf.resize(len, 0);
    r.read_exact(name_buf)?;
    Ok(Some(SpillRec {
        parent: u32::from_le_bytes(hdr[0..4].try_into().unwrap()),
        is_dir: hdr[4] != 0,
        mtime: u32::from_le_bytes(hdr[5..9].try_into().unwrap()),
        size: u64::from_le_bytes(hdr[9..17].try_into().unwrap()),
    }))
}

/// 把溢写文件读回内存缓冲（流式构建失败时的回退路径）。
fn read_spill_to_buf(spill_path: &Path, n: u64, live: &AtomicBool) -> std::io::Result<ScanBuf> {
    let file = std::fs::File::open(spill_path)?;
    let mut r = std::io::BufReader::with_capacity(1 << 20, file);
    let mut buf = ScanBuf {
        entries: Vec::with_capacity(n as usize),
        names: Vec::new(),
    };
    let mut name_buf = Vec::new();
    while let Some(rec) = read_spill_rec(&mut r, &mut name_buf)? {
        if buf.entries.len().is_multiple_of(4096) && !live.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "扫描已取消",
            ));
        }
        let name_off = buf.names.len() as u32;
        buf.names.extend_from_slice(&name_buf);
        buf.entries.push(RawEntry {
            parent: rec.parent,
            name_off,
            name_len: name_buf.len() as u16,
            is_dir: rec.is_dir,
            size: rec.size,
            mtime: rec.mtime,
        });
    }
    Ok(buf)
}

/// 流式构建的输出与统计参数。
struct StreamingBuild<'a> {
    spill_path: &'a Path,
    n: u64,
    name_bytes: u64,
    /// 给出时 rename 到该索引路径（全量重建直接落盘）
    persist: Option<&'a Path>,
    /// FSEvents 检查点，写进 v7 header。
    last_event_id: u64,
    live: &'a AtomicBool,
}

/// 从溢写文件流式构建 SizeTree，条目 / CSR / 名字池直接写进 v7 文件映射。
///
/// 全量构建的内存拐点：扫描阶段原始数据已溢写到磁盘，这里堆上只保留
/// 名字 intern 表和 CSR 前缀和两块暂存（16M 条目约 160MB），不再同时
/// 保留 RawEntry 数组和最终完整树。构建完成后：
/// - `persist` 给出 → rename 到索引路径（全量重建直接落盘）；
/// - 否则映射独立临时文件后立即删除（映射在 Unix 上继续有效）。
fn build_size_tree_streaming(
    volume: VolumeId,
    p: StreamingBuild<'_>,
) -> Result<SizeTree, std::io::Error> {
    let StreamingBuild {
        spill_path,
        n: n_u64,
        name_bytes,
        persist,
        last_event_id,
        live,
    } = p;
    let n = n_u64 as usize;
    if n == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "溢写为空",
        ));
    }
    let cancelled = || std::io::Error::new(std::io::ErrorKind::Interrupted, "扫描已取消");

    // 每趟读取都独立打开文件：try_clone 复制的 fd 与原句柄共享偏移，
    // 第一趟读到 EOF 后第二趟会立刻拿到空流。
    // Pass A0：每个节点的子节点数 → CSR 前缀和。
    let mut start = vec![0u32; n];
    {
        let f = std::fs::File::open(spill_path)?;
        let mut r = std::io::BufReader::with_capacity(1 << 20, f);
        let mut name_buf = Vec::new();
        for i in 0..n {
            if i % 4096 == 0 && !live.load(Ordering::Relaxed) {
                return Err(cancelled());
            }
            let rec = read_spill_rec(&mut r, &mut name_buf)?.ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "溢写提前结束")
            })?;
            if i > 0 {
                start[rec.parent as usize] += 1;
            }
        }
    }
    let mut ca_len = 0usize;
    for (i, s) in start.iter_mut().enumerate() {
        if i % 4096 == 0 && !live.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let c = *s;
        *s = ca_len as u32;
        ca_len += c as usize;
    }

    let mount = volume.mount_point().to_string_lossy().into_owned();
    let label = volume.display().to_string();
    let mount_b = mount.as_bytes();
    let label_b = label.as_bytes();
    let layout =
        V7Layout::names_trailing(n, mount_b.len(), label_b.len(), ca_len, name_bytes as usize);

    // 输出文件：持久化目标走 .tmp 再 rename；否则用独立临时文件
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let out_path = match persist {
        Some(p) => p.with_extension("bin.tmp"),
        None => std::env::temp_dir().join(format!(
            "qc-tree-{}-{}.bin",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )),
    };
    let mut out = MmapOut::create(out_path, layout.len)?;
    let ptr = out.ptr();
    let buf = out.as_mut_slice();

    // child_start 先填前缀和，Pass A 里当游标递增，结束后重写
    layout.write_child_start(buf, &start);

    // Pass A：intern 名字 + 写条目 + 填 child_at
    let mut pool = MmapPool::new(layout.name_off, n);
    {
        let f = std::fs::File::open(spill_path)?;
        let mut r = std::io::BufReader::with_capacity(1 << 20, f);
        let mut name_buf = Vec::new();
        for i in 0..n {
            if i % 4096 == 0 && !live.load(Ordering::Relaxed) {
                return Err(cancelled());
            }
            let rec = read_spill_rec(&mut r, &mut name_buf)?.ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "溢写提前结束")
            })?;
            let name_off = pool.intern(buf, &name_buf);
            TreeEntry::new(
                rec.parent,
                name_off,
                rec.is_dir,
                rec.size,
                rec.mtime as u64,
                if rec.is_dir { 0 } else { 1 },
            )
            .write_bytes_to(&mut buf[layout.entry_at(i)]);
            if i > 0 {
                layout.push_child(buf, rec.parent, i as u32);
            }
            pool.maybe_flush(ptr);
        }
    }

    // 重写 child_start（Pass A 里被当成游标改掉了）
    layout.write_child_start(buf, &start);
    drop(start);

    // 聚合传播：目录清零后把文件大小沿父链累加。全部直接操作映射页。
    for i in 0..n {
        if i % 4096 == 0 && !live.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let slot = layout.entry_at(i);
        let e = TreeEntry::from_bytes(&buf[slot.clone()]);
        if e.is_dir() && (e.size != 0 || e.file_count != 0) {
            e.with_totals(0, 0).write_bytes_to(&mut buf[slot]);
        }
    }
    for i in 0..n {
        if i % 4096 == 0 && !live.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let e = TreeEntry::from_bytes(&buf[layout.entry_at(i)]);
        if e.is_dir() {
            continue;
        }
        let (add_size, add_files) = (e.size, 1u32);
        let mut cur = e.parent();
        loop {
            let slot = layout.entry_at(cur as usize);
            let p = TreeEntry::from_bytes(&buf[slot.clone()]);
            p.with_totals(
                p.size.saturating_add(add_size),
                p.file_count.saturating_add(add_files),
            )
            .write_bytes_to(&mut buf[slot]);
            if cur == 0 {
                break;
            }
            cur = p.parent();
        }
    }

    // header + checksum + 收尾。
    // 文件/目录数和总大小以映射里的实际条目为准——扫描原子计数只算
    // 常规文件，而树把 symlink 等非目录节点也算作文件，必须一致才能
    // 通过加载校验。
    let mut hdr_files = 0u64;
    let mut hdr_dirs = 0u64;
    for i in 0..n {
        if i % 4096 == 0 && !live.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        if TreeEntry::from_bytes(&buf[layout.entry_at(i)]).is_dir() {
            hdr_dirs += 1;
        } else {
            hdr_files += 1;
        }
    }
    let root_total = TreeEntry::from_bytes(&buf[layout.entry_at(0)]).size;

    V7Header {
        layout: &layout,
        mount: mount_b,
        label: label_b,
        name_len: pool.name_len(),
        file_count: hdr_files,
        dir_count: hdr_dirs,
        total_size: root_total,
        last_event_id,
        scanned_at: now_secs(),
    }
    .write_into(buf);
    finalize_checksum(buf, pool.cursor());

    out.commit(pool.cursor())?;
    if let Some(target) = persist {
        out.rename_to(target)?;
    }
    let tree = SizeTree::from_mapped(volume, persist.unwrap_or(out.path()))
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "构建结果校验失败"))?;
    // 映射已在树里持有（persist 时文件也已 rename 走），`out` 的 Drop
    // 会把这个临时路径清掉。
    Ok(tree)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归：镜像剪枝后子目录队列下标必须与实际提交位置对齐。
    ///
    /// 旧实现镜像只从 raw_entries 里跳过，入队下标却用 enumerate 的
    /// 原始下标——剪掉一个，同批后续目录全部偏移，整棵子树嫁接到无关
    /// 条目上（文件下挂目录），索引校验拒载，每次启动都全量重建。
    #[test]
    fn prune_keeps_subdir_indices_aligned() {
        let dir = Path::new("/System/Volumes");
        let mk = |name: &str, is_dir: bool, size: u64| DirEntry {
            name: name.to_string(),
            is_dir,
            is_reg: !is_dir,
            size,
            mtime: 0,
        };
        // readdir 顺序任意；镜像放最前，后面跟同批目录与文件
        let entries = vec![
            mk("Data", true, 0), // 数据卷镜像，应剪掉
            mk("Preboot", true, 0),
            mk("VM", true, 0),
            mk("somefile", false, 123),
            mk("Update", true, 0),
        ];
        let batch = prepare_batch(dir, 7, entries);

        // 镜像不进条目、不进队列、不进统计
        assert_eq!(batch.entries.len(), 4);
        assert!(!batch.subdirs.iter().any(|(p, _)| p.ends_with("Data")));
        assert_eq!(batch.dir_count, 3);
        assert_eq!(batch.file_count, 1);
        assert_eq!(batch.total_size, 123);
        assert!(batch.entries.iter().all(|e| e.parent == 7));

        // 每个入队目录的下标 == 它在提交条目里的位置（含镜像之后的前移）
        for (path, idx) in &batch.subdirs {
            let name = path.file_name().unwrap().to_str().unwrap();
            let pos = batch
                .entries
                .iter()
                .position(|e| {
                    let s = String::from_utf8_lossy(
                        &batch.names[e.name_off as usize..e.name_off as usize + e.name_len as usize],
                    );
                    s == name
                })
                .expect("入队目录必须在条目里");
            assert_eq!(*idx, pos as u32, "{name} 的队列下标必须等于实际提交位置");
        }
        // VM 排在镜像之后，旧实现会把它的下标算成 2（错误），正确值是 1
        let vm = batch
            .subdirs
            .iter()
            .find(|(p, _)| p.ends_with("VM"))
            .unwrap();
        assert_eq!(vm.1, 1);
    }

    #[test]
    fn scan_indexes_nested_files() {
        let tmp = std::env::temp_dir().join(format!("{}_{}", "qc_test_walk_full_index", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let nm = tmp.join("node_modules").join("pkg");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("index.js"), vec![0u8; 4096]).unwrap();
        let vol = VolumeId::from_mount_point(tmp.clone());
        let live = AtomicBool::new(true);
        let result = scan_root(&tmp, vol, &live).expect("扫描应当成功");
        assert!(
            result
                .tree
                .find_node_by_path(&nm.join("index.js"))
                .is_some(),
            "全量索引必须包含 node_modules 内部文件，搜索才能命中"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_temp_dir() {
        let tmp = std::env::temp_dir().join(format!("{}_{}", "qc_test_walk_scan_temp", std::process::id()));
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
        let tmp = std::env::temp_dir().join(format!("{}_{}", "qc_test_walk_cancel", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let vol = VolumeId::from_mount_point(tmp.clone());
        let live = AtomicBool::new(false); // 立即取消
        let result = scan_root(&tmp, vol, &live);
        assert!(result.is_err(), "取消的扫描应当返回错误");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 扫描开始后外部取消必须及时生效。
    ///
    /// 回归背景：旧实现把外部 AtomicBool 的初始值复制进新的 Arc，
    /// 工作线程检查的是那份副本，扫描开始后外部置 false 根本看不到，
    /// 只能等整棵树扫完。现在工作线程通过 scoped threads 直接借用
    /// 外部标志，中途取消应在远小于完整扫描的时间内返回。
    #[test]
    fn scan_cancel_midway_returns_promptly() {
        let tmp = std::env::temp_dir().join(format!("{}_{}", "qc_test_walk_cancel_midway", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        // 生成 20000 个目录（各带一个文件），完整扫描明显超过取消时点
        for i in 0..20000 {
            let d = tmp.join(format!("d{i}"));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("f.txt"), b"x").unwrap();
        }
        let vol = VolumeId::from_mount_point(tmp.clone());
        let live = std::sync::Arc::new(AtomicBool::new(true));
        let started = Instant::now();
        {
            let live_for_scan = std::sync::Arc::clone(&live);
            let tmp_for_scan = tmp.clone();
            let handle = std::thread::spawn(move || scan_root(&tmp_for_scan, vol, &live_for_scan));
            std::thread::sleep(std::time::Duration::from_millis(50));
            live.store(false, Ordering::Relaxed);
            let result = handle.join().expect("扫描线程不应 panic");
            assert!(result.is_err(), "中途取消的扫描应当返回错误");
        }
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "取消后应当及时返回，实际耗时 {elapsed:?}"
        );
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

    /// 超过溢写阈值的大目录扫描：原始条目落临时文件，构建直接产出
    /// v7 映射。验证溢写 → 流式构建 → mmap 树整条管线，以及构建后
    /// 临时文件已清理。需要本机大目录，不进默认 CI：
    /// `cargo test --release --lib scan_large_dir_spills_to_mmap -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn scan_large_dir_spills_to_mmap() {
        let home = dirs::home_dir().expect("应当能拿到 home 目录");
        let root = home.join("Library");
        assert!(root.exists());
        let vol = VolumeId::from_mount_point(root.clone());
        let live = AtomicBool::new(true);
        let t0 = Instant::now();
        let result = scan_root(&root, vol, &live).expect("扫描应当成功");
        eprintln!(
            "scanned {} entries in {:?}, mapped base: {}, tree {:.1} MB",
            result.tree.entry_count(),
            t0.elapsed(),
            result.tree.has_mapped_base(),
            result.tree.memory_bytes() as f64 / (1024.0 * 1024.0)
        );
        if result.tree.entry_count() as usize >= SPILL_THRESHOLD {
            assert!(
                result.tree.has_mapped_base(),
                "超过溢写阈值的扫描应当产出 mmap 树"
            );
        }
        // 基本完整性：能按路径定位、聚合一致
        let probe = root.join("Preferences");
        if probe.exists() {
            assert!(result.tree.find_node_by_path(&probe).is_some());
        }
        assert!(result.tree.size_of(result.tree.root()) > 0);
        // 溢写临时文件不应残留
        let leftovers: Vec<_> = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("qc-spill-"))
            .collect();
        assert!(leftovers.is_empty(), "溢写临时文件残留: {leftovers:?}");
    }
}
