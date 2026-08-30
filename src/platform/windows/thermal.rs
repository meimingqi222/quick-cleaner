//! Windows 的风扇转速与温度。
//!
//! Windows 没有「问系统要传感器读数」的统一接口：转速和温度存在嵌入式
//! 控制器（EC）和主板的 Super I/O 芯片里，读它们要端口级访问，所以
//! HWiNFO / AIDA64 / LibreHardwareMonitor 这类多品牌工具**清一色自带内核
//! 驱动**。给一个磁盘清理工具装内核驱动不合适（签名、杀软误报、攻击面都是
//! 真代价），所以这里只走不装驱动就能拿到的几条路，按覆盖面从宽到窄排队，
//! 谁能给数据就用谁：
//!
//! 1. [`LHM_NAMESPACES`]：用户**已经在跑** LibreHardwareMonitor 或
//!    OpenHardwareMonitor 时，它们会把全部传感器发布成 WMI 命名空间。
//!    等于白捡它们的多品牌支持，我们一行驱动代码都不用写。
//! 2. [`LENOVO_CLASS`]：Lenovo Legion / 拯救者把风扇转速和 CPU 温度做成了
//!    只读 WMI 方法，不用驱动，但要管理员（应用默认就是提权启动的）。
//!    其他品牌大多没有对等接口，只能等第 1 条或者第 3 条。
//! 3. ACPI 热区（[`THERMAL_ZONE_COUNTER`]）：完全通用，但很多笔记本的
//!    固件根本不向 Windows 暴露热区，这时候一个实例都没有。只有温度，
//!    没有转速。
//!
//! 三条都空就如实返回空——卡片显示「未检测到风扇」，不编数字。

use crate::core::status::{FanInfo, ThermalReading};
use std::sync::{Mutex, OnceLock};

use super::pdh::PdhQuery;
use super::wmi::{with_namespace, Arg};

/// LibreHardwareMonitor / OpenHardwareMonitor 在跑的时候暴露的命名空间。
/// 前者是后者的活跃分支，两边的 `Sensor` 类形状一样。
const LHM_NAMESPACES: [&str; 2] = ["root\\LibreHardwareMonitor", "root\\OpenHardwareMonitor"];

/// Lenovo Legion 的只读传感器接口所在的类（老一代，2019 年前的 Y 系列）。
const LENOVO_CLASS: &str = "LENOVO_GAMEZONE_DATA";
const LENOVO_NAMESPACE: &str = "root\\WMI";

/// 新一代（Legion 2020+）的入口：传感器全在 `GetFeatureValue(IDs)` 后面。
/// 老一代那批 `GetFanXSpeed` 在这些机器上是空壳——调用返回
/// `WBEM_E_INVALID_OBJECT`，`GetCPUTemp` 恒返回 0。
const LENOVO_OTHER_CLASS: &str = "LENOVO_OTHER_METHOD";

/// 传感器的 feature id。固件的能力表（`LENOVO_CAPABILITY_DATA_00`）会声明
/// 支持哪些，实机上这三个都在里面。这里不查表，直接调用后用量程校验——
/// 不支持的 id 要么报错、要么返回 0，两种都过不了校验。
const ID_CPU_FAN: u32 = 0x0403_0001;
const ID_GPU_FAN: u32 = 0x0403_0002;
const ID_CPU_TEMP: u32 = 0x0504_0000;

/// ACPI 热区温度，开尔文。
const THERMAL_ZONE_COUNTER: &str = r"\Thermal Zone Information(*)\Temperature";

/// 合理的转速区间。超出的一律当成「这条方法不支持，返回了哨兵值」。
const RPM_RANGE: std::ops::Range<f64> = 0.0..20_000.0;
/// 合理的温度区间，摄氏度。0 也当无效：EC 读不到时常返回 0。
const TEMP_RANGE: std::ops::Range<f64> = 1.0..150.0;

pub fn read_thermal() -> ThermalReading {
    // 依次问每个来源，先拿到的赢：排在前面的口径更准（LHM 直接读芯片，
    // 厂商接口只给 EC 汇总值），排在后面的只是兜底。
    let mut reading = read_from_lhm().unwrap_or_default();
    if reading.fans.is_empty() && reading.cpu_temp.is_none() {
        reading = read_from_lenovo().unwrap_or_default();
    }
    if reading.fans.is_empty() && reading.cpu_temp.is_none() {
        reading = read_from_lenovo_v2().unwrap_or_default();
    }
    if reading.cpu_temp.is_none() {
        reading.cpu_temp = read_acpi_thermal_zone();
    }
    reading
}

/// LibreHardwareMonitor / OpenHardwareMonitor 正在运行时的传感器表。
///
/// `Sensor` 一行一个传感器：`SensorType` 是类别（`Fan` 是转速，
/// `Temperature` 是温度），`Identifier` 形如 `/amdcpu/0/temperature/2`，
/// 认 CPU 靠它而不是靠 `Name`——`Name` 是可以被用户改的。
fn read_from_lhm() -> Option<ThermalReading> {
    for namespace in LHM_NAMESPACES {
        let rows = with_namespace(namespace, |wmi| {
            wmi.map(|wmi| {
                wmi.query(
                    "SELECT Name, Value, SensorType, Identifier FROM Sensor",
                    &["Name", "Value", "SensorType", "Identifier"],
                )
            })
        });
        let Some(rows) = rows else { continue };
        if rows.is_empty() {
            continue;
        }
        let mut reading = ThermalReading::default();
        let mut cpu_temp: Option<f64> = None;
        for row in rows {
            let name = row
                .first()
                .and_then(|v| v.as_ref()?.as_text())
                .unwrap_or("");
            let Some(value) = row.get(1).and_then(|v| v.as_ref()?.as_number()) else {
                continue;
            };
            let kind = row.get(2).and_then(|v| v.as_ref()?.as_text()).unwrap_or("");
            let id = row.get(3).and_then(|v| v.as_ref()?.as_text()).unwrap_or("");
            match kind {
                "Fan" if RPM_RANGE.contains(&value) => reading.fans.push(FanInfo {
                    label: if name.is_empty() {
                        format!("Fan {}", reading.fans.len() + 1)
                    } else {
                        name.to_string()
                    },
                    rpm: value as f32,
                }),
                "Temperature" if is_cpu_sensor(id, name) && TEMP_RANGE.contains(&value) => {
                    // 多核机器上每个核一条，取最热的那条当 CPU 温度。
                    cpu_temp = Some(cpu_temp.map_or(value, |best: f64| best.max(value)));
                }
                _ => {}
            }
        }
        reading.cpu_temp = cpu_temp.map(|t| t as f32);
        if !reading.fans.is_empty() || reading.cpu_temp.is_some() {
            return Some(reading);
        }
    }
    None
}

/// 这条温度传感器是不是 CPU 的。
fn is_cpu_sensor(identifier: &str, name: &str) -> bool {
    identifier.starts_with("/intelcpu")
        || identifier.starts_with("/amdcpu")
        // 改过名字、或者旧版 OHM 没给 Identifier 时的退路。
        || name.to_ascii_lowercase().contains("cpu")
}

/// Lenovo Legion 的 WMI 接口。非提权进程会被拒（整条链返回 `None`）。
fn read_from_lenovo() -> Option<ThermalReading> {
    let path = lenovo_object_path()?;
    with_namespace(LENOVO_NAMESPACE, |wmi| {
        let wmi = wmi?;
        let number = |method: &str| wmi.call_number(path, method, "Data");
        // 方法名是固定的，没有「第 N 个风扇」的通用形式，机型最多两个风扇。
        let mut fans = Vec::new();
        for (index, method) in ["GetFan1Speed", "GetFan2Speed"].into_iter().enumerate() {
            match number(method) {
                Some(rpm) if RPM_RANGE.contains(&rpm) => fans.push(FanInfo {
                    label: format!("Fan {}", index + 1),
                    rpm: rpm as f32,
                }),
                // 第一个风扇就读不到 = 这台机器不支持，别再试第二个。
                _ if index == 0 => return None,
                _ => break,
            }
        }
        let cpu_temp = number("GetCPUTemp")
            .filter(|t| TEMP_RANGE.contains(t))
            .map(|t| t as f32);
        (!fans.is_empty() || cpu_temp.is_some()).then_some(ThermalReading { fans, cpu_temp })
    })
}

/// 新一代联想接口。风扇转速和 CPU 温度各是一次 `GetFeatureValue` 调用。
fn read_from_lenovo_v2() -> Option<ThermalReading> {
    let path = lenovo_other_object_path()?;
    with_namespace(LENOVO_NAMESPACE, |wmi| {
        let wmi = wmi?;
        let feature = |id: u32| {
            wmi.call_number_with_args(
                LENOVO_OTHER_CLASS,
                path,
                "GetFeatureValue",
                &[("IDs", Arg::Number(id))],
                "value",
            )
            .ok()
            .flatten()
        };
        let mut fans = Vec::new();
        for (index, id) in [ID_CPU_FAN, ID_GPU_FAN].into_iter().enumerate() {
            // 0 在这条接口上是「读不到」而不是「停转」：风扇真停下时这个 id
            // 直接报错。留着 0 只会让卡片显示一个假的 0 RPM。
            if let Some(rpm) = feature(id).filter(|rpm| *rpm > 0.0 && RPM_RANGE.contains(rpm)) {
                fans.push(FanInfo {
                    label: format!("Fan {}", index + 1),
                    rpm: rpm as f32,
                });
            }
        }
        let cpu_temp = feature(ID_CPU_TEMP)
            .filter(|t| TEMP_RANGE.contains(t))
            .map(|t| t as f32);
        (!fans.is_empty() || cpu_temp.is_some()).then_some(ThermalReading { fans, cpu_temp })
    })
}

/// 同 [`lenovo_object_path`]，新一代那个类的。
fn lenovo_other_object_path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| {
        with_namespace(LENOVO_NAMESPACE, |wmi| {
            wmi?.first_instance_path(LENOVO_OTHER_CLASS)
        })
    })
    .as_deref()
}

/// Lenovo 类的对象路径。一次连不上就永远不再试：这条路要么支持要么不支持，
/// 中间态只有「没提权」，而提权状态在进程生命周期里不会变。
fn lenovo_object_path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| {
        with_namespace(LENOVO_NAMESPACE, |wmi| {
            wmi?.first_instance_path(LENOVO_CLASS)
        })
    })
    .as_deref()
}

enum ZoneState {
    Unopened,
    Unavailable,
    Ready(PdhQuery),
}

static ZONES: Mutex<ZoneState> = Mutex::new(ZoneState::Unopened);

/// ACPI 热区温度。固件不暴露热区的机器上一个实例都没有，返回 `None`。
///
/// 一台机器上可能有好几个热区（CPU、主板、电池仓），取最高的那个当整机
/// 温度：用户想知道的是「烫不烫」，不是某个具体位置的读数。
fn read_acpi_thermal_zone() -> Option<f32> {
    let mut state = ZONES.lock().ok()?;
    if matches!(*state, ZoneState::Unopened) {
        *state = match PdhQuery::open(&[THERMAL_ZONE_COUNTER]) {
            Some(query) => ZoneState::Ready(query),
            None => ZoneState::Unavailable,
        };
    }
    let ZoneState::Ready(query) = &*state else {
        return None;
    };
    if !query.collect() {
        return None;
    }
    query
        .values(0)
        .into_values()
        // 计数器给的是开尔文。
        .map(|kelvin| kelvin - 273.15)
        .filter(|celsius| TEMP_RANGE.contains(celsius))
        .fold(None, |best: Option<f64>, c| {
            Some(best.map_or(c, |best| best.max(c)))
        })
        .map(|c| c as f32)
}
