//! MFT 字节解析与记录解析

use super::mft_types::*;
use crate::core::disk::ScanError;
use std::os::windows::ffi::OsStrExt;

const FSCTL_GET_NTFS_VOLUME_DATA: u32 = 0x0009_0064;

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub(super) struct NtfsVolumeData {
    volume_serial_number: i64,
    number_sectors: i64,
    total_clusters: i64,
    free_clusters: i64,
    total_reserved: i64,
    pub(super) bytes_per_sector: u32,
    pub(super) bytes_per_cluster: u32,
    pub(super) bytes_per_file_record_segment: u32,
    clusters_per_file_record_segment: u32,
    pub(super) mft_valid_data_length: i64,
    pub(super) mft_start_lcn: i64,
    mft2_start_lcn: i64,
    mft_zone_start: i64,
    mft_zone_end: i64,
}

pub(super) struct Volume {
    handle: winapi::um::winnt::HANDLE,
}

impl Drop for Volume {
    fn drop(&mut self) {
        // SAFETY: handle 只可能来自 Volume::open 里成功的 CreateFileW，
        // Volume 不实现 Clone，因此这里是唯一一次关闭。
        unsafe { winapi::um::handleapi::CloseHandle(self.handle) };
    }
}

impl Volume {
    pub(super) fn open(letter: char) -> Result<Self, ScanError> {
        use winapi::um::fileapi::{CreateFileW, OPEN_EXISTING};
        use winapi::um::handleapi::INVALID_HANDLE_VALUE;
        use winapi::um::winnt::{FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ};

        let path = format!("\\\\.\\{letter}:");
        let wide: Vec<u16> = std::ffi::OsStr::new(&path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // SAFETY: wide 是本地 Vec 且以 NUL 结尾，活到调用结束。其余参数
        // 都是常量标志位。返回值在使用前会与 INVALID_HANDLE_VALUE 比对。
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
            // SAFETY: GetLastError 不接收参数，只读当前线程的错误码。
            let err = unsafe { winapi::um::errhandlingapi::GetLastError() };
            return Err(if err == 5 {
                ScanError::AccessDenied
            } else {
                ScanError::Io(format!("CreateFileW (Win32 {err})"))
            });
        }
        Ok(Volume { handle })
    }

    pub(super) fn volume_data(&self) -> Result<NtfsVolumeData, ScanError> {
        use winapi::um::ioapiset::DeviceIoControl;

        let mut data = NtfsVolumeData::default();
        let mut returned: u32 = 0;
        // SAFETY: self.handle 是构造时校验过的有效卷句柄。输出缓冲是本地的
        // NtfsVolumeData，按它的真实大小上报；驱动写入量由 bytes 出参回报。
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
            return Err(ScanError::UnsupportedFilesystem("NTFS"));
        }
        Ok(data)
    }

    pub(super) fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize, ScanError> {
        use winapi::um::fileapi::{ReadFile, SetFilePointerEx};

        // SAFETY: LARGE_INTEGER 是个纯 POD union，全零是合法位模式；
        // QuadPart_mut 拿到的是这块本地内存里 i64 那一路的可变引用。
        let mut distance: winapi::um::winnt::LARGE_INTEGER = unsafe { std::mem::zeroed() };
        unsafe { *distance.QuadPart_mut() = offset as i64 };

        // SAFETY: 句柄有效，distance 按值传入，第三个参数传 null 表示
        // 不需要回报新位置——这是文档允许的。
        let ok = unsafe { SetFilePointerEx(self.handle, distance, std::ptr::null_mut(), 0) };
        if ok == 0 {
            // SAFETY: GetLastError 不接收参数，只读当前线程的错误码。
            let err = unsafe { winapi::um::errhandlingapi::GetLastError() };
            return Err(ScanError::Io(format!(
                "SetFilePointerEx @{offset} (Win32 {err})"
            )));
        }

        let mut read: u32 = 0;
        // SAFETY: buf 是调用方提供的可变切片，指针与长度取自它本身，
        // ReadFile 写入量不会超过上报的长度。第五个参数传 null 表示同步读，
        // 与 CreateFileW 时没有指定 FILE_FLAG_OVERLAPPED 一致。
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
            // SAFETY: GetLastError 不接收参数，只读当前线程的错误码。
            let err = unsafe { winapi::um::errhandlingapi::GetLastError() };
            return Err(ScanError::Io(format!("ReadFile @{offset} (Win32 {err})")));
        }
        Ok(read as usize)
    }
}

// ---------------------------------------------------------------------------
// 字节解析辅助
// ---------------------------------------------------------------------------

// 说明：`ScanError::Io` 的 payload 必须是**语言中立**的技术细节（API 名 +
// Win32 错误码）。它会被 `ui::i18n::tr_mft_error` 原样嵌进本地化的外层文案里，
// payload 自己写中文的话，英文界面上就会冒出半句中文。

pub(super) fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

pub(super) fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

pub(super) fn u64_at(b: &[u8], off: usize) -> u64 {
    let mut v = [0u8; 8];
    v.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(v)
}

pub(super) fn apply_fixup(rec: &mut [u8], bytes_per_sector: usize) -> bool {
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

/// 把 `len_size` / `off_size` 个小端字节拼成 u64。调用方保证 `n <= 8`。
pub(super) fn le_bytes_to_u64(b: &[u8], n: usize) -> u64 {
    let mut v = 0u64;
    for (i, &byte) in b.iter().take(n).enumerate() {
        v |= (byte as u64) << (i * 8);
    }
    v
}

/// 把 `n` 字节的小端值按二进制补码符号扩展成 i64。调用方保证 `1 <= n <= 8`。
///
/// 不能写成「减去 `1 << (n*8)`」：`n == 8` 时那是 `1i64 << 64`，直接移位溢出。
/// 左移到最高位再算术右移回来，`n == 8` 时 shift 为 0，天然退化成恒等变换。
pub(super) fn sign_extend(raw: u64, n: usize) -> i64 {
    let shift = 64 - n * 8;
    ((raw << shift) as i64) >> shift
}

pub(super) fn parse_runs(runs: &[u8]) -> Vec<(i64, u64)> {
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
        // 字段宽度是磁盘上的 4 bit，最大 15，但 u64/i64 最多装得下 8 字节。
        // 不挡住就会在下面的移位上溢出——损坏记录能给出 15，而 off_size == 8
        // 本身在大卷上完全合法，老写法在那里也会炸。
        if len_size == 0 || len_size > 8 || off_size > 8 {
            break;
        }
        if pos + len_size + off_size > runs.len() {
            break;
        }

        let run_len = le_bytes_to_u64(&runs[pos..], len_size);
        pos += len_size;

        // off_size == 0 是「稀疏段」：不占实际簇，跳过但不能中断整个 run list
        if off_size == 0 {
            continue;
        }

        let run_off = sign_extend(le_bytes_to_u64(&runs[pos..], off_size), off_size);
        pos += off_size;

        // LCN 是累加出来的，每一步的增量都直接来自磁盘。加不动了说明这份
        // run list 本身就是垃圾，后面的段也没有解析价值，就此收手。
        let Some(next) = lcn.checked_add(run_off) else {
            break;
        };
        lcn = next;
        if lcn >= 0 && run_len > 0 {
            out.push((lcn, run_len));
        }
    }
    out
}

/// 属性头的最小长度：常驻 0x18 字节，非常驻 0x40 字节。
///
/// 以前这里的下限是 0x10（只覆盖属性头的前 16 字节），但后面每一处读
/// 0x10 / 0x14 / 0x20 / 0x30 偏移的代码都超出了这个范围。损坏或撕裂的
/// 记录只要声明一个 `alen == 0x10` 的非常驻属性，就能让解析越界 panic。
/// 这些偏移全部落在属性头内部，因此把下限提到真实头长度之后，后续读取
/// 一次性全部安全，不必再逐处补检查。
/// `$ATTRIBUTE_LIST` 允许的最大长度。
///
/// 真实卷上这份列表最多几十 KB（碎片极多的巨型文件），16 MB 是量级上
/// 绰绰有余的天花板。它的作用不是精确，而是让「磁盘上声明的长度」在被
/// 拿去分配内存之前有个上界。
const MAX_ATTR_LIST_BYTES: u64 = 16 * 1024 * 1024;

const ATTR_HDR_RESIDENT: usize = 0x18;
const ATTR_HDR_NON_RESIDENT: usize = 0x40;

/// 校验 `rec[pos..]` 处的属性头，返回 `(类型, 属性总长, 是否非常驻, 名字长度)`。
///
/// 返回 `None` 表示「到此为止」：属性表结束标记、越界、或长度字段不自洽。
/// 调用方一律应当终止遍历，不要试图跳过继续——长度不可信时无从跳起。
pub(super) fn attr_header(rec: &[u8], pos: usize) -> Option<(u32, usize, bool, usize)> {
    if pos + 0x10 > rec.len() {
        return None;
    }
    let atype = u32_at(rec, pos);
    if atype == 0xffff_ffff {
        return None;
    }
    let alen = u32_at(rec, pos + 4) as usize;
    let non_resident = rec[pos + 8] == 1;
    let name_len = rec[pos + 9] as usize;
    let min = if non_resident {
        ATTR_HDR_NON_RESIDENT
    } else {
        ATTR_HDR_RESIDENT
    };
    if alen < min || pos + alen > rec.len() {
        return None;
    }
    Some((atype, alen, non_resident, name_len))
}

#[derive(Clone, Debug)]
pub(super) struct DataFragment {
    pub(super) start_vcn: u64,
    pub(super) runs: Vec<(i64, u64)>,
}

pub(super) fn collect_data_fragments(rec: &[u8], out: &mut Vec<DataFragment>) {
    let mut pos = u16_at(rec, 0x14) as usize;

    while let Some((atype, alen, non_resident, name_len)) = attr_header(rec, pos) {
        if atype == 0x80 && non_resident && name_len == 0 {
            let start_vcn = u64_at(rec, pos + 0x10);
            let run_off = u16_at(rec, pos + 0x20) as usize;
            // run_off 必须落在属性内部，否则切片起点会跑到下一个属性里
            if run_off >= ATTR_HDR_NON_RESIDENT && run_off < alen {
                out.push(DataFragment {
                    start_vcn,
                    runs: parse_runs(&rec[pos + run_off..pos + alen]),
                });
            }
        }
        pos += alen;
    }
}

pub(super) fn attribute_list_data_records(list: &[u8]) -> Vec<u64> {
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

pub(super) fn read_attribute_list(
    rec: &[u8],
    vol: &Volume,
    bytes_per_cluster: u64,
) -> Option<Vec<u8>> {
    let mut pos = u16_at(rec, 0x14) as usize;

    while let Some((atype, alen, non_resident, _)) = attr_header(rec, pos) {
        if atype == 0x20 {
            if !non_resident {
                let val_off = u16_at(rec, pos + 0x14) as usize;
                let val_len = u32_at(rec, pos + 0x10) as usize;
                let v = pos + val_off;
                if v + val_len <= rec.len() {
                    return Some(rec[v..v + val_len].to_vec());
                }
                return None;
            }

            // data_size 是磁盘上声明的长度，不能直接拿去 with_capacity——
            // 一条损坏记录声明 2^40 字节就能把进程 OOM 掉。
            let data_size = u64_at(rec, pos + 0x30);
            if data_size > MAX_ATTR_LIST_BYTES {
                return None;
            }
            let data_size = data_size as usize;

            let run_off = u16_at(rec, pos + 0x20) as usize;
            if run_off < ATTR_HDR_NON_RESIDENT || run_off >= alen {
                return None;
            }
            let runs = parse_runs(&rec[pos + run_off..pos + alen]);
            let mut buf = Vec::with_capacity(data_size);
            for (lcn, clusters) in runs {
                // 簇数同样来自磁盘：乘法要挡溢出，单次读取量也要挡住
                let want = clusters
                    .checked_mul(bytes_per_cluster)
                    .filter(|&w| w <= MAX_ATTR_LIST_BYTES)? as usize;
                let at = (lcn as u64).checked_mul(bytes_per_cluster)?;
                let mut chunk = vec![0u8; want];
                if vol.read_at(at, &mut chunk).is_err() {
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

pub(super) fn flatten_fragments(mut frags: Vec<DataFragment>) -> Vec<(i64, u64)> {
    frags.sort_by_key(|f| f.start_vcn);
    let mut out = Vec::new();
    for f in frags {
        out.extend(f.runs);
    }
    out
}

pub(super) fn read_mft_record(
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
        // 簇数来自磁盘，累加可能溢出；饱和之后 target_vcn 必定落在区间内，
        // 于是这一段被当成「命中」，再由下面的 checked 计算把它挡回去。
        let end = vcn.saturating_add(clusters);
        if target_vcn < end {
            let lcn_at = (lcn as u64).checked_add(target_vcn - vcn)?;
            let at = lcn_at.checked_mul(bytes_per_cluster)?.checked_add(within)?;
            let mut buf = vec![0u8; rec_size];
            vol.read_at(at, &mut buf).ok()?;
            if !apply_fixup(&mut buf, bytes_per_sector) {
                return None;
            }
            return Some(buf);
        }
        vcn = end;
    }
    None
}

/// 解析阶段的条目。`name` 是临时字段——`build_tree` 会把名字灌入
/// `name_pool` 并清空它，之后 `Entry` 就只剩定长字段。
/// 解析阶段 `name` 占 24 字节栈空间 + 堆分配；建树后释放。
#[derive(Clone, Default)]
pub(super) struct Entry {
    pub(super) parent: u32,
    pub(super) name_off: u32,
    pub(super) name_len: u16,
    pub(super) is_dir: bool,
    pub(super) used: bool,
    pub(super) size: u64,
    pub(super) base_ref: u32,
    pub(super) mtime: u64,
    /// 临时：解析阶段存名字，build_tree 灌入 name_pool 后清空。
    pub(super) name: String,
}

pub(super) fn parse_record(rec: &[u8], out: &mut Entry, links: &mut Vec<(u32, u8)>) -> bool {
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
    let mut mtime: u64 = 0;

    while let Some((atype, alen, non_resident, name_len)) = attr_header(rec, pos) {
        match atype {
            // $STANDARD_INFORMATION (type 0x10)：解析修改时间
            0x10 if !non_resident => {
                let val_off = u16_at(rec, pos + 0x14) as usize;
                let v = pos + val_off;
                // 修改时间在偏移 0x08（跳过创建时间），FILETIME 格式
                if v + 0x10 <= rec.len() {
                    let filetime = u64_at(rec, v + 0x08);
                    // FILETIME → Unix epoch 秒：减去 11644473600（1601→1970），再除以 1e7
                    if filetime > 116_444_736_000_000_000 {
                        mtime = (filetime - 116_444_736_000_000_000) / 10_000_000;
                    }
                }
            }
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
                    // 只认第一段（start_vcn == 0）上记录的总长度。
                    // 这两处读取都落在非常驻属性头（0x40 字节）内部，
                    // attr_header 已经保证了长度，不必再单独判边界。
                    if u64_at(rec, pos + 0x10) == 0 {
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
    out.mtime = mtime;
    true
}
