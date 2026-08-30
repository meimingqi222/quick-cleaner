//! macOS 状态采集：SMC 风扇转速与 CPU 温度、系统运行时长、结束进程。
//!
//! SMC 走 IOKit 的 AppleSMC user client，协议与 smc-command 参考实现一致：
//! `IOConnectCallStructMethod` 的 selector 固定为 `KERNEL_INDEX_SMC = 2`，
//! 具体操作由 `data8` 命令字决定：状态采集使用 `READ_KEYINFO(9)` 和
//! `READ_BYTES(5)`；风扇控制还会发送 `WRITE_BYTES(6)`，但写入面仅限
//! `Ftst`、每颗风扇的模式键和目标转速键。

use crate::core::status::{FanInfo, ThermalReading};
use objc::runtime::Object;
use objc::{class, msg_send, sel, sel_impl};
use std::ffi::{c_char, c_void};
use std::sync::Mutex;

// ---- IOKit（SMC 只需要这一小撮 C 函数，不值得为此引入 objc2/io-kit-sys） ----

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingService(port: u32, matcher: *mut c_void) -> u32;
    fn IOServiceOpen(service: u32, owning_task: u32, kind: u32, connect: *mut u32) -> i32;
    fn IOServiceClose(connect: u32) -> i32;
    fn IOObjectRelease(object: u32) -> i32;
    fn IOConnectCallStructMethod(
        connection: u32,
        selector: u32,
        input: *const SmcKeyData,
        input_size: usize,
        output: *mut SmcKeyData,
        output_size: *mut usize,
    ) -> i32;
}

// libc 0.2 把这个符号标了 deprecated（推荐 mach2 crate），为一个小符号
// 引依赖不值当，直接声明 libSystem 里本来就有的 C 函数。
extern "C" {
    fn mach_task_self() -> u32;
}

const KERNEL_INDEX_SMC: u32 = 2;
const SMC_CMD_READ_BYTES: u8 = 5;
const SMC_CMD_WRITE_BYTES: u8 = 6;
const SMC_CMD_READ_KEYINFO: u8 = 9;

/// 类型标识：`'fpe2'`（风扇转速，定点 /4）、`'sp78'`（温度，定点 /256）、
/// `'ui8 '`（单字节无符号，风扇个数）、`'flt '`（IEEE754 小端 float，
/// Apple Silicon 上的传感器通用类型）。
const TYPE_FPE2: u32 = u32::from_be_bytes(*b"fpe2");
const TYPE_SP78: u32 = u32::from_be_bytes(*b"sp78");
const TYPE_UI8: u32 = u32::from_be_bytes(*b"ui8 ");
const TYPE_FLT: u32 = u32::from_be_bytes(*b"flt ");

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct SmcVers {
    major: u8,
    minor: u8,
    build: u8,
    reserved: [u8; 1],
    release: u16,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct SmcPLimit {
    version: u16,
    length: u16,
    cpu_plimit: u32,
    gpu_plimit: u32,
    mem_plimit: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct SmcKeyInfo {
    data_size: u32,
    data_type: u32,
    data_attributes: u8,
}

/// 与 AppleSMC user client 约定的 80 字节结构，字段顺序必须一字不差。
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct SmcKeyData {
    key: u32,
    vers: SmcVers,
    p_limit_data: SmcPLimit,
    key_info: SmcKeyInfo,
    result: u8,
    status: u8,
    data8: u8,
    data32: u32,
    bytes: [u8; 32],
}

/// 四字符键名转 u32（SMC 协议里键就是大端四字节）。
fn key_code(name: &str) -> u32 {
    let mut bytes = [b' '; 4];
    for (dst, src) in bytes.iter_mut().zip(name.as_bytes()) {
        *dst = *src;
    }
    u32::from_be_bytes(bytes)
}

/// 打开 AppleSMC 连接。失败（无 SMC、沙盒拦截等）返回 None。
fn smc_open() -> Option<u32> {
    unsafe {
        let matcher = IOServiceMatching(c"AppleSMC".as_ptr());
        if matcher.is_null() {
            return None;
        }
        // IOServiceGetMatchingService 会消费掉 matcher（哪怕失败），无须 release。
        let device = IOServiceGetMatchingService(0, matcher);
        if device == 0 {
            return None;
        }
        let mut conn: u32 = 0;
        let kr = IOServiceOpen(device, mach_task_self(), 0, &mut conn);
        IOObjectRelease(device);
        if kr == 0 && conn != 0 {
            Some(conn)
        } else {
            None
        }
    }
}

/// 读一个 SMC 键：先查键信息拿类型和长度，再读字节。任何一步失败都返回 None。
fn smc_read(conn: u32, key: &str) -> Option<(u32, usize, [u8; 32])> {
    let code = key_code(key);
    unsafe {
        let mut input = SmcKeyData {
            key: code,
            data8: SMC_CMD_READ_KEYINFO,
            ..Default::default()
        };
        let mut output = SmcKeyData::default();
        let mut out_size = std::mem::size_of::<SmcKeyData>();
        let kr = IOConnectCallStructMethod(
            conn,
            KERNEL_INDEX_SMC,
            &input,
            std::mem::size_of::<SmcKeyData>(),
            &mut output,
            &mut out_size,
        );
        if kr != 0 || output.result != 0 {
            return None;
        }
        let (data_size, data_type) = (output.key_info.data_size, output.key_info.data_type);
        if data_size == 0 || data_size > 32 {
            return None;
        }

        input = SmcKeyData {
            key: code,
            key_info: SmcKeyInfo {
                data_size,
                data_type,
                data_attributes: 0,
            },
            data8: SMC_CMD_READ_BYTES,
            ..Default::default()
        };
        let mut output = SmcKeyData::default();
        let mut out_size = std::mem::size_of::<SmcKeyData>();
        let kr = IOConnectCallStructMethod(
            conn,
            KERNEL_INDEX_SMC,
            &input,
            std::mem::size_of::<SmcKeyData>(),
            &mut output,
            &mut out_size,
        );
        if kr != 0 || output.result != 0 {
            return None;
        }
        Some((data_type, data_size as usize, output.bytes))
    }
}

/// 按类型把原始字节换算成数值。Intel 时代是定点数（`fpe2`/`sp78`），
/// Apple Silicon 上大部分传感器键换成了 IEEE754 小端 float（`flt `），
/// 同一份代码两代机器都要能读。
fn decode_scalar(data_type: u32, bytes: &[u8; 32], size: usize) -> Option<f32> {
    match (data_type, size) {
        (TYPE_FPE2, n) if n >= 2 => Some(u16::from_be_bytes([bytes[0], bytes[1]]) as f32 / 4.0),
        (TYPE_SP78, n) if n >= 2 => Some(i16::from_be_bytes([bytes[0], bytes[1]]) as f32 / 256.0),
        (TYPE_FLT, 4) => Some(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
        (TYPE_UI8, 1) => Some(bytes[0] as f32),
        _ => None,
    }
}

/// Apple Silicon 的 P 核簇温度键。不能像早期实现那样取第一个可读值：
/// 簇间负载可能不均，风扇控制必须以最高的可用温度为准。
const APPLE_CPU_TEMP_KEYS: [&str; 4] = ["Tp01", "Tp05", "Tp09", "Tp0D"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CpuTempProfile {
    IntelPackage,
    AppleSiliconPCore,
}

#[derive(Clone, Copy, Debug)]
struct CpuTempSample {
    celsius: f32,
    profile: CpuTempProfile,
}

/// Foundation 提供的整机热压力。它汇总了系统掌握的多传感器信息，适合做
/// 手动风扇控制的安全闸门；但只有四档，不适合单独拿来生成连续风扇曲线。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SystemThermalState {
    Nominal,
    Fair,
    Serious,
    Critical,
}

/// macOS 走 SMC 直写（必要时经特权守护进程），档位是可以改的。
pub fn fan_control_supported() -> bool {
    true
}

pub fn read_thermal() -> ThermalReading {
    let mut reading = ThermalReading::default();
    let Some(conn) = smc_open() else {
        return reading;
    };
    // 无论读成什么样都关闭连接：SMC 打开是每两秒一次的高频操作。
    let _guard = smc_close_guard(conn);

    if let Some((t, size, bytes)) = smc_read(conn, "FNum") {
        if let Some(count) = decode_scalar(t, &bytes, size) {
            let count = (count as usize).min(4); // 桌面机也没有超过 4 个独立风扇的型号
            for i in 0..count {
                let key = format!("F{i}Ac");
                if let Some((t, size, bytes)) = smc_read(conn, &key) {
                    if let Some(rpm) =
                        decode_scalar(t, &bytes, size).filter(|rpm| (0.0..20_000.0).contains(rpm))
                    {
                        reading.fans.push(FanInfo {
                            label: format!("Fan {}", i + 1),
                            rpm,
                        });
                    }
                }
            }
        }
    }
    reading.cpu_temp = cpu_temp_sample_on(conn).map(|sample| sample.celsius);
    reading
}

/// 在既有 SMC 连接上读 CPU 温度。风扇控制的安全策略挂在这个读数上
/// （见 [`effective_duty`]），所以它必须能在写风扇的同一条连接上取到，
/// 不能再开一条。
fn read_valid_temp(conn: u32, key: &str) -> Option<f32> {
    smc_read(conn, key)
        .and_then(|(t, size, bytes)| decode_scalar(t, &bytes, size))
        .filter(|t| (0.0..150.0).contains(t))
}

fn cpu_temp_sample_on(conn: u32) -> Option<CpuTempSample> {
    // Intel 的 TC0P 是封装温度，刻度与 Apple Silicon P 核簇不同，必须使用
    // 单独的曲线。Apple Silicon 上没有 TC0P，再对所有 P 核簇取最高值。
    if let Some(celsius) = read_valid_temp(conn, "TC0P") {
        return Some(CpuTempSample {
            celsius,
            profile: CpuTempProfile::IntelPackage,
        });
    }
    APPLE_CPU_TEMP_KEYS
        .iter()
        .filter_map(|key| read_valid_temp(conn, key))
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|celsius| CpuTempSample {
            celsius,
            profile: CpuTempProfile::AppleSiliconPCore,
        })
}

fn system_thermal_state() -> Option<SystemThermalState> {
    unsafe {
        let info: *mut Object = msg_send![class!(NSProcessInfo), processInfo];
        if info.is_null() {
            return None;
        }
        let raw: isize = msg_send![info, thermalState];
        match raw {
            0 => Some(SystemThermalState::Nominal),
            1 => Some(SystemThermalState::Fair),
            2 => Some(SystemThermalState::Serious),
            3 => Some(SystemThermalState::Critical),
            _ => None,
        }
    }
}

/// IOServiceClose 的 RAII 兜底：早退路径也不漏关连接。
fn smc_close_guard(conn: u32) -> impl Drop {
    struct Guard(u32);
    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe {
                IOServiceClose(self.0);
            }
        }
    }
    Guard(conn)
}

pub fn system_uptime_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    unsafe {
        let mut boottime: libc::timeval = std::mem::zeroed();
        let mut size = std::mem::size_of::<libc::timeval>();
        let name = c"kern.boottime";
        let ok = libc::sysctlbyname(
            name.as_ptr(),
            &mut boottime as *mut libc::timeval as *mut c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        );
        if ok == 0 && boottime.tv_sec > 0 {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            return now.saturating_sub(boottime.tv_sec as u64);
        }
    }
    0
}

/// Darwin 在一次启动内给每个进程分配的 `p_uniqueid`，PID 复用后会变。
///
/// macOS 没有 pidfd / 可绑定的进程句柄：`kill(2)` 始终按 PID 投递。
/// 这个值让「同一秒内退出-复用」在**检查当下**能被拆穿；检查与 `kill`
/// 之间的 TOCTOU 关不掉，所以拿不到 uniqueid 时拒绝结束，而不是退回
/// 秒级 start_time 假装已经避免了复用。
pub fn process_unique_id(pid: u32) -> Option<u64> {
    #[repr(C)]
    struct ProcUniqIdentifierInfo {
        p_uuid: [u8; 16],
        p_uniqueid: u64,
        p_puniqueid: u64,
        p_idversion: u64,
        p_reserve2: u64,
        p_reserve3: u64,
    }
    const PROC_PIDUNIQIDENTIFIERINFO: i32 = 17;
    extern "C" {
        fn proc_pidinfo(
            pid: i32,
            flavor: i32,
            arg: u64,
            buffer: *mut c_void,
            buffersize: i32,
        ) -> i32;
    }
    let mut info = ProcUniqIdentifierInfo {
        p_uuid: [0; 16],
        p_uniqueid: 0,
        p_puniqueid: 0,
        p_idversion: 0,
        p_reserve2: 0,
        p_reserve3: 0,
    };
    let size = std::mem::size_of::<ProcUniqIdentifierInfo>() as i32;
    let n = unsafe {
        proc_pidinfo(
            pid as i32,
            PROC_PIDUNIQIDENTIFIERINFO,
            0,
            &mut info as *mut ProcUniqIdentifierInfo as *mut c_void,
            size,
        )
    };
    (n == size && info.p_uniqueid != 0).then_some(info.p_uniqueid)
}

pub fn terminate_process(
    pid: u32,
    _expected_start_time: u64,
    expected_unique_id: Option<u64>,
) -> Result<(), String> {
    let Some(expected_unique_id) = expected_unique_id else {
        return Err(format!("PID {pid} 缺少 Darwin 进程标识，拒绝结束"));
    };
    let Some(actual_unique_id) = process_unique_id(pid) else {
        return Err(format!("PID {pid} 已退出或无法读取进程标识"));
    };
    if actual_unique_id != expected_unique_id {
        return Err(format!("PID {pid} 已退出或已被其他进程复用"));
    }
    // 残余限制：kill(2) 仍按 PID 投递。uniqueid 在发出信号前再读一次，
    // 把窗口缩到两次 syscalls 之间；无法做到 Windows 那种句柄绑定。
    let Some(again) = process_unique_id(pid) else {
        return Err(format!("PID {pid} 已退出或已被其他进程复用"));
    };
    if again != expected_unique_id {
        return Err(format!("PID {pid} 已退出或已被其他进程复用"));
    }
    let rc = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}
// ---- 风扇控制 ----
//
// 新款 Mac 只提供「模式键 + F{i}Tg 手动接管」这一条控制通道：`F{i}Mn`
// （smcFanControl 那套「只抬高最小转速、系统仍可自行往上调」的做法）在
// Apple Silicon 上是只读的——本机以 root 实测写入被固件拒绝（result=0x86）。
//
// 手动接管意味着**系统的多传感器调速被完全取代**，所以一个固定的中间档
// （比如恒定 60%）在高负载时会低于系统本来想要的转速。中间档要能安全提供，
// 必须自己补上「热了就升档」这条：见 [`effective_duty`]。
//
// 另一侧的下界由固件自己的 `F{i}Mn` 兜住：任何目标值都夹进 [Mn, Mx]，
// 所以即使有人从协议层送来一个荒谬的低档，也不会低于系统的怠速下限。

use crate::core::status::{FanError, FanMode};

/// 按类型把数值编码成 SMC 原始字节（与 `decode_scalar` 互逆）。
fn encode_scalar(data_type: u32, value: f32) -> Option<(usize, [u8; 32])> {
    let mut bytes = [0u8; 32];
    match data_type {
        TYPE_FPE2 => {
            let v = (value * 4.0).round().clamp(0.0, u16::MAX as f32) as u16;
            bytes[0..2].copy_from_slice(&v.to_be_bytes());
            Some((2, bytes))
        }
        TYPE_SP78 => {
            let v = (value * 256.0)
                .round()
                .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            bytes[0..2].copy_from_slice(&v.to_be_bytes());
            Some((2, bytes))
        }
        TYPE_FLT => {
            bytes[0..4].copy_from_slice(&value.to_le_bytes());
            Some((4, bytes))
        }
        TYPE_UI8 => {
            bytes[0] = value.round().clamp(0.0, 255.0) as u8;
            Some((1, bytes))
        }
        _ => None,
    }
}

/// 写一个 SMC 键。键的类型 / 长度必须与键信息一致，否则驱动会拒绝。
fn smc_write(
    conn: u32,
    key: &str,
    data_type: u32,
    data_size: u32,
    value: f32,
) -> Result<(), FanError> {
    let Some((size, bytes)) = encode_scalar(data_type, value) else {
        return Err(FanError::Other(format!(
            "不支持的 SMC 值类型 {data_type:#x}"
        )));
    };
    if size != data_size as usize {
        return Err(FanError::Other(format!(
            "键 {key} 的长度 {data_size} 与编码长度 {size} 不一致"
        )));
    }
    let input = SmcKeyData {
        key: key_code(key),
        key_info: SmcKeyInfo {
            data_size,
            data_type,
            data_attributes: 0,
        },
        data8: SMC_CMD_WRITE_BYTES,
        bytes,
        ..Default::default()
    };
    let mut output = SmcKeyData::default();
    let kr = unsafe {
        let mut out_size = std::mem::size_of::<SmcKeyData>();
        IOConnectCallStructMethod(
            conn,
            KERNEL_INDEX_SMC,
            &input,
            std::mem::size_of::<SmcKeyData>(),
            &mut output,
            &mut out_size,
        )
    };
    if kr == 0 && output.result == 0 {
        Ok(())
    } else {
        // 0xe00002c1 / 0xe00002c2：SMC 固件按键的属性位拒绝对非特权进程写入。
        let detail = format!("SMC write {key} = {kr:#x}, result={:#x}", output.result);
        let ku = kr as u32;
        if ku == 0xe00002c1
            || ku == 0xe00002c2
            || (kr == 0 && output.result != 0 && unsafe { libc::geteuid() } != 0)
        {
            Err(FanError::NeedsRoot(detail))
        } else {
            Err(FanError::Other(detail))
        }
    }
}

/// 风扇个数；读不到时按 0 处理（不支持风扇控制的机型直接报错）。
fn fan_count(conn: u32) -> usize {
    smc_read(conn, "FNum")
        .and_then(|(t, size, bytes)| decode_scalar(t, &bytes, size))
        .map(|n| (n as usize).min(4))
        .unwrap_or(0)
}

fn restore_auto(conn: u32, count: usize) -> Result<(), FanError> {
    let mut first_error = None;
    for i in 0..count {
        if let Some(key) = fan_mode_key(conn, i) {
            match smc_read(conn, &key) {
                Some((t, sz, _)) => {
                    if let Err(err) = smc_write(conn, &key, t, sz as u32, 0.0) {
                        first_error.get_or_insert(err);
                    }
                }
                None => {
                    first_error.get_or_insert_with(|| FanError::Other(format!("{key} 不可读")));
                }
            }
        }
        let target_key = format!("F{i}Tg");
        if let Some((t, sz, _)) = smc_read(conn, &target_key) {
            if let Err(err) = smc_write(conn, &target_key, t, sz as u32, 0.0) {
                first_error.get_or_insert(err);
            }
        }
    }
    if let Some((t, sz, _)) = smc_read(conn, "Ftst") {
        if let Err(err) = smc_write(conn, "Ftst", t, sz as u32, 0.0) {
            first_error.get_or_insert(err);
        }
    }
    first_error.map_or(Ok(()), Err)
}

pub fn set_fan_mode(mode: FanMode) -> Result<(), FanError> {
    if !mode.is_supported() {
        return Err(FanError::Other("仅支持自动、降温或全速风扇模式".into()));
    }
    let Some(conn) = smc_open() else {
        return Err(FanError::Other("无法打开 AppleSMC 连接".into()));
    };
    let _guard = smc_close_guard(conn);
    let count = fan_count(conn);
    if count == 0 {
        // 无风扇机型上 Auto 本来就是终态；Percent 才是真的做不到。
        // 这里必须让 Auto 成功，否则 helper 启动时 restore_auto 会永远重试。
        return match mode {
            FanMode::Auto => Ok(()),
            FanMode::Percent(_) => Err(FanError::Other("未检测到可控风扇".into())),
        };
    }

    match mode {
        FanMode::Auto => {
            // 把模式交还给系统（0=auto），清空目标转速，再撤掉诊断解锁标志。
            // 任何写入失败都必须返回：普通 GUI 进程会因无 root 权限失败，UI
            // 正是靠这个错误转去停止已提权的守护进程。吞掉错误会让按钮显示
            // “自动”，实际 root 守护进程仍在持续强制转速。
            reset_cooling_control();
            restore_auto(conn, count)
        }
        FanMode::Percent(pct) => {
            let requested = pct.clamp(1, 100) as f32 / 100.0;
            // 全速不会限制散热，不依赖传感器。中间档则必须同时拿到 CPU 温度
            // 和系统热压力；任何信号缺失或进入严重热状态，都先交还系统控制。
            let duty = if requested >= 1.0 {
                reset_cooling_control();
                1.0
            } else {
                let decision = cpu_temp_sample_on(conn)
                    .zip(system_thermal_state())
                    .ok_or("关键温度信号不可用")
                    .and_then(|(sample, thermal)| cooling_duty(requested, sample, thermal));
                match decision {
                    Ok(duty) => duty,
                    Err(reason) => {
                        reset_cooling_control();
                        let restore = restore_auto(conn, count);
                        return match restore {
                            Ok(()) => Err(FanError::Other(format!("{reason}，已恢复系统自动调速"))),
                            Err(err) => Err(FanError::Other(format!(
                                "{reason}，且恢复系统自动调速失败：{err}"
                            ))),
                        };
                    }
                }
            };
            let applied = (|| {
                unlock_fan_control(conn)?;
                for i in 0..count {
                    let mx_key = format!("F{i}Mx");
                    let mn_key = format!("F{i}Mn");
                    let (mx_type, mx_size, mx_bytes) = smc_read(conn, &mx_key)
                        .ok_or_else(|| FanError::Other(format!("{mx_key} 不可读")))?;
                    // 用键信息返回的真实类型解码：Apple Silicon 是 'flt '，Intel 是 'fpe2'。
                    let max = decode_scalar(mx_type, &mx_bytes, mx_size)
                        .ok_or_else(|| FanError::Other(format!("{mx_key} 无法解码")))?;
                    // 下界夹到固件自己的最小转速：`F{i}Mn` 只读，但读得到，
                    // 它就是系统的怠速下限。任何目标都不该低于它。
                    let min = smc_read(conn, &mn_key)
                        .and_then(|(t, sz, b)| decode_scalar(t, &b, sz))
                        .unwrap_or(0.0);
                    let target = (max * duty).clamp(min, max);

                    // 模式键：F%dMd（多数机型）/ F%dmd（M5 一代），启动时探测一次。
                    let mode_key = fan_mode_key(conn, i)
                        .ok_or_else(|| FanError::Other(format!("F{i}Md 不可读")))?;
                    let (mt, msz, mb) = smc_read(conn, &mode_key)
                        .ok_or_else(|| FanError::Other(format!("{mode_key} 不可读")))?;
                    let current = decode_scalar(mt, &mb, msz).unwrap_or(0.0);
                    if current != 1.0 {
                        smc_write(conn, &mode_key, mt, msz as u32, 1.0)?;
                    }
                    let tg_key = format!("F{i}Tg");
                    let (tg_type, tg_size, _) = smc_read(conn, &tg_key)
                        .ok_or_else(|| FanError::Other(format!("{tg_key} 不可读")))?;
                    smc_write(conn, &tg_key, tg_type, tg_size as u32, target)?;
                }
                Ok(())
            })();
            if applied.is_err() {
                reset_cooling_control();
                let _ = restore_auto(conn, count);
            }
            applied
        }
    }
}

/// 只读转储风扇相关的 SMC 键（`examples/fanprobe.rs` 用）。
///
/// 判断「抬高最小转速 `F{i}Mn`」这条路在具体机型上是否可行，只能实测：
/// 键存不存在、当前值多少、类型是什么。纯读，不写。
pub fn dump_fan_keys() {
    let Some(conn) = smc_open() else {
        println!("无法打开 AppleSMC");
        return;
    };
    let _guard = smc_close_guard(conn);
    let count = fan_count(conn);
    println!("风扇数 FNum = {count}");
    for i in 0..count {
        for suffix in ["Ac", "Mn", "Mx", "Tg", "Md", "md", "Sf"] {
            let key = format!("F{i}{suffix}");
            match smc_read(conn, &key) {
                Some((t, sz, b)) => {
                    let ty = t.to_be_bytes();
                    println!(
                        "  {key}  type={:?} size={sz}  value={:?}",
                        String::from_utf8_lossy(&ty),
                        decode_scalar(t, &b, sz)
                    );
                }
                None => println!("  {key}  <不可读>"),
            }
        }
    }
    match smc_read(conn, "Ftst") {
        Some((t, sz, b)) => println!("  Ftst  value={:?}", decode_scalar(t, &b, sz)),
        None => println!("  Ftst  <不存在>"),
    }
}

/// 实测「抬高最小转速」这条路是否可行（`examples/fanprobe.rs --raise` 用）。
///
/// 只动 `F{i}Mn`，**不碰模式键**：系统继续在 `[新下限, 最大值]` 区间自己调速，
/// 所以这个操作只可能增加风量，不可能减少散热。跑完自动写回原值；万一进程被
/// 强杀导致没写回，SMC 里的值只存活在 RAM，重启即失效。
pub fn probe_raise_min_speed(pct: f32, hold_secs: u64) {
    if unsafe { libc::geteuid() } != 0 {
        println!("需要 root：sudo 跑");
        return;
    }
    let Some(conn) = smc_open() else {
        println!("无法打开 AppleSMC");
        return;
    };
    let _guard = smc_close_guard(conn);
    let count = fan_count(conn);
    let mut saved = Vec::new();

    for i in 0..count {
        let mn = format!("F{i}Mn");
        let mx = format!("F{i}Mx");
        let (Some((mt, msz, mb)), Some((xt, xsz, xb))) = (smc_read(conn, &mn), smc_read(conn, &mx))
        else {
            println!("{mn}/{mx} 读不到，跳过");
            continue;
        };
        let (Some(cur), Some(max)) = (decode_scalar(mt, &mb, msz), decode_scalar(xt, &xb, xsz))
        else {
            continue;
        };
        let target = max * pct;
        println!("{mn}: 当前下限 {cur:.0}，上限 {max:.0} → 尝试写入 {target:.0}");
        match smc_write(conn, &mn, mt, msz as u32, target) {
            Ok(()) => {
                saved.push((mn.clone(), mt, msz, cur));
                let readback = smc_read(conn, &mn).and_then(|(t, sz, b)| decode_scalar(t, &b, sz));
                println!("  写入成功，回读 = {readback:?}");
            }
            Err(e) => println!("  写入失败: {e}"),
        }
    }
    if saved.is_empty() {
        println!("没有任何键写成功，这条路在本机走不通");
        return;
    }
    println!("保持 {hold_secs} 秒，观察转速是否跟上来……");
    for _ in 0..hold_secs {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let rpms: Vec<String> = (0..count)
            .map(|i| {
                smc_read(conn, &format!("F{i}Ac"))
                    .and_then(|(t, sz, b)| decode_scalar(t, &b, sz))
                    .map(|v| format!("{v:.0}"))
                    .unwrap_or_else(|| "?".into())
            })
            .collect();
        println!("  实际转速 {}", rpms.join(" / "));
    }
    for (key, t, sz, orig) in saved {
        match smc_write(conn, &key, t, sz as u32, orig) {
            Ok(()) => println!("{key} 已恢复为 {orig:.0}"),
            Err(e) => println!("{key} 恢复失败（重启即复原）: {e}"),
        }
    }
}

/// Apple Silicon P 核簇和 Intel 封装温度不是同一刻度，分别使用曲线。
///
/// 这里读的是 Apple Silicon 的 P 核温度（`Tp01` 一族），它常年就在 70–90 °C，
/// 不是 Intel 时代那种 40–60 °C 的封装温度。本机自动模式下的实测点：
///
/// | 温度 | 系统自己的转速（上限 5348） |
/// |---|---|
/// | 70 °C | 1518（怠速） |
/// | 76 °C | 1548 |
/// | 78 °C | 1598 |
/// | 79 °C | 1921 |
///
/// 也就是说到 79 °C 系统都还按在怠速附近。第一版把拐点定成 55→80 °C，于是
/// 70 °C 就给到 84%、79 °C 给到 98%——「降温」实际等于全速，用户一点就发现了。
///
/// 现在定在 85→100 °C：85 °C 以下老老实实给用户设的地板（这台机器上 60%
/// ≈ 3209 RPM，远高于系统的 1500），进入真正的高温区才逐步顶到满速。
/// 100 °C 附近本来就是降频区，那时给满速不会比系统要的少。
const APPLE_RAMP_START_C: f32 = 85.0;
const APPLE_RAMP_FULL_C: f32 = 100.0;
const INTEL_RAMP_START_C: f32 = 70.0;
const INTEL_RAMP_FULL_C: f32 = 95.0;
const FAIR_DUTY_FLOOR: f32 = 0.75;
const COOLING_HYSTERESIS_C: f32 = 2.0;
const MAX_DUTY_DROP_PER_TICK: f32 = 0.05;

#[derive(Default)]
struct CoolingControlState {
    held_temp_c: Option<f32>,
    last_duty: Option<f32>,
}

static COOLING_CONTROL: Mutex<CoolingControlState> = Mutex::new(CoolingControlState {
    held_temp_c: None,
    last_duty: None,
});

fn reset_cooling_control() {
    let mut state = COOLING_CONTROL.lock().unwrap_or_else(|e| e.into_inner());
    *state = CoolingControlState::default();
}

fn curve_bounds(profile: CpuTempProfile) -> (f32, f32) {
    match profile {
        CpuTempProfile::IntelPackage => (INTEL_RAMP_START_C, INTEL_RAMP_FULL_C),
        CpuTempProfile::AppleSiliconPCore => (APPLE_RAMP_START_C, APPLE_RAMP_FULL_C),
    }
}

fn cooling_duty(
    requested: f32,
    sample: CpuTempSample,
    thermal: SystemThermalState,
) -> Result<f32, &'static str> {
    if thermal >= SystemThermalState::Serious {
        return Err("系统热压力已达到严重等级");
    }
    let mut state = COOLING_CONTROL.lock().unwrap_or_else(|e| e.into_inner());
    Ok(cooling_duty_with_state(
        requested, sample, thermal, &mut state,
    ))
}

fn cooling_duty_with_state(
    requested: f32,
    sample: CpuTempSample,
    thermal: SystemThermalState,
    state: &mut CoolingControlState,
) -> f32 {
    // 升温立即跟随；降温必须越过 2°C 回差才更新控制温度，避免临界点抖动。
    let held_temp = match state.held_temp_c {
        None => sample.celsius,
        Some(previous) if sample.celsius >= previous => sample.celsius,
        Some(previous) if sample.celsius <= previous - COOLING_HYSTERESIS_C => sample.celsius,
        Some(previous) => previous,
    };
    state.held_temp_c = Some(held_temp);

    let (start, full) = curve_bounds(sample.profile);
    let mut desired =
        effective_duty(requested, Some(held_temp), start, full).expect("调用方已提供有效温度");
    if thermal == SystemThermalState::Fair {
        desired = desired.max(FAIR_DUTY_FLOOR);
    }

    // 升档立即执行；降档每个 3 秒控制周期最多降 5 个百分点，避免风噪突变。
    let applied = match state.last_duty {
        Some(last) if desired < last => desired.max(last - MAX_DUTY_DROP_PER_TICK),
        _ => desired,
    };
    state.last_duty = Some(applied);
    applied
}

/// 实际要施加的占空比：**用户设定的地板**与**温度决定的升档**取大者。
///
/// 手动接管会把系统的多传感器调速整个替掉，固定档位因此可能在高负载时
/// 低于系统所需。这个函数是补回来的**高温逃生口**：进入对应平台曲线的
/// 起始温度
/// 以上就逐步顶到满速。它**不是** Apple 那条曲线的模型——单看 CPU 温度
/// 跟系统的多传感器决策相关性很弱（实测 70 °C 时系统按在怠速，64 °C 时
/// 反而在 3196 RPM）。它保证的只有一条：真热起来我们一定在满速，那时不
/// 可能比系统要的少。
///
/// 温度读不到时返回 `None`：拿不到温度就没法保证安全，此时只允许全速
/// （`requested >= 1.0` 时直接放行，它本来就不可能限制散热）。
fn effective_duty(
    requested: f32,
    temp_c: Option<f32>,
    ramp_start_c: f32,
    ramp_full_c: f32,
) -> Option<f32> {
    let requested = requested.clamp(0.0, 1.0);
    if requested >= 1.0 {
        return Some(1.0);
    }
    let temp = temp_c?;
    let ramp = if temp <= ramp_start_c {
        0.0
    } else if temp >= ramp_full_c {
        1.0
    } else {
        (temp - ramp_start_c) / (ramp_full_c - ramp_start_c)
    };
    // 升档是从用户地板往满速插值，而不是从 0 开始——否则低温时反而会把
    // 地板拉低。
    Some(requested + (1.0 - requested) * ramp)
}

/// 模式键的大小写因机型而异（M4 及以前是大写 `Md`，M5 是小写 `md`），
/// 每次探测成本是一次键信息读取，不值得缓存。
fn fan_mode_key(conn: u32, fan: usize) -> Option<String> {
    let upper = format!("F{fan}Md");
    let lower = format!("F{fan}md");
    if smc_read(conn, &upper).is_some() {
        Some(upper)
    } else if smc_read(conn, &lower).is_some() {
        Some(lower)
    } else {
        None
    }
}

/// Apple Silicon 上 thermalmonitord 默认把风扇锁在「系统模式」（模式键读 3），
/// 直接写模式会被固件拒绝。写 `Ftst=1` 后守护进程会在 3~6 秒内让位；
/// 没有 `Ftst` 键的新机型（M5 等）可以直接写，无需等待。
fn unlock_fan_control(conn: u32) -> Result<(), FanError> {
    let Some((t, sz, _)) = smc_read(conn, "Ftst") else {
        return Ok(());
    };
    smc_write(conn, "Ftst", t, sz as u32, 1.0)?;
    // 等模式键从 3（系统托管）变回 0/1，最多 8 秒。
    for _ in 0..32 {
        std::thread::sleep(std::time::Duration::from_millis(250));
        if fan_mode_key(conn, 0)
            .and_then(|key| smc_read(conn, &key))
            .and_then(|(t, sz, b)| decode_scalar(t, &b, sz))
            .is_none_or(|mode| mode != 3.0)
        {
            return Ok(());
        }
    }
    Err(FanError::Other(
        "thermalmonitord 未让出风扇控制权（8s 超时）".into(),
    ))
}

#[cfg(test)]
mod tests {
    fn apple_duty(requested: f32, temp_c: Option<f32>) -> Option<f32> {
        effective_duty(requested, temp_c, APPLE_RAMP_START_C, APPLE_RAMP_FULL_C)
    }

    /// 拐点刻度对不对，直接决定「降温」是不是等于「全速」。第一版按
    /// 55→80 °C 定，而 P 核传感器常年 70–90 °C，于是 70 °C 就给 84%，
    /// 用户一点就发现降温和全速没区别。这个测试钉住的正是那个回归。
    #[test]
    fn boost_stays_at_the_user_floor_across_normal_p_core_temperatures() {
        let boost = 0.60;
        // 本机自动模式下的实测温度区间，这里必须原样给地板，不能偷偷升档
        for t in [64.0, 70.0, 76.0, 79.0, 85.0] {
            assert_eq!(
                apple_duty(boost, Some(t)),
                Some(0.60),
                "{t}°C 属于这颗芯片的常态区间，降温档不该在这里就升速"
            );
        }
        // 真正的高温区才升，100 °C（降频区）给满
        let at90 = apple_duty(boost, Some(90.0)).unwrap();
        assert!(
            (0.60..1.0).contains(&at90),
            "90°C 应在升档途中，实际 {at90}"
        );
        assert_eq!(apple_duty(boost, Some(100.0)), Some(1.0));
        assert_eq!(apple_duty(boost, Some(120.0)), Some(1.0));

        // 单调不降
        let mut prev = 0.0;
        for t in 0..=130 {
            let d = apple_duty(boost, Some(t as f32)).unwrap();
            assert!(d >= prev - f32::EPSILON, "{t}°C 处占空比回落了");
            assert!((0.0..=1.0).contains(&d));
            prev = d;
        }
    }

    /// 全速不可能限制散热，所以它不依赖温度；中间档没温度就不能保证安全，
    /// 必须拒绝而不是硬着头皮给一个固定值。
    #[test]
    fn full_speed_needs_no_temperature_but_intermediate_does() {
        assert_eq!(apple_duty(1.0, None), Some(1.0));
        assert_eq!(apple_duty(1.0, Some(90.0)), Some(1.0));
        assert_eq!(apple_duty(0.60, None), None);
        assert_eq!(apple_duty(0.30, None), None);
    }

    /// 荒谬的低档不能把风扇压到系统怠速以下——那一层由 `F{i}Mn` 夹紧
    /// （在 `set_fan_mode` 里），这里只钉住曲线本身不会反向。
    #[test]
    fn a_hostile_low_duty_still_ramps_up_with_temperature() {
        assert_eq!(apple_duty(0.01, Some(30.0)), Some(0.01));
        assert_eq!(apple_duty(0.01, Some(100.0)), Some(1.0));
        let mid = apple_duty(0.01, Some(92.5)).unwrap();
        assert!((mid - 0.505).abs() < 0.01, "中点应在 ~0.5，实际 {mid}");
    }

    #[test]
    fn fair_thermal_pressure_raises_the_floor() {
        let mut state = CoolingControlState::default();
        let duty = cooling_duty_with_state(
            0.60,
            CpuTempSample {
                celsius: 70.0,
                profile: CpuTempProfile::AppleSiliconPCore,
            },
            SystemThermalState::Fair,
            &mut state,
        );
        assert_eq!(duty, FAIR_DUTY_FLOOR);
    }

    #[test]
    fn cooling_curve_has_hysteresis_and_limits_only_downward_changes() {
        let sample = |celsius| CpuTempSample {
            celsius,
            profile: CpuTempProfile::AppleSiliconPCore,
        };
        let mut state = CoolingControlState::default();
        let hot =
            cooling_duty_with_state(0.60, sample(95.0), SystemThermalState::Nominal, &mut state);
        let within_hysteresis =
            cooling_duty_with_state(0.60, sample(94.0), SystemThermalState::Nominal, &mut state);
        assert_eq!(within_hysteresis, hot);

        let cooling =
            cooling_duty_with_state(0.60, sample(90.0), SystemThermalState::Nominal, &mut state);
        assert!((cooling - (hot - MAX_DUTY_DROP_PER_TICK)).abs() < f32::EPSILON);

        let reheated =
            cooling_duty_with_state(0.60, sample(100.0), SystemThermalState::Nominal, &mut state);
        assert_eq!(reheated, 1.0, "升档不能被平滑延迟");
    }

    #[test]
    fn serious_thermal_pressure_refuses_manual_cooling() {
        let sample = CpuTempSample {
            celsius: 90.0,
            profile: CpuTempProfile::AppleSiliconPCore,
        };
        assert!(cooling_duty(0.60, sample, SystemThermalState::Serious).is_err());
        assert!(cooling_duty(0.60, sample, SystemThermalState::Critical).is_err());
    }

    #[test]
    fn foundation_exposes_system_thermal_state() {
        assert!(
            system_thermal_state().is_some(),
            "当前支持的 macOS 应提供 NSProcessInfo.thermalState"
        );
    }

    #[test]
    fn process_unique_id_is_stable_and_required_to_kill() {
        let pid = std::process::id();
        let id = super::process_unique_id(pid).expect("当前进程应有 Darwin uniqueid");
        assert_eq!(super::process_unique_id(pid), Some(id));
        assert!(
            super::terminate_process(pid, 0, None).is_err(),
            "没有 uniqueid 必须拒绝，不能退回秒级 start_time"
        );
        assert!(
            super::terminate_process(pid, 0, Some(id.wrapping_add(1))).is_err(),
            "uniqueid 对不上必须拒绝"
        );
    }

    use super::*;
}
