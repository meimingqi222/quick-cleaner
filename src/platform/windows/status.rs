//! Windows 状态采集：电池、运行时长与结束进程。温度 / 风扇读数暂无
//! 不依赖驱动的通用方案（WMI 的 MSAcpi 枚举大多被 OEM 关闭），
//! 如实返回空，由 UI 显示「不可用」。GPU 在 [`super::gpu`]。

use crate::core::status::{BatteryReading, FanError, ThermalReading};
use winapi::shared::minwindef::FILETIME;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::handleapi::CloseHandle;
use winapi::um::processthreadsapi::{GetProcessTimes, OpenProcess, TerminateProcess};
use winapi::um::winbase::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use winapi::um::winnt::{HANDLE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE};

/// `SYSTEM_POWER_STATUS.ACLineStatus`：接着市电。
const AC_LINE_ONLINE: u8 = 1;
/// `SYSTEM_POWER_STATUS.BatteryFlag`：正在充电。
const BATTERY_FLAG_CHARGING: u8 = 8;
/// `SYSTEM_POWER_STATUS.BatteryFlag`：这台机器没有电池。
const BATTERY_FLAG_NO_BATTERY: u8 = 128;

pub fn read_thermal() -> ThermalReading {
    super::thermal::read_thermal()
}

pub fn read_gpus() -> Vec<crate::core::status::GpuReading> {
    super::gpu::read_gpus()
}

/// 电池读数。台式机（`BatteryFlag` 带 [`BATTERY_FLAG_NO_BATTERY`]）返回
/// `None`，那台机器上整张电池卡片不渲染。
///
/// 只有 `GetSystemPowerStatus` 这一个不依赖驱动的通道，它给的是「电量 /
/// 在充没充 / 还能用多久」。循环次数和健康度要走
/// `IOCTL_BATTERY_QUERY_INFORMATION`（还得先 SetupDi 枚举电池设备接口），
/// winapi 0.3 里没有对应的结构体和控制码，这里如实留 `None`——宁可少两行
/// 脚注，也不拿别的数字冒充健康度。
pub fn read_battery() -> Option<BatteryReading> {
    // SAFETY: 出参是本函数栈上的结构体，API 只写不留引用。
    let mut power: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    if unsafe { GetSystemPowerStatus(&mut power) } == 0 {
        return None;
    }
    if power.BatteryFlag & BATTERY_FLAG_NO_BATTERY != 0 {
        return None;
    }
    // >100 = 驱动说不准（哨兵值 255）。没有电量就没有这张卡的主读数，
    // 整张卡不渲染，好过画一根 255% 的进度条。
    if power.BatteryLifePercent > 100 {
        return None;
    }
    let percent = power.BatteryLifePercent as f32;
    let charging = power.BatteryFlag & BATTERY_FLAG_CHARGING != 0;
    let external = power.ACLineStatus == AC_LINE_ONLINE;
    Some(BatteryReading {
        percent,
        charging,
        external,
        // Windows 不直接说「充满了」：接着电、又没在充、电量顶格，就是充满。
        fully_charged: external && !charging && percent >= 100.0,
        cycle_count: None,
        design_cycle_count: None,
        health_percent: None,
        temp_c: None,
        // 放电时是「还能用多久」，接上电源后系统给的是 0xFFFFFFFF（未知）。
        minutes_remaining: (power.BatteryLifeTime != u32::MAX)
            .then_some(power.BatteryLifeTime / 60),
    })
}

pub fn system_uptime_secs() -> u64 {
    // GetTickCount64 自系统启动起累计毫秒，不含休眠时间的口径差异可忽略。
    unsafe { winapi::um::sysinfoapi::GetTickCount64() / 1000 }
}

pub fn process_unique_id(_pid: u32) -> Option<u64> {
    None
}

pub fn terminate_process(
    pid: u32,
    expected_start_time: u64,
    _unique_id: Option<u64>,
) -> Result<(), String> {
    // 身份判定必须挂在**句柄**上而不是「先按 PID 查一遍、再单独
    // OpenProcess」：两步之间目标若退出且 PID 被复用，按 PID 打开的
    // 句柄就已经指向别的进程了。先开句柄，句柄绑定的进程不会再变，
    // 再用同一句柄核对创建时间，最后终止。残余限制：sysinfo 侧的
    // start_time 只有秒级精度，「同一秒内完成退出-复用」的极端情形
    // 仍无法区分，这是没有 pidfd 类机制的平台能做到的上限。
    unsafe {
        let handle: HANDLE = OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        );
        if handle.is_null() {
            return Err(format!(
                "OpenProcess({pid}) failed (GetLastError={})",
                GetLastError()
            ));
        }
        let result = terminate_same_process(handle, pid, expected_start_time);
        CloseHandle(handle);
        result
    }
}

/// 句柄到手后：同一句柄核对创建时间，再终止。
fn terminate_same_process(
    handle: HANDLE,
    pid: u32,
    expected_start_time: u64,
) -> Result<(), String> {
    let Some(actual) = handle_start_time_secs(handle) else {
        return Err(format!("PID {pid} 创建时间读取失败，拒绝终止"));
    };
    if actual != expected_start_time {
        return Err(format!("PID {pid} 已退出或已被其他进程复用"));
    }
    unsafe {
        let ok = TerminateProcess(handle, 1);
        if ok == 0 {
            Err(format!(
                "TerminateProcess({pid}) failed (GetLastError={})",
                GetLastError()
            ))
        } else {
            Ok(())
        }
    }
}

/// 句柄对应进程的创建时间，Unix 纪元秒。
///
/// 换算公式与 sysinfo 的 `start_time()` 完全一致（FILETIME 100ns 计数
/// 除以 10⁷ 再减 Windows/Unix 纪元差），两边才能直接比较。
fn handle_start_time_secs(handle: HANDLE) -> Option<u64> {
    let mut created: FILETIME = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut ignored: FILETIME = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    // SAFETY: 四个参数都是本函数栈上的输出缓冲。
    let ok = unsafe {
        GetProcessTimes(
            handle,
            &mut created,
            &mut ignored,
            &mut ignored,
            &mut ignored,
        )
    };
    if ok == 0 {
        return None;
    }
    let ticks = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
    Some(ticks / 10_000_000 - 11_644_473_600)
}

/// Windows 上转速是**只读**的：改档位要么走厂商驱动，要么直接写 EC 端口，
/// 两条都得装内核驱动。UI 靠这个开关决定不画档位按钮，而不是画完再报错。
pub fn fan_control_supported() -> bool {
    false
}

pub fn set_fan_mode(_mode: crate::core::status::FanMode) -> Result<(), FanError> {
    // Windows 没有不依赖厂商驱动的风扇控制通道（WMI 的 MSAcpi 大多被 OEM 关闭）。
    Err(FanError::Other("当前平台不支持风扇控制".into()))
}

pub fn elevated_fan_control(_mode: crate::core::status::FanMode) -> Result<(), FanError> {
    Err(FanError::Other("当前平台不支持风扇控制".into()))
}

pub fn fan_helper_installed() -> bool {
    false
}

pub fn install_fan_helper(_prompt: &str) -> Result<(), FanError> {
    Err(FanError::Other("当前平台不支持风扇控制".into()))
}

pub fn uninstall_fan_helper(_prompt: &str) -> Result<(), FanError> {
    Err(FanError::Other("当前平台不支持风扇控制".into()))
}
