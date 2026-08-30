//! Windows 传感器探针：逐条打印 `platform::windows::thermal` 那条链上每一步
//! 的原始结果，包括 COM 的 HRESULT。
//!
//! 风扇卡片空着的时候，光看 UI 分不清是「这台机器没有这条通道」「没提权」
//! 还是「我们的调用写错了」。这个探针把三者分开。
//!
//! ```text
//! cargo run --example thermalprobe --features thermalprobe
//! ```
//!
//! 厂商接口要管理员权限，非提权跑只能看到「拒绝访问」。要看提权后的结果：
//!
//! ```powershell
//! cargo build --example thermalprobe --features thermalprobe
//! Start-Process -Verb RunAs .\target\debug\examples\thermalprobe.exe
//! ```
//!
//! 提权启动的控制台窗口会一闪而过，所以报告同时写进
//! `%TEMP%\quickcleaner-thermalprobe.txt`。

#[cfg(not(windows))]
fn main() {
    eprintln!("thermalprobe 只在 Windows 上有意义");
}

#[cfg(windows)]
fn main() {
    use quick_cleaner::platform::windows::wmi::{Arg, Wmi};

    let mut out = String::new();
    macro_rules! say {
        ($($arg:tt)*) => {{
            let line = format!($($arg)*);
            println!("{line}");
            out.push_str(&line);
            out.push('\n');
        }};
    }

    say!("== 环境 ==");
    say!("提权: {}", quick_cleaner::platform::is_elevated());

    say!("");
    say!("== WMI 命名空间 ==");
    for namespace in [
        "root\\CIMV2",
        "root\\WMI",
        "root\\LibreHardwareMonitor",
        "root\\OpenHardwareMonitor",
    ] {
        match Wmi::connect_diagnostic(namespace) {
            Ok(_) => say!("{namespace}: 连上了"),
            Err(hr) => say!("{namespace}: 失败 hr=0x{:08x}", hr as u32),
        }
    }

    say!("");
    say!("== 连通性自检（root\\CIMV2 一定读得到）==");
    match Wmi::connect_diagnostic("root\\CIMV2") {
        Ok(wmi) => {
            match wmi.query_diagnostic("SELECT Caption FROM Win32_OperatingSystem", &["Caption"]) {
                Ok(rows) => say!("查询回来 {} 行，第一行: {:?}", rows.len(), rows.first()),
                Err(hr) => say!("查询失败 hr=0x{:08x}", hr as u32),
            }
        }
        Err(hr) => say!("连不上 hr=0x{:08x}", hr as u32),
    }

    say!("");
    say!("== 带入参的方法调用自检（root\\default:StdRegProv）==");
    // 普通用户就能调，用来验证 GetMethod/SpawnInstance/Put 那条链是通的。
    // 厂商类要管理员，没这一步就分不清「参数塞错了」和「没权限」。
    match Wmi::connect_diagnostic("root\\default") {
        Err(hr) => say!("连不上 hr=0x{:08x}", hr as u32),
        Ok(wmi) => {
            // 0x80000002 = HKEY_LOCAL_MACHINE。ReturnValue 0 = 调通了。
            match wmi.call_number_with_args(
                "StdRegProv",
                "StdRegProv",
                "EnumKey",
                &[
                    ("hDefKey", Arg::Number(0x8000_0002)),
                    ("sSubKeyName", Arg::Text("SOFTWARE")),
                ],
                "ReturnValue",
            ) {
                Ok(value) => say!("EnumKey(HKLM) ReturnValue = {value:?}"),
                Err(hr) => say!("EnumKey(HKLM) 失败 hr=0x{:08x}", hr as u32),
            }
        }
    }

    say!("");
    say!("== 厂商接口（Lenovo Legion）==");
    match Wmi::connect_diagnostic("root\\WMI") {
        Err(hr) => say!("root\\WMI 连不上 hr=0x{:08x}", hr as u32),
        Ok(wmi) => {
            for class in [
                "LENOVO_GAMEZONE_DATA",
                "LENOVO_FAN_METHOD",
                "LENOVO_FAN_TABLE_DATA",
            ] {
                match wmi.query_diagnostic(&format!("SELECT * FROM {class}"), &["__PATH"]) {
                    Err(hr) => say!("{class}: 查不了 hr=0x{:08x}", hr as u32),
                    Ok(rows) if rows.is_empty() => say!("{class}: 没有实例"),
                    Ok(rows) => say!("{class}: {:?}", rows.first()),
                }
            }
            let path = wmi.first_instance_path("LENOVO_GAMEZONE_DATA");
            say!("GAMEZONE 对象路径: {path:?}");
            if let Some(path) = path {
                for method in [
                    "GetFanCount",
                    "GetFan1Speed",
                    "GetFan2Speed",
                    "GetFanMaxSpeed",
                    "GetCPUTemp",
                    "GetGPUTemp",
                    "GetThermalMode",
                ] {
                    match wmi.call_number_diagnostic(&path, method, "Data") {
                        Ok(value) => say!("{method}() = {value:?}"),
                        Err(hr) => say!("{method}() 失败 hr=0x{:08x}", hr as u32),
                    }
                }
            }
        }
    }

    say!("");
    say!("== 新一代联想接口（LENOVO_OTHER_METHOD.GetFeatureValue）==");
    match Wmi::connect_diagnostic("root\\WMI") {
        Err(hr) => say!("root\\WMI 连不上 hr=0x{:08x}", hr as u32),
        Ok(wmi) => {
            // 能力表列出这台固件支持哪些 feature id；传感器就藏在其中几个后面。
            let mut ids: Vec<u32> = Vec::new();
            for (class, props) in [
                (
                    "LENOVO_CAPABILITY_DATA_00",
                    &["IDs", "Capability", "DefaultValue"][..],
                ),
                (
                    "LENOVO_CAPABILITY_DATA_01",
                    &[
                        "IDs",
                        "Capability",
                        "DefaultValue",
                        "MinValue",
                        "MaxValue",
                        "Step",
                    ][..],
                ),
            ] {
                match wmi.query_diagnostic(&format!("SELECT * FROM {class}"), props) {
                    Err(hr) => say!("{class}: 查不了 hr=0x{:08x}", hr as u32),
                    Ok(rows) => {
                        say!("{class}: {} 条", rows.len());
                        for row in &rows {
                            say!("  {row:?}");
                            if let Some(Some(id)) = row.first() {
                                if let Some(id) = id.as_number() {
                                    ids.push(id as u32);
                                }
                            }
                        }
                    }
                }
            }
            let path = wmi.first_instance_path("LENOVO_OTHER_METHOD");
            say!("OTHER_METHOD 对象路径: {path:?}");
            if let Some(path) = path {
                // Lenovo Legion Toolkit 用的那组传感器 id：能力表里未必列出，
                // 但固件通常照样答。先单独试这几个，好认。
                for (label, id) in [
                    ("CPU 温度", 0x0504_0000_u32),
                    ("GPU 温度", 0x0505_0000),
                    ("CPU 风扇", 0x0403_0001),
                    ("GPU 风扇", 0x0403_0002),
                    ("风扇全速", 0x0402_0000),
                ] {
                    match wmi.call_number_with_args(
                        "LENOVO_OTHER_METHOD",
                        &path,
                        "GetFeatureValue",
                        &[("IDs", Arg::Number(id))],
                        "value",
                    ) {
                        Ok(value) => say!("  [{label}] 0x{id:08x} = {value:?}"),
                        Err(hr) => say!("  [{label}] 0x{id:08x} 失败 hr=0x{:08x}", hr as u32),
                    }
                }
                for id in ids {
                    match wmi.call_number_with_args(
                        "LENOVO_OTHER_METHOD",
                        &path,
                        "GetFeatureValue",
                        &[("IDs", Arg::Number(id))],
                        "value",
                    ) {
                        Ok(value) => say!("  GetFeatureValue(0x{id:08x}) = {value:?}"),
                        Err(hr) => {
                            say!("  GetFeatureValue(0x{id:08x}) 失败 hr=0x{:08x}", hr as u32)
                        }
                    }
                }
            }
        }
    }

    say!("");
    say!("== 风扇表 / 转速上限（属性直读）==");
    match Wmi::connect_diagnostic("root\\WMI") {
        Err(hr) => say!("root\\WMI 连不上 hr=0x{:08x}", hr as u32),
        Ok(wmi) => {
            for (class, props) in [
                (
                    "LENOVO_FAN_TABLE_DATA",
                    &[
                        "Fan_Id",
                        "CurrentFanMaxSpeed",
                        "CurrentFanMinSpeed",
                        "Mode",
                        "Sensor_ID",
                        "MaxSensorTemperature",
                        "MinSensorTemperature",
                    ][..],
                ),
                (
                    "LENOVO_FAN_MAX_SPEED_DATA",
                    &[
                        "Fan_Id",
                        "Fan_CurrentMaxSpeed",
                        "Fan_DefaultMaxSpeed",
                        "Fan_Flag",
                    ][..],
                ),
            ] {
                match wmi.query_diagnostic(&format!("SELECT * FROM {class}"), props) {
                    Err(hr) => say!("{class}: 查不了 hr=0x{:08x}", hr as u32),
                    Ok(rows) => {
                        say!("{class}: {} 条", rows.len());
                        for row in &rows {
                            say!("  {row:?}");
                        }
                    }
                }
            }
        }
    }

    say!("");
    say!("== ACPI 热区（PDH）==");
    match quick_cleaner::platform::windows::pdh::PdhQuery::open(&[
        r"\Thermal Zone Information(*)\Temperature",
    ]) {
        None => say!("计数器挂不上：这台机器没有 ACPI 热区实例"),
        Some(query) => {
            // 差值型计数器要两拍，热区是瞬时值，采两次同样安全。
            query.collect();
            std::thread::sleep(std::time::Duration::from_millis(1000));
            query.collect();
            say!("热区实例: {:?}", query.values(0));
        }
    }

    say!("");
    say!("== 汇总（UI 拿到的就是这个）==");
    let reading = quick_cleaner::platform::read_thermal();
    say!(
        "风扇 {} 个, CPU 温度 {:?}",
        reading.fans.len(),
        reading.cpu_temp
    );
    for fan in &reading.fans {
        say!("  {} = {} RPM", fan.label, fan.rpm);
    }

    let report = std::env::temp_dir().join("quickcleaner-thermalprobe.txt");
    match std::fs::write(&report, &out) {
        Ok(()) => println!("\n报告已写入 {}", report.display()),
        Err(e) => eprintln!("\n报告写不进 {}: {e}", report.display()),
    }
}
