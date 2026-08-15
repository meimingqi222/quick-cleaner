use std::collections::HashMap;
use std::time::Instant;

use std::os::windows::ffi::OsStrExt;

const ROOT_RECORD: u32 = 5;
const MAX_DEPTH: usize = 256;
const CHUNK_BYTES: usize = 8 * 1024 * 1024;

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
#[derive(Clone)]
pub struct MftTree {
    volume: char,
    entries: Vec<Entry>,
    dir_size: Vec<u64>,
    dir_files: Vec<u64>,
    child_start: Vec<u32>,
    child_at: Vec<u32>,
}

impl std::fmt::Debug for MftTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MftTree({}: {} 条记录)", self.volume, self.entries.len())
    }
}

impl MftTree {
    pub fn volume(&self) -> char {
        self.volume
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

    pub fn name_of(&self, idx: u32) -> String {
        if idx == ROOT_RECORD {
            return format!("{}:", self.volume);
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
    /// 一屏目录）务必用 [`MftTree::path_of_with`] 复用同一个缓存，
    /// 否则每一行都要从头回溯到根。
    pub fn path_of(&self, idx: u32) -> String {
        let mut cache = HashMap::new();
        self.path_of_with(idx, &mut cache)
    }

    /// 复用调用方持有的缓存解析路径。同一批次里父链会被逐级记住。
    pub fn path_of_with(&self, idx: u32, cache: &mut HashMap<u32, String>) -> String {
        resolve_path(&self.entries, idx, self.volume, cache)
    }

    fn child_slice(&self, idx: u32) -> &[u32] {
        let i = idx as usize;
        if i + 1 >= self.child_start.len() {
            return &[];
        }
        let (a, b) = (self.child_start[i] as usize, self.child_start[i + 1] as usize);
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

    pub fn children(&self, idx: u32) -> Vec<Node> {
        let mut out: Vec<Node> = self
            .child_slice(idx)
            .iter()
            .filter(|&&c| {
                self.valid(c)
                    && !Self::is_ntfs_system_meta(c, &self.entries[c as usize].name)
            })
            .map(|&c| {
                let e = &self.entries[c as usize];
                Node {
                    idx: c,
                    name: e.name.clone(),
                    is_dir: e.is_dir,
                    size: if e.is_dir { self.dir_size[c as usize] } else { e.size },
                    file_count: if e.is_dir { self.dir_files[c as usize] } else { 1 },
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
            if heap.len() == n {
                if e.size <= heap.peek().map(|Reverse((s, _))| *s).unwrap_or(0) {
                    continue;
                }
            }
            if Self::is_ntfs_system_meta(i as u32, &e.name) {
                continue;
            }
            heap.push(Reverse((e.size, i as u32)));
            if heap.len() > n {
                heap.pop();
            }
        }

        let mut files: Vec<(u64, u32)> = heap.into_iter().map(|Reverse(v)| v).collect();
        files.sort_unstable_by(|a, b| b.0.cmp(&a.0));
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
            let hit = self.child_slice(cur).iter().copied().find(|&c| {
                self.valid(c) && self.entries[c as usize].name.eq_ignore_ascii_case(&comp_str)
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
        while cur != idx && self.valid(cur) && visited < 1000 {
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
pub enum MftError {
    AccessDenied,
    NotNtfs,
    Io(String),
}

impl std::fmt::Display for MftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MftError::AccessDenied => write!(f, "需要管理员权限才能读取 $MFT"),
            MftError::NotNtfs => write!(f, "该卷不是 NTFS 或无法获取卷信息"),
            MftError::Io(e) => write!(f, "读取失败：{e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Windows 原生结构与 FFI
// ---------------------------------------------------------------------------

const FSCTL_GET_NTFS_VOLUME_DATA: u32 = 0x0009_0064;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct NtfsVolumeData {
    volume_serial_number: i64,
    number_sectors: i64,
    total_clusters: i64,
    free_clusters: i64,
    total_reserved: i64,
    bytes_per_sector: u32,
    bytes_per_cluster: u32,
    bytes_per_file_record_segment: u32,
    clusters_per_file_record_segment: u32,
    mft_valid_data_length: i64,
    mft_start_lcn: i64,
    mft2_start_lcn: i64,
    mft_zone_start: i64,
    mft_zone_end: i64,
}

struct Volume {
    handle: winapi::um::winnt::HANDLE,
}

impl Drop for Volume {
    fn drop(&mut self) {
        unsafe { winapi::um::handleapi::CloseHandle(self.handle) };
    }
}

impl Volume {
    fn open(letter: char) -> Result<Self, MftError> {
        use winapi::um::fileapi::{CreateFileW, OPEN_EXISTING};
        use winapi::um::handleapi::INVALID_HANDLE_VALUE;
        use winapi::um::winnt::{
            FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ,
        };

        let path = format!("\\\\.\\{letter}:");
        let wide: Vec<u16> = std::ffi::OsStr::new(&path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            let err = unsafe { winapi::um::errhandlingapi::GetLastError() };
            return Err(if err == 5 {
                MftError::AccessDenied
            } else {
                MftError::Io(format!("CreateFileW 失败，错误码 {err}"))
            });
        }
        Ok(Volume { handle })
    }

    fn volume_data(&self) -> Result<NtfsVolumeData, MftError> {
        use winapi::um::ioapiset::DeviceIoControl;

        let mut data = NtfsVolumeData::default();
        let mut returned: u32 = 0;
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                FSCTL_GET_NTFS_VOLUME_DATA,
                std::ptr::null_mut(),
                0,
                &mut data as *mut _ as *mut winapi::ctypes::c_void,
                std::mem::size_of::<NtfsVolumeData>() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 || data.bytes_per_cluster == 0 || data.bytes_per_file_record_segment == 0 {
            return Err(MftError::NotNtfs);
        }
        Ok(data)
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize, MftError> {
        use winapi::um::fileapi::{ReadFile, SetFilePointerEx};

        let mut distance: winapi::um::winnt::LARGE_INTEGER = unsafe { std::mem::zeroed() };
        unsafe { *distance.QuadPart_mut() = offset as i64 };

        let ok = unsafe {
            SetFilePointerEx(
                self.handle,
                distance,
                std::ptr::null_mut(),
                0,
            )
        };
        if ok == 0 {
            let err = unsafe { winapi::um::errhandlingapi::GetLastError() };
            return Err(MftError::Io(format!("定位到 {offset} 失败，错误码 {err}")));
        }

        let mut read: u32 = 0;
        let ok = unsafe {
            ReadFile(
                self.handle,
                buf.as_mut_ptr() as *mut winapi::ctypes::c_void,
                buf.len() as u32,
                &mut read,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            let err = unsafe { winapi::um::errhandlingapi::GetLastError() };
            return Err(MftError::Io(format!("读取 {offset} 失败，错误码 {err}")));
        }
        Ok(read as usize)
    }
}

// ---------------------------------------------------------------------------
// 字节解析辅助
// ---------------------------------------------------------------------------

fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

fn u64_at(b: &[u8], off: usize) -> u64 {
    let mut v = [0u8; 8];
    v.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(v)
}

fn apply_fixup(rec: &mut [u8], bytes_per_sector: usize) -> bool {
    if rec.len() < 0x30 {
        return false;
    }
    let usa_off = u16_at(rec, 0x04) as usize;
    let usa_count = u16_at(rec, 0x06) as usize;
    if usa_count == 0 || usa_off + usa_count * 2 > rec.len() {
        return false;
    }

    let usn = u16_at(rec, usa_off);
    for i in 0..usa_count - 1 {
        let sector_end = (i + 1) * bytes_per_sector;
        if sector_end < 2 || sector_end > rec.len() {
            return false;
        }
        if u16_at(rec, sector_end - 2) != usn {
            return false;
        }
        let fix = u16_at(rec, usa_off + 2 + i * 2);
        rec[sector_end - 2] = (fix & 0xff) as u8;
        rec[sector_end - 1] = (fix >> 8) as u8;
    }
    true
}

fn parse_runs(runs: &[u8]) -> Vec<(i64, u64)> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut lcn: i64 = 0;

    while pos < runs.len() {
        let header = runs[pos];
        if header == 0 {
            break;
        }
        let len_size = (header & 0x0f) as usize;
        let off_size = (header >> 4) as usize;
        pos += 1;
        if len_size == 0 || pos + len_size + off_size > runs.len() {
            break;
        }

        let mut run_len: u64 = 0;
        for i in 0..len_size {
            run_len |= (runs[pos + i] as u64) << (i * 8);
        }
        pos += len_size;

        if off_size == 0 {
            pos += off_size;
            continue;
        }

        let mut run_off: i64 = 0;
        for i in 0..off_size {
            run_off |= (runs[pos + i] as i64) << (i * 8);
        }
        let sign_bit = 1i64 << (off_size * 8 - 1);
        if run_off & sign_bit != 0 {
            run_off -= 1i64 << (off_size * 8);
        }
        pos += off_size;

        lcn += run_off;
        if lcn >= 0 && run_len > 0 {
            out.push((lcn, run_len));
        }
    }
    out
}

#[derive(Clone, Debug)]
struct DataFragment {
    start_vcn: u64,
    runs: Vec<(i64, u64)>,
}

fn collect_data_fragments(rec: &[u8], out: &mut Vec<DataFragment>) {
    let attrs_off = u16_at(rec, 0x14) as usize;
    let mut pos = attrs_off;

    while pos + 8 <= rec.len() {
        let atype = u32_at(rec, pos);
        if atype == 0xffff_ffff {
            break;
        }
        let alen = u32_at(rec, pos + 4) as usize;
        if alen < 0x10 || pos + alen > rec.len() {
            break;
        }
        let non_resident = rec[pos + 8] == 1;
        let name_len = rec[pos + 9];
        if atype == 0x80 && non_resident && name_len == 0 && pos + 0x22 <= rec.len() {
            let start_vcn = u64_at(rec, pos + 0x10);
            let run_off = u16_at(rec, pos + 0x20) as usize;
            if run_off < alen {
                out.push(DataFragment {
                    start_vcn,
                    runs: parse_runs(&rec[pos + run_off..pos + alen]),
                });
            }
        }
        pos += alen;
    }
}

fn attribute_list_data_records(list: &[u8]) -> Vec<u64> {
    let mut out = Vec::new();
    let mut pos = 0usize;

    while pos + 0x18 <= list.len() {
        let atype = u32_at(list, pos);
        let entry_len = u16_at(list, pos + 0x04) as usize;
        if entry_len < 0x18 || pos + entry_len > list.len() {
            break;
        }
        let name_len = list[pos + 0x06];
        if atype == 0x80 && name_len == 0 {
            let rec_no = u64_at(list, pos + 0x10) & 0x0000_ffff_ffff_ffff;
            if rec_no != 0 && !out.contains(&rec_no) {
                out.push(rec_no);
            }
        }
        pos += entry_len;
    }
    out
}

fn read_attribute_list(
    rec: &[u8],
    vol: &Volume,
    bytes_per_cluster: u64,
) -> Option<Vec<u8>> {
    let attrs_off = u16_at(rec, 0x14) as usize;
    let mut pos = attrs_off;

    while pos + 8 <= rec.len() {
        let atype = u32_at(rec, pos);
        if atype == 0xffff_ffff {
            break;
        }
        let alen = u32_at(rec, pos + 4) as usize;
        if alen < 0x10 || pos + alen > rec.len() {
            break;
        }
        if atype == 0x20 {
            let non_resident = rec[pos + 8] == 1;
            if !non_resident {
                let val_off = u16_at(rec, pos + 0x14) as usize;
                let val_len = u32_at(rec, pos + 0x10) as usize;
                let v = pos + val_off;
                if v + val_len <= rec.len() {
                    return Some(rec[v..v + val_len].to_vec());
                }
                return None;
            }
            let data_size = u64_at(rec, pos + 0x30) as usize;
            let run_off = u16_at(rec, pos + 0x20) as usize;
            if run_off >= alen {
                return None;
            }
            let runs = parse_runs(&rec[pos + run_off..pos + alen]);
            let mut buf = Vec::with_capacity(data_size);
            for (lcn, clusters) in runs {
                let want = (clusters * bytes_per_cluster) as usize;
                let mut chunk = vec![0u8; want];
                if vol
                    .read_at(lcn as u64 * bytes_per_cluster, &mut chunk)
                    .is_err()
                {
                    return None;
                }
                buf.extend_from_slice(&chunk);
                if buf.len() >= data_size {
                    break;
                }
            }
            buf.truncate(data_size);
            return Some(buf);
        }
        pos += alen;
    }
    None
}

fn flatten_fragments(mut frags: Vec<DataFragment>) -> Vec<(i64, u64)> {
    frags.sort_by_key(|f| f.start_vcn);
    let mut out = Vec::new();
    for f in frags {
        out.extend(f.runs);
    }
    out
}

fn read_mft_record(
    vol: &Volume,
    runs: &[(i64, u64)],
    rec_no: u64,
    rec_size: usize,
    bytes_per_cluster: u64,
    bytes_per_sector: usize,
) -> Option<Vec<u8>> {
    let byte_off = rec_no * rec_size as u64;
    let target_vcn = byte_off / bytes_per_cluster;
    let within = byte_off % bytes_per_cluster;

    let mut vcn = 0u64;
    for &(lcn, clusters) in runs {
        if target_vcn < vcn + clusters {
            let lcn_at = lcn as u64 + (target_vcn - vcn);
            let mut buf = vec![0u8; rec_size];
            vol.read_at(lcn_at * bytes_per_cluster + within, &mut buf)
                .ok()?;
            if !apply_fixup(&mut buf, bytes_per_sector) {
                return None;
            }
            return Some(buf);
        }
        vcn += clusters;
    }
    None
}

#[derive(Clone, Default)]
struct Entry {
    parent: u32,
    name: String,
    is_dir: bool,
    size: u64,
    used: bool,
    base_ref: u32,
}

fn parse_record(rec: &[u8], out: &mut Entry, links: &mut Vec<(u32, u8)>) -> bool {
    if rec.len() < 0x30 || &rec[0..4] != b"FILE" {
        return false;
    }
    let flags = u16_at(rec, 0x16);
    let in_use = flags & 0x01 != 0;
    let is_dir = flags & 0x02 != 0;
    if !in_use {
        return false;
    }

    let base_ref = (u64_at(rec, 0x20) & 0x0000_ffff_ffff_ffff) as u32;

    let attrs_off = u16_at(rec, 0x14) as usize;
    let mut pos = attrs_off;

    let mut best_namespace = 0xffu8;
    let mut parent = 0u32;
    let mut name = String::new();
    let mut size = 0u64;
    let mut got_name = false;

    while pos + 8 <= rec.len() {
        let atype = u32_at(rec, pos);
        if atype == 0xffff_ffff {
            break;
        }
        let alen = u32_at(rec, pos + 4) as usize;
        if alen < 0x10 || pos + alen > rec.len() {
            break;
        }
        let non_resident = rec[pos + 8] == 1;
        let name_len = rec[pos + 9] as usize;

        match atype {
            0x30 if !non_resident => {
                let val_off = u16_at(rec, pos + 0x14) as usize;
                let val_len = u32_at(rec, pos + 0x10) as usize;
                let v = pos + val_off;
                if val_len >= 0x42 && v + val_len <= rec.len() {
                    let fname_len = rec[v + 0x40] as usize;
                    let namespace = rec[v + 0x41];
                    let rank = match namespace {
                        1 => 0u8,
                        3 => 1,
                        0 => 2,
                        _ => 3,
                    };
                    if v + 0x42 + fname_len * 2 <= rec.len() {
                        let this_parent = (u64_at(rec, v) & 0x0000_ffff_ffff_ffff) as u32;

                        match links.iter_mut().find(|(p, _)| *p == this_parent) {
                            Some(slot) => {
                                if rank < slot.1 {
                                    slot.1 = rank;
                                }
                            }
                            None => links.push((this_parent, rank)),
                        }

                        if rank < best_namespace {
                            best_namespace = rank;
                            parent = this_parent;
                            let units: Vec<u16> = (0..fname_len)
                                .map(|i| u16_at(rec, v + 0x42 + i * 2))
                                .collect();
                            name = String::from_utf16_lossy(&units);
                            got_name = true;
                        }
                    }
                }
            }
            0x80 if name_len == 0 => {
                if non_resident {
                    if u64_at(rec, pos + 0x10) == 0 && pos + 0x38 <= rec.len() {
                        size = u64_at(rec, pos + 0x30);
                    }
                } else {
                    size = u32_at(rec, pos + 0x10) as u64;
                }
            }
            _ => {}
        }
        pos += alen;
    }

    if base_ref != 0 {
        out.base_ref = base_ref;
        out.size = size;
        return true;
    }

    if !got_name {
        return false;
    }

    out.parent = parent;
    out.name = name;
    out.is_dir = is_dir;
    out.size = if is_dir { 0 } else { size };
    out.used = true;
    true
}

pub fn scan_volume(letter: char, top_n: usize) -> Result<MftScan, MftError> {
    let started = Instant::now();

    let vol = Volume::open(letter)?;
    let vd = vol.volume_data()?;

    let bytes_per_cluster = vd.bytes_per_cluster as u64;
    let bytes_per_sector = vd.bytes_per_sector as usize;
    let rec_size = vd.bytes_per_file_record_segment as usize;
    let mft_offset = vd.mft_start_lcn as u64 * bytes_per_cluster;

    let mut first = vec![0u8; rec_size.max(bytes_per_sector)];
    vol.read_at(mft_offset, &mut first)?;
    if !apply_fixup(&mut first, bytes_per_sector) {
        return Err(MftError::NotNtfs);
    }
    let mut frags: Vec<DataFragment> = Vec::new();
    collect_data_fragments(&first, &mut frags);

    let mut ext_records = 0usize;
    if let Some(list) = read_attribute_list(&first, &vol, bytes_per_cluster) {
        let partial = flatten_fragments(frags.clone());
        for rec_no in attribute_list_data_records(&list) {
            if rec_no == 0 {
                continue;
            }
            if let Some(ext) = read_mft_record(
                &vol,
                &partial,
                rec_no,
                rec_size,
                bytes_per_cluster,
                bytes_per_sector,
            ) {
                let before = frags.len();
                collect_data_fragments(&ext, &mut frags);
                if frags.len() > before {
                    ext_records += 1;
                }
            }
        }
    }

    let runs = flatten_fragments(frags);
    if runs.is_empty() {
        return Err(MftError::NotNtfs);
    }
    let run_clusters: u64 = runs.iter().map(|&(_, c)| c).sum();

    let mft_valid = vd.mft_valid_data_length.max(0) as u64;
    let est_records = (mft_valid / rec_size as u64) as usize;
    let mut entries: Vec<Entry> = Vec::with_capacity(est_records + 1024);

    let mut buf = vec![0u8; CHUNK_BYTES];
    let mut consumed: u64 = 0;
    let mut links: Vec<(u32, u8)> = Vec::with_capacity(8);
    let mut hard_links: Vec<(u32, u32)> = Vec::new();

    'outer: for (lcn, clusters) in runs {
        let run_bytes = clusters * bytes_per_cluster;
        let base = lcn as u64 * bytes_per_cluster;
        let mut done: u64 = 0;

        while done < run_bytes {
            if consumed >= mft_valid {
                break 'outer;
            }
            let remain = run_bytes - done;
            let want = (CHUNK_BYTES as u64).min(remain) / rec_size as u64 * rec_size as u64;
            if want == 0 {
                break;
            }

            let got = vol.read_at(base + done, &mut buf[..want as usize])?;
            let full = got / rec_size;
            if full == 0 {
                break 'outer;
            }

            for k in 0..full {
                let rec_no = entries.len() as u32;
                let rec = &mut buf[k * rec_size..(k + 1) * rec_size];
                let mut entry = Entry::default();
                links.clear();
                if apply_fixup(rec, bytes_per_sector) {
                    parse_record(rec, &mut entry, &mut links);
                }

                if entry.base_ref != 0 {
                    for &(p, _) in links.iter() {
                        hard_links.push((entry.base_ref, p));
                    }
                } else if entry.used && !entry.is_dir && links.len() > 1 {
                    for &(p, _) in links.iter() {
                        if p != entry.parent {
                            hard_links.push((rec_no, p));
                        }
                    }
                }
                entries.push(entry);
            }

            let advanced = (full * rec_size) as u64;
            done += advanced;
            consumed += advanced;
        }
    }

    if entries.len() <= ROOT_RECORD as usize {
        return Err(MftError::NotNtfs);
    }

    let n = entries.len();

    let mut merged_from_ext = 0u64;
    for i in 0..n {
        let (base, size) = (entries[i].base_ref as usize, entries[i].size);
        if base == 0 || size == 0 || base >= n {
            continue;
        }
        if entries[base].used && !entries[base].is_dir {
            entries[base].size += size;
            merged_from_ext += 1;
        }
    }

    let mut dir_size = vec![0u64; n];
    let mut dir_files = vec![0u64; n];
    let mut total_size = 0u64;
    let mut file_count = 0u64;
    let mut dir_count = 0u64;

    for i in 0..n {
        let e = &entries[i];
        if !e.used {
            continue;
        }
        if e.is_dir {
            dir_count += 1;
            continue;
        }
        file_count += 1;
        total_size += e.size;

        add_to_ancestors(&entries, &mut dir_size, &mut dir_files, e.parent, e.size);
    }

    hard_links.sort_unstable();
    hard_links.dedup();
    hard_links.retain(|&(rec_no, parent)| {
        let i = rec_no as usize;
        i < n && entries[i].used && !entries[i].is_dir && entries[i].parent != parent
    });

    let mut hard_link_size = 0u64;
    for &(rec_no, parent) in &hard_links {
        let size = entries[rec_no as usize].size;
        hard_link_size += size;
        add_to_ancestors(&entries, &mut dir_size, &mut dir_files, parent, size);
    }
    let unique_size = total_size;
    let unique_files = file_count;
    total_size += hard_link_size;
    file_count += hard_links.len() as u64;

    // 目录体积排行榜只有命令行工具 mftscan 用得上；GUI 走的是 MftTree
    // 逐层下钻，不需要这份榜单。top_n 为 0 时直接跳过全盘排序与路径解析。
    let dirs: Vec<DirUsage> = if top_n == 0 {
        Vec::new()
    } else {
        let mut ranked: Vec<u32> = (0..n as u32)
            .filter(|&i| {
                entries[i as usize].used
                    && entries[i as usize].is_dir
                    && dir_size[i as usize] > 0
            })
            .collect();
        ranked.sort_unstable_by(|&a, &b| dir_size[b as usize].cmp(&dir_size[a as usize]));
        ranked.truncate(top_n);

        let mut cache: HashMap<u32, String> = HashMap::new();
        ranked
            .iter()
            .map(|&i| DirUsage {
                path: resolve_path(&entries, i, letter, &mut cache),
                size: dir_size[i as usize],
                file_count: dir_files[i as usize],
            })
            .collect()
    };

    let tree = build_tree(letter, entries, dir_size, dir_files);

    Ok(MftScan {
        volume: letter,
        tree,
        total_size,
        file_count,
        dir_count,
        dirs,
        elapsed_ms: started.elapsed().as_millis() as u64,
        records_read: n as u64,
        records_expected: mft_valid / rec_size as u64,
        mft_run_bytes: run_clusters * bytes_per_cluster,
        ext_records: ext_records as u64,
        ext_data_merged: merged_from_ext,
        hard_links: hard_links.len() as u64,
        unique_size,
        unique_files,
    })
}

fn build_tree(
    volume: char,
    entries: Vec<Entry>,
    dir_size: Vec<u64>,
    dir_files: Vec<u64>,
) -> MftTree {
    let n = entries.len();

    let mut counts = vec![0u32; n];
    for i in 0..n {
        let e = &entries[i];
        if !e.used || i as u32 == ROOT_RECORD {
            continue;
        }
        let p = e.parent as usize;
        if p < n && entries[p].used && entries[p].is_dir {
            counts[p] += 1;
        }
    }

    let mut child_start = vec![0u32; n + 1];
    for i in 0..n {
        child_start[i + 1] = child_start[i] + counts[i];
    }

    let mut cursor: Vec<u32> = child_start[..n].to_vec();
    let mut child_at = vec![0u32; child_start[n] as usize];
    for i in 0..n {
        let e = &entries[i];
        if !e.used || i as u32 == ROOT_RECORD {
            continue;
        }
        let p = e.parent as usize;
        if p < n && entries[p].used && entries[p].is_dir {
            child_at[cursor[p] as usize] = i as u32;
            cursor[p] += 1;
        }
    }

    MftTree {
        volume,
        entries,
        dir_size,
        dir_files,
        child_start,
        child_at,
    }
}

fn add_to_ancestors(
    entries: &[Entry],
    dir_size: &mut [u64],
    dir_files: &mut [u64],
    start: u32,
    size: u64,
) {
    let n = entries.len();
    let mut cur = start;
    let mut depth = 0;
    loop {
        let idx = cur as usize;
        if idx >= n || depth > MAX_DEPTH {
            break;
        }
        dir_size[idx] += size;
        dir_files[idx] += 1;
        if cur == ROOT_RECORD {
            break;
        }
        let next = entries[idx].parent;
        if next == cur {
            break;
        }
        cur = next;
        depth += 1;
    }
}

fn resolve_path(
    entries: &[Entry],
    idx: u32,
    letter: char,
    cache: &mut HashMap<u32, String>,
) -> String {
    if idx == ROOT_RECORD {
        return format!("{letter}:");
    }
    if let Some(hit) = cache.get(&idx) {
        return hit.clone();
    }

    let mut chain: Vec<u32> = Vec::new();
    let mut cur = idx;
    let mut base = format!("{letter}:");
    let mut depth = 0;

    loop {
        if cur == ROOT_RECORD || depth > MAX_DEPTH {
            break;
        }
        if let Some(hit) = cache.get(&cur) {
            base = hit.clone();
            break;
        }
        let i = cur as usize;
        if i >= entries.len() || !entries[i].used {
            break;
        }
        chain.push(cur);
        let next = entries[i].parent;
        if next == cur {
            break;
        }
        cur = next;
        depth += 1;
    }

    let mut path = base;
    for &c in chain.iter().rev() {
        path.push('\\');
        path.push_str(&entries[c as usize].name);
        cache.insert(c, path.clone());
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_tree() -> MftTree {
        let mut entries = vec![Entry::default(); 12];
        let mut mk = |i: usize, parent: u32, name: &str, is_dir: bool, size: u64| {
            entries[i] = Entry {
                parent,
                name: name.to_string(),
                is_dir,
                size,
                used: true,
                base_ref: 0,
            };
        };
        mk(5, 5, "", true, 0);
        mk(6, 5, "Windows", true, 0);
        mk(7, 5, "Users", true, 0);
        mk(8, 6, "a.dll", false, 100);
        mk(9, 7, "me", true, 0);
        mk(10, 9, "big.iso", false, 5000);
        mk(11, 7, "readme.txt", false, 50);

        let mut dir_size = vec![0u64; 12];
        let mut dir_files = vec![0u64; 12];
        for (i, sz) in [(5, 5150u64), (6, 100), (7, 5050), (9, 5000)] {
            dir_size[i] = sz;
        }
        for (i, fc) in [(5, 3u64), (6, 1), (7, 2), (9, 1)] {
            dir_files[i] = fc;
        }
        build_tree('C', entries, dir_size, dir_files)
    }

    #[test]
    fn children_are_sorted_by_size_desc() {
        let t = synthetic_tree();
        let kids = t.children(t.root());
        let names: Vec<&str> = kids.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Users", "Windows"]);
        assert_eq!(kids[0].size, 5050);
        assert_eq!(kids[1].size, 100);
    }

    #[test]
    fn children_mix_dirs_and_files() {
        let t = synthetic_tree();
        let kids = t.children(7);
        let names: Vec<&str> = kids.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["me", "readme.txt"]);
        assert!(kids[0].is_dir);
        assert!(!kids[1].is_dir);
        assert_eq!(kids[1].size, 50);
    }

    #[test]
    fn own_size_excludes_subdirectories() {
        let t = synthetic_tree();
        let kids = t.children(t.root());
        let users = kids.iter().find(|c| c.name == "Users").unwrap();
        assert_eq!(users.size, 5050);
        assert_eq!(users.own_size, 50);

        let me = t.children(7).into_iter().find(|c| c.name == "me").unwrap();
        assert_eq!(me.own_size, 5000);
    }

    #[test]
    fn resolves_full_paths() {
        let t = synthetic_tree();
        assert_eq!(t.path_of(10), r"C:\Users\me\big.iso");
        assert_eq!(t.path_of(6), r"C:\Windows");
        assert_eq!(t.path_of(t.root()), "C:");
    }

    #[test]
    fn parent_walks_up_and_stops_at_root() {
        let t = synthetic_tree();
        assert_eq!(t.parent_of(10), Some(9));
        assert_eq!(t.parent_of(9), Some(7));
        assert_eq!(t.parent_of(7), Some(ROOT_RECORD));
        assert_eq!(t.parent_of(ROOT_RECORD), None);
    }

    #[test]
    fn largest_files_ignores_directories() {
        let t = synthetic_tree();
        let files = t.largest_files(10);
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["big.iso", "a.dll", "readme.txt"]);
        assert!(files.iter().all(|f| !f.is_dir));
    }

    #[test]
    fn largest_files_respects_limit() {
        let t = synthetic_tree();
        let files = t.largest_files(2);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "big.iso");
        assert_eq!(files[1].name, "a.dll");
        assert!(t.largest_files(0).is_empty());
    }

    #[test]
    fn empty_directory_has_no_children() {
        let t = synthetic_tree();
        assert!(t.children(8).is_empty());
    }

    #[test]
    fn ntfs_metadata_is_filtered_from_children_and_largest_files() {
        let mut entries = vec![Entry::default(); 16];
        let mut mk = |i: usize, parent: u32, name: &str, is_dir: bool, size: u64| {
            entries[i] = Entry {
                parent,
                name: name.to_string(),
                is_dir,
                size,
                used: true,
                base_ref: 0,
            };
        };
        mk(0, 5, "$MFT", false, 3_000_000_000);
        mk(2, 5, "$LogFile", false, 64_000_000);
        mk(5, 5, "", true, 0);
        mk(6, 5, "MyData", true, 0);
        mk(11, 5, "$Extend", true, 100_000);
        mk(14, 6, "video.mp4", false, 1_000_000_000);

        let mut dir_size = vec![0u64; 16];
        let mut dir_files = vec![0u64; 16];
        dir_size[5] = 4_064_100_000;
        dir_files[5] = 4;
        let t = build_tree('C', entries, dir_size, dir_files);

        // 验证根目录下过滤掉了 $MFT, $LogFile, $Extend，只保留 MyData
        let kids = t.children(t.root());
        assert_eq!(kids.len(), 1);
        assert_eq!(kids[0].name, "MyData");

        // 验证全盘大文件排除了 $MFT 和 $LogFile，只排入普通文件 video.mp4
        let files = t.largest_files(10);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "video.mp4");
    }

    #[test]
    fn parse_runs_single() {
        let runs = [0x21u8, 0x18, 0x34, 0x02, 0x00];
        assert_eq!(parse_runs(&runs), vec![(0x0234, 0x18)]);
    }

    #[test]
    fn parse_runs_negative_offset() {
        let runs = [0x11u8, 0x10, 0x20, 0x11, 0x10, 0xe0, 0x00];
        let got = parse_runs(&runs);
        assert_eq!(got, vec![(0x20, 0x10), (0x00, 0x10)]);
    }

    #[test]
    fn parse_runs_stops_at_terminator() {
        let runs = [0x11u8, 0x08, 0x10, 0x00, 0x11, 0x08, 0x10];
        assert_eq!(parse_runs(&runs), vec![(0x10, 0x08)]);
    }

    fn attr_list_entry(atype: u32, start_vcn: u64, rec_no: u64, name_len: u8) -> Vec<u8> {
        let mut e = vec![0u8; 0x18];
        e[0x00..0x04].copy_from_slice(&atype.to_le_bytes());
        e[0x04..0x06].copy_from_slice(&0x18u16.to_le_bytes());
        e[0x06] = name_len;
        e[0x08..0x10].copy_from_slice(&start_vcn.to_le_bytes());
        e[0x10..0x18].copy_from_slice(&rec_no.to_le_bytes());
        e
    }

    #[test]
    fn attribute_list_picks_unnamed_data_records() {
        let mut list = Vec::new();
        list.extend(attr_list_entry(0x10, 0, 0, 0));
        list.extend(attr_list_entry(0x80, 0, 0, 0));
        list.extend(attr_list_entry(0x80, 100, 42, 0));
        list.extend(attr_list_entry(0x80, 200, 77, 0));
        list.extend(attr_list_entry(0x80, 300, 99, 4));
        list.extend(attr_list_entry(0x80, 400, 42, 0));

        assert_eq!(attribute_list_data_records(&list), vec![42, 77]);
    }

    #[test]
    fn flatten_orders_fragments_by_vcn() {
        let frags = vec![
            DataFragment { start_vcn: 100, runs: vec![(0x50, 4)] },
            DataFragment { start_vcn: 0, runs: vec![(0x10, 2), (0x30, 3)] },
            DataFragment { start_vcn: 300, runs: vec![(0x90, 1)] },
        ];
        assert_eq!(
            flatten_fragments(frags),
            vec![(0x10, 2), (0x30, 3), (0x50, 4), (0x90, 1)]
        );
    }

    #[test]
    fn fixup_rejects_corrupt_record() {
        let mut rec = vec![0u8; 1024];
        rec[0..4].copy_from_slice(b"FILE");
        rec[0x04] = 0x30;
        rec[0x06] = 3;
        rec[0x30] = 0xaa;
        rec[0x31] = 0xbb;
        assert!(!apply_fixup(&mut rec, 512));
    }

    #[test]
    fn fixup_restores_sector_tails() {
        let mut rec = vec![0u8; 1024];
        rec[0..4].copy_from_slice(b"FILE");
        rec[0x04] = 0x30;
        rec[0x06] = 3;
        rec[0x30] = 0xaa;
        rec[0x31] = 0xbb;
        rec[0x32] = 0x22;
        rec[0x33] = 0x11;
        rec[0x34] = 0x44;
        rec[0x35] = 0x33;
        rec[510] = 0xaa;
        rec[511] = 0xbb;
        rec[1022] = 0xaa;
        rec[1023] = 0xbb;

        assert!(apply_fixup(&mut rec, 512));
        assert_eq!(u16_at(&rec, 510), 0x1122);
        assert_eq!(u16_at(&rec, 1022), 0x3344);
    }
}
