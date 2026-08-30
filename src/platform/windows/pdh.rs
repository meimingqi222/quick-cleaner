//! 性能计数器（PDH）的最小封装。
//!
//! GPU 忙闲、显存占用、ACPI 热区温度都从这里来：Windows 上这些读数没有
//! 别的免驱动通道，任务管理器读的也是同一批计数器。
//!
//! 两个约束决定了这层的形状：
//!
//! 1. **查询句柄必须跨拍存活**。利用率一类是差值型计数器，两次
//!    `PdhCollectQueryData` 之间的间隔就是统计窗口；每拍新开查询只会拿到
//!    一堆 0 或者错误。所以调用方把 [`PdhQuery`] 存进 static。
//! 2. **通配实例要按字节数问缓冲区**。实例数跟着进程增减，几百条是常态。

use std::collections::HashMap;
use winapi::shared::minwindef::DWORD;
use winapi::um::pdh::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
};

use super::registry::to_wide;

/// 缓冲区不够时 PDH 的返回码（`PDH_MORE_DATA`）。winapi 0.3 没导出。
const PDH_MORE_DATA: i32 = 0x800007D2u32 as i32;
/// 计数器取值有效（`PDH_CSTATUS_VALID_DATA` / `PDH_CSTATUS_NEW_DATA`）。
const PDH_CSTATUS_VALID_DATA: DWORD = 0;
const PDH_CSTATUS_NEW_DATA: DWORD = 1;

/// 一条 PDH 查询，挂着若干个通配计数器。
pub struct PdhQuery {
    handle: PDH_HQUERY,
    counters: Vec<PDH_HCOUNTER>,
}

// SAFETY: PDH 句柄不绑定线程，跨线程使用是 API 明确支持的；调用方还会再套
// 一层 Mutex。采样任务每拍落在后台线程池的哪个线程上并不固定，没有 Send
// 就存不进 static。
unsafe impl Send for PdhQuery {}

impl PdhQuery {
    /// 建查询并挂上全部计数器。**任何一条挂不上就整个失败**：调用方拿到的
    /// 是「这台机器支持这组读数」的承诺，半套数据比没有更难解释。
    pub fn open(paths: &[&str]) -> Option<PdhQuery> {
        let mut handle: PDH_HQUERY = std::ptr::null_mut();
        // SAFETY: 出参是本函数栈上的变量。
        if unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut handle) } != 0 {
            return None;
        }
        let mut counters = Vec::with_capacity(paths.len());
        for path in paths {
            match add_counter(handle, path) {
                Some(counter) => counters.push(counter),
                None => {
                    // SAFETY: handle 刚由 PdhOpenQueryW 建立，还没交出去过。
                    unsafe { PdhCloseQuery(handle) };
                    return None;
                }
            }
        }
        Some(PdhQuery { handle, counters })
    }

    /// 采一拍。差值型计数器的第一拍没有可比的前一拍，`values` 会给空表。
    pub fn collect(&self) -> bool {
        // SAFETY: 句柄由 open 建立，生命周期内一直有效。
        unsafe { PdhCollectQueryData(self.handle) == 0 }
    }

    /// 第 `index` 个计数器当前的实例表：`实例名 → 取值`。
    ///
    /// 同名实例（不同进程同一条引擎）在这里就地相加，省得每个调用方各写一遍。
    pub fn values(&self, index: usize) -> HashMap<String, f64> {
        let Some(counter) = self.counters.get(index) else {
            return HashMap::new();
        };
        let mut out: HashMap<String, f64> = HashMap::new();
        for (name, value) in counter_array(*counter) {
            *out.entry(name).or_insert(0.0) += value;
        }
        out
    }
}

/// 计数器按**英文名**添加：`PdhAddCounterW` 认的是本地化过的计数器名，
/// 中文系统上 `\GPU Engine(*)\...` 会直接找不到。
fn add_counter(query: PDH_HQUERY, path: &str) -> Option<PDH_HCOUNTER> {
    let wide = to_wide(path);
    let mut counter: PDH_HCOUNTER = std::ptr::null_mut();
    // SAFETY: wide 以 NUL 结尾且在调用期间存活，counter 是栈上出参。
    let status = unsafe { PdhAddEnglishCounterW(query, wide.as_ptr(), 0, &mut counter) };
    (status == 0).then_some(counter)
}

/// 通配计数器的当前实例表：`(实例名, 取值)`，可能有重名。
fn counter_array(counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
    let mut size: DWORD = 0;
    let mut count: DWORD = 0;
    // 第一次调用只为问缓冲区大小。不能按固定上限截断——被截掉的正好是最忙
    // 的那个实例时，数字就是错的。
    // SAFETY: 两个出参在栈上，缓冲区显式传空。
    let status = unsafe {
        PdhGetFormattedCounterArrayW(
            counter,
            PDH_FMT_DOUBLE,
            &mut size,
            &mut count,
            std::ptr::null_mut(),
        )
    };
    if status != PDH_MORE_DATA || size == 0 {
        return Vec::new();
    }
    // PDH 把实例名的字符串数据接在数组后面，所以 size 是**字节总数**而不是
    // 元素个数。按元素大小向上取整分配，顺带拿到正确的对齐。
    let items = size as usize / std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>() + 1;
    // SAFETY: PDH_FMT_COUNTERVALUE_ITEM_W 是纯 POD（指针 + 联合体），全零是
    // 合法初值。
    let mut buf: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> = vec![unsafe { std::mem::zeroed() }; items];
    // SAFETY: 缓冲区字节数 >= size，PDH 只在这块内存里写。
    let status = unsafe {
        PdhGetFormattedCounterArrayW(
            counter,
            PDH_FMT_DOUBLE,
            &mut size,
            &mut count,
            buf.as_mut_ptr(),
        )
    };
    if status != 0 {
        return Vec::new();
    }
    buf.iter()
        .take(count as usize)
        .filter(|entry| {
            matches!(
                entry.FmtValue.CStatus,
                PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA
            )
        })
        .filter_map(|entry| {
            let name = read_wide_ptr(entry.szName)?;
            // SAFETY: 取的是刚按 PDH_FMT_DOUBLE 要过的那一支联合体成员。
            Some((name, unsafe { *entry.FmtValue.u.doubleValue() }))
        })
        .collect()
}

/// PDH 给的是指向宽字符串的裸指针，长度未知，读到 NUL 为止。
fn read_wide_ptr(ptr: *mut u16) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let mut len = 0;
    // SAFETY: PDH 保证 szName 指向缓冲区内一段以 NUL 结尾的宽字符串。
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: 长度就是上面数出来的，不含结尾的 NUL。
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    Some(String::from_utf16_lossy(slice))
}
