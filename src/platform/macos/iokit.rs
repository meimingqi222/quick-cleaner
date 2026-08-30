//! IORegistry 属性读取：GPU 利用率与电池信息。
//!
//! 两者都没有公开的 C API：GPU 利用率只存在于 `IOAccelerator` 服务的
//! `PerformanceStatistics` 字典里（活动监视器、asitop 走的也是这条路），
//! 电池的循环次数和设计容量只在 `AppleSmartBattery` 的属性里。所以这里
//! 走通用做法——把服务的属性整包捞成 CFDictionary，再按键名取值。
//!
//! 与 `status.rs` 的 SMC 一样只声明用得到的那几个 C 函数，不引 core-foundation
//! / io-kit-sys：需要的面很小（取字典、读数字/布尔/字符串），引 crate 反而
//! 要连带一整套类型体系。
//!
//! **所有权约定**（CF 的 Get/Create 规则，弄错就是内存泄漏或二次释放）：
//! * `IORegistryEntryCreateCFProperties` 是 Create 规则 → 拿到的字典**要**
//!   `CFRelease`，这里统一由 [`Properties`] 的 `Drop` 负责。
//! * `CFDictionaryGetValue` 是 Get 规则 → 借来的引用**不能** release。
//! * `IOServiceGetMatchingService` / `IOServiceGetMatchingServices` 会消费掉
//!   传入的 matching 字典，无论成败都不用自己释放。

use crate::core::status::{BatteryReading, GpuReading};
use std::ffi::{c_char, c_void, CString};

type CFTypeRef = *const c_void;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingService(port: u32, matcher: *mut c_void) -> u32;
    fn IOServiceGetMatchingServices(port: u32, matcher: *mut c_void, iter: *mut u32) -> i32;
    fn IOIteratorNext(iterator: u32) -> u32;
    fn IOObjectRelease(object: u32) -> i32;
    fn IORegistryEntryCreateCFProperties(
        entry: u32,
        properties: *mut CFTypeRef,
        allocator: CFTypeRef,
        options: u32,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFGetTypeID(cf: CFTypeRef) -> usize;
    fn CFNumberGetTypeID() -> usize;
    fn CFBooleanGetTypeID() -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFDictionaryGetValue(dict: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    fn CFNumberGetValue(number: CFTypeRef, the_type: i32, value_ptr: *mut c_void) -> bool;
    fn CFBooleanGetValue(boolean: CFTypeRef) -> bool;
    fn CFStringCreateWithCString(alloc: CFTypeRef, cstr: *const c_char, encoding: u32)
        -> CFTypeRef;
    fn CFStringGetCString(
        string: CFTypeRef,
        buffer: *mut c_char,
        size: isize,
        encoding: u32,
    ) -> bool;
}

/// `kCFNumberSInt64Type`。所有数值统一按 i64 取——IORegistry 里的整数实际
/// 宽度不定，用最宽的类型接住，CF 会自己做转换。
const CF_NUMBER_SINT64: i32 = 4;
const CF_STRING_UTF8: u32 = 0x0800_0100;

/// 一个 CFString 的 RAII 包装，只为给 `CFDictionaryGetValue` 造键。
struct CFStr(CFTypeRef);

impl CFStr {
    fn new(s: &str) -> Option<Self> {
        let c = CString::new(s).ok()?;
        let raw =
            unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), CF_STRING_UTF8) };
        (!raw.is_null()).then_some(CFStr(raw))
    }
}

impl Drop for CFStr {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}

/// 一个 IORegistry 服务的属性字典。`Drop` 里 release，调用方不用操心。
struct Properties(CFTypeRef);

impl Properties {
    /// 捞取某个服务的全部属性。
    fn of(entry: u32) -> Option<Self> {
        let mut raw: CFTypeRef = std::ptr::null();
        let kr = unsafe { IORegistryEntryCreateCFProperties(entry, &mut raw, std::ptr::null(), 0) };
        (kr == 0 && !raw.is_null()).then_some(Properties(raw))
    }

    fn raw(&self) -> CFTypeRef {
        self.0
    }
}

impl Drop for Properties {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}

/// 从字典里按键名借一个值出来（Get 规则，不能 release）。
fn value_of(dict: CFTypeRef, key: &str) -> Option<CFTypeRef> {
    if dict.is_null() {
        return None;
    }
    let key = CFStr::new(key)?;
    let v = unsafe { CFDictionaryGetValue(dict, key.0) };
    (!v.is_null()).then_some(v)
}

fn number(dict: CFTypeRef, key: &str) -> Option<i64> {
    let v = value_of(dict, key)?;
    if unsafe { CFGetTypeID(v) != CFNumberGetTypeID() } {
        return None;
    }
    let mut out: i64 = 0;
    let ok = unsafe { CFNumberGetValue(v, CF_NUMBER_SINT64, &mut out as *mut i64 as *mut c_void) };
    ok.then_some(out)
}

fn boolean(dict: CFTypeRef, key: &str) -> Option<bool> {
    let v = value_of(dict, key)?;
    if unsafe { CFGetTypeID(v) != CFBooleanGetTypeID() } {
        return None;
    }
    Some(unsafe { CFBooleanGetValue(v) })
}

fn string(dict: CFTypeRef, key: &str) -> Option<String> {
    let v = value_of(dict, key)?;
    if unsafe { CFGetTypeID(v) != CFStringGetTypeID() } {
        return None;
    }
    let mut buf = [0i8; 256];
    let ok = unsafe { CFStringGetCString(v, buf.as_mut_ptr(), buf.len() as isize, CF_STRING_UTF8) };
    if !ok {
        return None;
    }
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| *b as u8)
        .collect();
    String::from_utf8(bytes).ok().filter(|s| !s.is_empty())
}

fn sub_dict(dict: CFTypeRef, key: &str) -> Option<CFTypeRef> {
    let v = value_of(dict, key)?;
    (unsafe { CFGetTypeID(v) == CFDictionaryGetTypeID() }).then_some(v)
}

// ------------------------------------------------------------------- GPU

/// GPU 利用率。
///
/// 数据来自 `IOAccelerator` 服务的 `PerformanceStatistics` 字典——这是活动
/// 监视器和 asitop 用的同一个来源，没有别的公开通道。机器上可能挂着多个
/// accelerator（独显 + 核显、或者虚拟的），**全都列出来**交给 UI 切换，
/// 而不是替用户挑一张。
///
/// 顺序就是 IORegistry 的枚举顺序，稳定；`id` 用序号，UI 靠它记住用户的
/// 选择。读不出利用率的条目（虚拟 accelerator）跳过：它们在切换按钮里只会
/// 挤掉真显卡的位置。
pub fn read_gpus() -> Vec<GpuReading> {
    let mut gpus = Vec::new();
    unsafe {
        let matcher = IOServiceMatching(c"IOAccelerator".as_ptr());
        if matcher.is_null() {
            return gpus;
        }
        let mut iter: u32 = 0;
        // matcher 会被消费掉，失败也不用自己释放。
        if IOServiceGetMatchingServices(0, matcher, &mut iter) != 0 || iter == 0 {
            return gpus;
        }
        loop {
            let entry = IOIteratorNext(iter);
            if entry == 0 {
                break;
            }
            if let Some(props) = Properties::of(entry) {
                let dict = props.raw();
                if let Some(stats) = sub_dict(dict, "PerformanceStatistics") {
                    let util = number(stats, "Device Utilization %")
                        // 老驱动没有 Device Utilization %，退到渲染器占用。
                        .or_else(|| number(stats, "Renderer Utilization %"))
                        .map(|v| v.clamp(0, 100) as f32);
                    if let Some(util) = util {
                        gpus.push(GpuReading {
                            id: format!("{}", gpus.len()),
                            name: string(dict, "model").or_else(|| string(dict, "IOClass")),
                            utilization: Some(util),
                            renderer_utilization: number(stats, "Renderer Utilization %")
                                .map(|v| v.clamp(0, 100) as f32),
                            vram_in_use: number(stats, "In use system memory")
                                .filter(|v| *v > 0)
                                .map(|v| v as u64),
                            // IOAccelerator 不提供温度；macOS 的芯片温度走
                            // SMC（见 status.rs 的 read_thermal）。
                            temp_c: None,
                        });
                    }
                }
            }
            IOObjectRelease(entry);
        }
        IOObjectRelease(iter);
    }
    gpus
}

// ------------------------------------------------------------------ 电池

/// 电池信息。台式机（无 `AppleSmartBattery` 服务）返回 `None`。
pub fn read_battery() -> Option<BatteryReading> {
    let props = unsafe {
        let matcher = IOServiceMatching(c"AppleSmartBattery".as_ptr());
        if matcher.is_null() {
            return None;
        }
        let service = IOServiceGetMatchingService(0, matcher);
        if service == 0 {
            return None;
        }
        let props = Properties::of(service);
        IOObjectRelease(service);
        props?
    };
    let dict = props.raw();

    if boolean(dict, "BatteryInstalled") == Some(false) {
        return None;
    }

    // CurrentCapacity / MaxCapacity 的单位随机型而异：Apple Silicon 上直接
    // 是百分比（MaxCapacity 恒为 100），Intel 上是 mAh。取比值对两者都成立。
    let current = number(dict, "CurrentCapacity")?;
    let max = number(dict, "MaxCapacity").filter(|m| *m > 0)?;
    let percent = (current as f32 / max as f32 * 100.0).clamp(0.0, 100.0);

    // 健康度 = 当前满电容量 / 出厂设计容量，与「设置 → 电池 → 最大容量」同口径。
    let health_percent = match (
        number(dict, "AppleRawMaxCapacity").filter(|v| *v > 0),
        number(dict, "DesignCapacity").filter(|v| *v > 0),
    ) {
        (Some(raw_max), Some(design)) => {
            Some((raw_max as f32 / design as f32 * 100.0).clamp(0.0, 100.0))
        }
        _ => None,
    };

    // 充电时是「充满还需」，放电时是「还能用」。65535 是驱动的「暂不可知」哨兵
    // （刚插拔电源时会持续几十秒），不能当成 45 天。
    let minutes_remaining = number(dict, "TimeRemaining")
        .filter(|m| *m > 0 && *m < 65535)
        .map(|m| m as u32);

    Some(BatteryReading {
        percent,
        charging: boolean(dict, "IsCharging").unwrap_or(false),
        external: boolean(dict, "ExternalConnected").unwrap_or(false),
        fully_charged: boolean(dict, "FullyCharged").unwrap_or(false),
        cycle_count: number(dict, "CycleCount").map(|v| v.max(0) as u32),
        design_cycle_count: number(dict, "DesignCycleCount9C")
            .filter(|v| *v > 0)
            .map(|v| v as u32),
        health_percent,
        // 百分之一摄氏度。
        temp_c: number(dict, "Temperature")
            .filter(|t| *t > 0)
            .map(|t| t as f32 / 100.0),
        minutes_remaining,
    })
}
