//! NVIDIA 显卡温度（NVML）。
//!
//! `nvml.dll` 是显卡驱动的一部分，装了 N 卡驱动的机器上就在 `System32`，
//! 不需要管理员、不需要额外 SDK、也不需要装驱动——这是 Windows 上少数几个
//! 「免费就能拿到的真实温度」之一（`nvidia-smi` 读的也是它）。
//!
//! 只取温度：
//! - 利用率和显存已经从 PDH 拿到了，两处口径不一致反而让人怀疑数字。
//! - 风扇转速（`nvmlDeviceGetFanSpeed`）在笔记本上普遍返回 NOT_SUPPORTED，
//!   独显风扇归 EC 管，不归显卡驱动管。
//!
//! 动态加载而不是链接导入库：没有 N 卡的机器上 `nvml.dll` 根本不存在，
//! 静态链接会让整个程序起不来。

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::OnceLock;
use winapi::shared::minwindef::HMODULE;
use winapi::um::libloaderapi::{GetProcAddress, LoadLibraryW};

use super::registry::to_wide;

/// `nvmlReturn_t` 的成功值。
const NVML_SUCCESS: i32 = 0;
/// `NVML_TEMPERATURE_GPU`：芯片本体温度（另一个是显存，多数卡不支持）。
const NVML_TEMPERATURE_GPU: u32 = 0;
/// `NVML_DEVICE_NAME_BUFFER_SIZE`。
const NAME_BUFFER: usize = 96;

type NvmlDevice = *mut c_void;

/// 已经解析好的 NVML 入口。
struct Nvml {
    device_count: unsafe extern "C" fn(*mut u32) -> i32,
    handle_by_index: unsafe extern "C" fn(u32, *mut NvmlDevice) -> i32,
    device_name: unsafe extern "C" fn(NvmlDevice, *mut u8, u32) -> i32,
    temperature: unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> i32,
}

/// 一张 N 卡：型号名 + 设备句柄。句柄在进程生命周期内有效，枚举一次即可。
struct Device {
    name: String,
    handle: NvmlDevice,
}

// SAFETY: NVML 句柄是驱动侧的不透明指针，官方文档明确允许跨线程使用；
// 这里也只在采样线程上读。
unsafe impl Send for Device {}
unsafe impl Sync for Device {}

/// 型号名 → 当前温度（摄氏度）。没有 N 卡、驱动太老、读不到都返回空表。
///
/// 按**型号名**而不是索引对外：PDH 那边只有 LUID，NVML 这边只有 PCI 地址，
/// 两套编号没有公开的对照关系，能对上的只有 DXGI 的型号名。同型号双卡会
/// 认错，但那是工作站配置，笔记本上不会遇到。
pub fn gpu_temperatures() -> HashMap<String, f32> {
    let mut out = HashMap::new();
    let Some((nvml, devices)) = state() else {
        return out;
    };
    for device in devices {
        let mut celsius: u32 = 0;
        // SAFETY: 句柄来自 nvmlDeviceGetHandleByIndex，出参在栈上。
        let status =
            unsafe { (nvml.temperature)(device.handle, NVML_TEMPERATURE_GPU, &mut celsius) };
        // 上限挡住驱动偶发的哨兵值（0xFFFFFFFF 之类）。
        if status == NVML_SUCCESS && celsius > 0 && celsius < 150 {
            out.insert(device.name.clone(), celsius as f32);
        }
    }
    out
}

/// 加载 NVML 并枚举设备，只做一次。
fn state() -> Option<&'static (Nvml, Vec<Device>)> {
    static STATE: OnceLock<Option<(Nvml, Vec<Device>)>> = OnceLock::new();
    STATE.get_or_init(load).as_ref()
}

fn load() -> Option<(Nvml, Vec<Device>)> {
    let name = to_wide("nvml.dll");
    // SAFETY: name 以 NUL 结尾。找不到 DLL 返回空句柄，不是 UB。
    let library = unsafe { LoadLibraryW(name.as_ptr()) };
    if library.is_null() {
        return None;
    }
    // 带 `_v2` 后缀的是现行版本；不做无后缀回退，那是 2012 年前的驱动。
    let nvml = Nvml {
        device_count: unsafe { symbol(library, c"nvmlDeviceGetCount_v2")? },
        handle_by_index: unsafe { symbol(library, c"nvmlDeviceGetHandleByIndex_v2")? },
        device_name: unsafe { symbol(library, c"nvmlDeviceGetName")? },
        temperature: unsafe { symbol(library, c"nvmlDeviceGetTemperature")? },
    };
    let init: unsafe extern "C" fn() -> i32 = unsafe { symbol(library, c"nvmlInit_v2")? };
    // SAFETY: 无参数入口，失败只返回错误码。
    if unsafe { init() } != NVML_SUCCESS {
        return None;
    }
    // 不配对 nvmlShutdown：库和设备句柄要活到进程结束，中途关掉下一拍就读
    // 不到了。进程退出时由系统回收。

    let mut count: u32 = 0;
    // SAFETY: 出参在栈上。
    if unsafe { (nvml.device_count)(&mut count) } != NVML_SUCCESS {
        return None;
    }
    let mut devices = Vec::new();
    for index in 0..count {
        let mut handle: NvmlDevice = std::ptr::null_mut();
        // SAFETY: 出参在栈上，index 在 count 范围内。
        if unsafe { (nvml.handle_by_index)(index, &mut handle) } != NVML_SUCCESS {
            continue;
        }
        let mut buffer = [0u8; NAME_BUFFER];
        // SAFETY: 缓冲区大小如实告知，NVML 只写这么多并补 NUL。
        let status = unsafe { (nvml.device_name)(handle, buffer.as_mut_ptr(), NAME_BUFFER as u32) };
        if status != NVML_SUCCESS {
            continue;
        }
        let end = buffer.iter().position(|b| *b == 0).unwrap_or(NAME_BUFFER);
        let name = String::from_utf8_lossy(&buffer[..end]).into_owned();
        if !name.is_empty() {
            devices.push(Device { name, handle });
        }
    }
    (!devices.is_empty()).then_some((nvml, devices))
}

/// 取一个导出函数并转成指定的函数类型。
///
/// # Safety
///
/// 调用方要保证 `T` 和该导出函数的真实签名一致——签名写错不会报错，
/// 会在调用时踩栈。
unsafe fn symbol<T: Copy>(library: HMODULE, name: &std::ffi::CStr) -> Option<T> {
    debug_assert_eq!(
        std::mem::size_of::<T>(),
        std::mem::size_of::<*const c_void>()
    );
    let address = GetProcAddress(library, name.as_ptr());
    if address.is_null() {
        return None;
    }
    Some(*(&address as *const _ as *const T))
}
