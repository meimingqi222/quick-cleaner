//! Windows GPU 采样。
//!
//! 没有厂商 SDK 的话，系统级的 GPU 读数只有两个公开来源：性能计数器
//! （PDH）给忙闲和显存占用，DXGI 给适配器名字。任务管理器的 GPU 页也是
//! 这两个来源，数字对得上。
//!
//! - `\GPU Engine(*)\Utilization Percentage`：**每进程每引擎**一条实例，
//!   名字形如 `pid_1234_luid_0x00000000_0x00011365_phys_0_eng_0_engtype_3d`。
//!   整机忙闲要自己按适配器和引擎汇总。
//! - `\GPU Adapter Memory(*)\Dedicated Usage`：每张卡一条，全系统口径的
//!   独立显存占用（不是本进程的份额）。
//!
//! 笔记本普遍是核显 + 独显两张卡，所以这里**列出所有适配器**交给 UI 切换，
//! 而不是替用户挑一张。

use crate::core::status::GpuReading;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use winapi::ctypes::c_void;
use winapi::shared::dxgi::{
    CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, DXGI_ADAPTER_DESC1,
    DXGI_ADAPTER_FLAG_SOFTWARE,
};
use winapi::Interface;

use super::nvml;
use super::pdh::PdhQuery;
use super::registry::from_wide;

/// 忙闲：每进程每引擎一条实例。
const ENGINE_COUNTER: &str = r"\GPU Engine(*)\Utilization Percentage";
/// 独立显存占用：每张卡一条实例。
const MEMORY_COUNTER: &str = r"\GPU Adapter Memory(*)\Dedicated Usage";

/// 3D 引擎的类别名。渲染占用单独拎出来，和 macOS 的
/// "Renderer Utilization %" 摆在卡片的同一个位置。
const ENGTYPE_3D: &str = "3d";

enum State {
    /// 还没开过查询。
    Unopened,
    /// 开不出来（计数器被禁用 / WDDM 太老），别每两秒重试一次。
    Unavailable,
    Ready(PdhQuery),
}

static STATE: Mutex<State> = Mutex::new(State::Unopened);

/// 这台机器上的全部 GPU，按 LUID 升序（顺序要稳，UI 的切换按钮跟着它排）。
/// 拿不到就是空表，UI 那边整张卡不渲染。
pub fn read_gpus() -> Vec<GpuReading> {
    let Ok(mut state) = STATE.lock() else {
        return Vec::new();
    };
    if matches!(*state, State::Unopened) {
        *state = match PdhQuery::open(&[ENGINE_COUNTER, MEMORY_COUNTER]) {
            Some(query) => State::Ready(query),
            None => State::Unavailable,
        };
    }
    let State::Ready(query) = &*state else {
        return Vec::new();
    };
    if !query.collect() {
        return Vec::new();
    }
    // 第一拍没有可比的前一拍，PDH 返回错误、实例表是空的——于是第一拍如实
    // 什么都不显示，第二拍开始才有数。
    let mut gpus = aggregate(&query.values(0), &query.values(1));
    // 温度只有 N 卡拿得到，按型号名贴到对应那张卡上（见 `nvml`）。
    let temperatures = nvml::gpu_temperatures();
    for gpu in &mut gpus {
        gpu.temp_c = gpu
            .name
            .as_deref()
            .and_then(|name| temperatures.get(name).copied());
    }
    gpus
}

/// 把两张实例表汇总成每张卡一条读数。纯函数，可单测。
fn aggregate(engines: &HashMap<String, f64>, memory: &HashMap<String, f64>) -> Vec<GpuReading> {
    // 适配器 → 引擎实例 → 各进程占用之和。
    //
    // 键必须是**具体那条引擎**（`phys_0_eng_1_engtype_copy`）而不是引擎类别：
    // 一张卡可能挂着多条同类引擎（独显的复制引擎常有两三条），按类别求和会
    // 把「两条复制引擎各 60%」算成 120%，clamp 之后变成一个假的 100%。
    let mut per_adapter: HashMap<u64, HashMap<&str, f64>> = HashMap::new();
    for (instance, value) in engines {
        let (Some(luid), Some(engine)) = (parse_luid(instance), parse_engine(instance)) else {
            continue;
        };
        *per_adapter
            .entry(luid)
            .or_default()
            .entry(engine)
            .or_insert(0.0) += value;
    }

    let mut vram: HashMap<u64, u64> = HashMap::new();
    for (instance, bytes) in memory {
        if let Some(luid) = parse_luid(instance) {
            if *bytes > 0.0 {
                vram.insert(luid, *bytes as u64);
            }
        }
    }

    // 只列 PDH 真的认识、而且确实在干活或占着显存的卡。虚拟显示器驱动
    // （远程控制软件装的那种）也会占一条 LUID，但既没有引擎实例也不占独立
    // 显存，混进切换按钮里只会挤掉真显卡的位置。
    //
    // 软件适配器（"Microsoft Basic Render Driver"，也就是 WARP）另说：它有
    // 引擎实例也占显存，但它不是硬件，实测会在切换按钮里多出一个没人想看的
    // 「Microsoft」，按 DXGI 的标志位剔掉。
    let adapters = adapters();
    let mut luids: Vec<u64> = per_adapter
        .keys()
        .chain(vram.keys())
        .copied()
        .filter(|luid| !adapters.get(luid).is_some_and(|a| a.software))
        .collect();
    luids.sort_unstable();
    luids.dedup();
    luids
        .into_iter()
        .map(|luid| {
            let engines = per_adapter.get(&luid);
            // 一张卡的忙闲取**最忙的那条引擎**，不是各条之和：3D、复制、视频
            // 编解码是并行的独立单元，加起来轻易过 100%。任务管理器也是取最大值。
            //
            // 没有引擎实例 = 这张卡上没有任何进程的 GPU 上下文，那就是真闲着，
            // 报 0% 不算编数据。
            let util = engines.map_or(0.0, |e| e.values().copied().fold(0.0_f64, f64::max));
            let renderer = engines.and_then(|engines| {
                engines
                    .iter()
                    .filter(|(engine, _)| parse_engtype(engine) == Some(ENGTYPE_3D))
                    .map(|(_, util)| *util)
                    .fold(None, |best: Option<f64>, util| {
                        Some(best.map_or(util, |best| best.max(util)))
                    })
            });
            GpuReading {
                id: format!("{luid:016x}"),
                name: adapters.get(&luid).and_then(|a| a.name.clone()),
                utilization: Some(util.clamp(0.0, 100.0) as f32),
                renderer_utilization: renderer.map(|util| util.clamp(0.0, 100.0) as f32),
                vram_in_use: vram.get(&luid).copied(),
                // 温度是 NVML 那边贴上来的，聚合这一步只管 PDH 的数据。
                temp_c: None,
            }
        })
        .collect()
}

/// 实例名里的适配器 LUID，高低两半拼成一个数当键。
///
/// `pid_1234_luid_0x00000000_0x00011365_phys_0_eng_0_engtype_3d`
/// `luid_0x00000000_0x00011365_phys_0`
fn parse_luid(instance: &str) -> Option<u64> {
    let mut parts = instance.split("luid_").nth(1)?.split('_');
    let high = parse_hex(parts.next()?)?;
    let low = parse_hex(parts.next()?)?;
    Some(((high as u64) << 32) | low as u64)
}

fn parse_hex(token: &str) -> Option<u32> {
    u32::from_str_radix(token.strip_prefix("0x")?, 16).ok()
}

/// 实例名里的引擎标识：适配器内部的第几条引擎，形如
/// `phys_0_eng_1_engtype_copy`。同一张卡的同类引擎可能有好几条，汇总时
/// 必须分开算。
fn parse_engine(instance: &str) -> Option<&str> {
    // +1 只吃掉分隔用的下划线，`phys_` 本身要留着——它是引擎标识的一部分。
    let engine = &instance[instance.find("_phys_")? + 1..];
    engine.contains("engtype_").then_some(engine)
}

/// 引擎类别（`3d` / `copy` / `videodecode` / `video codec 0` …）。
/// 类别名可能带空格，所以取的是「`engtype_` 之后的全部」而不是下一段。
fn parse_engtype(instance: &str) -> Option<&str> {
    instance
        .split("engtype_")
        .nth(1)
        .filter(|engtype| !engtype.is_empty())
}

/// DXGI 眼里的一张适配器。
struct Adapter {
    name: Option<String>,
    /// 软件渲染器（WARP）。能跑 D3D，但不是这台机器上的显卡。
    software: bool,
}

/// LUID → 适配器。适配器在进程生命周期内不会变，枚举一次就够。
fn adapters() -> &'static HashMap<u64, Adapter> {
    static ADAPTERS: OnceLock<HashMap<u64, Adapter>> = OnceLock::new();
    ADAPTERS.get_or_init(enumerate_adapters)
}

/// DXGI 枚举适配器，取 LUID、型号名和「是不是软件渲染器」。PDH 那边只有
/// LUID，别的都得从这里来。
fn enumerate_adapters() -> HashMap<u64, Adapter> {
    let mut names = HashMap::new();
    let mut factory: *mut IDXGIFactory1 = std::ptr::null_mut();
    // SAFETY: 出参是本函数栈上的指针，失败时不会被写。
    let hr = unsafe {
        CreateDXGIFactory1(
            &IDXGIFactory1::uuidof(),
            &mut factory as *mut *mut IDXGIFactory1 as *mut *mut c_void,
        )
    };
    if hr < 0 || factory.is_null() {
        return names;
    }
    // SAFETY: factory 创建成功；下面每个 COM 指针都在用完后 Release。
    unsafe {
        let mut index = 0;
        loop {
            let mut adapter: *mut IDXGIAdapter1 = std::ptr::null_mut();
            // 枚举到头返回 DXGI_ERROR_NOT_FOUND（负数）。
            if (*factory).EnumAdapters1(index, &mut adapter) < 0 || adapter.is_null() {
                break;
            }
            let mut desc: DXGI_ADAPTER_DESC1 = std::mem::zeroed();
            if (*adapter).GetDesc1(&mut desc) >= 0 {
                let luid = ((desc.AdapterLuid.HighPart as u32 as u64) << 32)
                    | desc.AdapterLuid.LowPart as u64;
                let name = from_wide(&desc.Description);
                names.insert(
                    luid,
                    Adapter {
                        name: (!name.is_empty()).then_some(name),
                        software: desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE != 0,
                    },
                );
            }
            (*adapter).Release();
            index += 1;
        }
        (*factory).Release();
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[(&str, f64)]) -> HashMap<String, f64> {
        rows.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn instance_names_yield_adapter_and_engine() {
        let instance = "pid_12680_luid_0x00000000_0x00011365_phys_0_eng_0_engtype_3d";
        assert_eq!(parse_luid(instance), Some(0x11365));
        assert_eq!(parse_engine(instance), Some("phys_0_eng_0_engtype_3d"));
        assert_eq!(parse_engtype(instance), Some("3d"));
        // 显存计数器的实例名没有引擎那截，不能当成引擎实例混进来。
        assert_eq!(parse_engine("luid_0x0_0x1_phys_0"), None);
        assert_eq!(
            parse_luid("luid_0x00000000_0x000142a1_phys_0"),
            Some(0x142a1)
        );
        // 类别名带空格，不能按下划线再切一刀。
        assert_eq!(
            parse_engtype("pid_1_luid_0x0_0x1_phys_0_eng_10_engtype_video codec 0"),
            Some("video codec 0")
        );
        // LUID 高半区非零的机器上，高低两半不能混成同一个键。
        assert_eq!(
            parse_luid("luid_0x00000001_0x00000002_phys_0"),
            Some(0x1_0000_0002)
        );
        assert_eq!(parse_luid("no luid here"), None);
    }

    /// 一张卡的忙闲取最忙的那条引擎：3D 60% + 复制 40% 是 60%，不是 100%。
    #[test]
    fn utilization_takes_the_busiest_engine_not_the_sum() {
        let engines = table(&[
            ("pid_1_luid_0x0_0x1_phys_0_eng_0_engtype_3d", 40.0),
            ("pid_2_luid_0x0_0x1_phys_0_eng_0_engtype_3d", 20.0),
            ("pid_1_luid_0x0_0x1_phys_0_eng_1_engtype_copy", 40.0),
        ]);
        let gpus = aggregate(&engines, &table(&[("luid_0x0_0x1_phys_0", 1024.0)]));
        assert_eq!(gpus.len(), 1);
        // 同一条引擎上的多个进程要相加：两个 3D 进程合起来 60%。
        assert_eq!(gpus[0].utilization, Some(60.0));
        assert_eq!(gpus[0].renderer_utilization, Some(60.0));
        assert_eq!(gpus[0].vram_in_use, Some(1024));
    }

    /// 同一张卡的同类引擎有好几条时也不能相加：两条复制引擎各 60% 是 60%，
    /// 不是 120%——按类别求和再 clamp，就成了一个假的满载 100%。
    #[test]
    fn several_engines_of_one_type_are_not_summed_together() {
        let engines = table(&[
            ("pid_1_luid_0x0_0x1_phys_0_eng_1_engtype_copy", 60.0),
            ("pid_1_luid_0x0_0x1_phys_0_eng_2_engtype_copy", 60.0),
        ]);
        let gpus = aggregate(&engines, &HashMap::new());
        assert_eq!(gpus[0].utilization, Some(60.0));
        // 一条 3D 引擎都没有，就别编一个渲染占用出来。
        assert_eq!(gpus[0].renderer_utilization, None);
    }

    /// 双显卡机器：两张都要列出来（UI 靠这张表画切换按钮），顺序按 LUID
    /// 固定，显存各归各的。
    #[test]
    fn every_adapter_is_listed_in_a_stable_order() {
        let engines = table(&[
            ("pid_1_luid_0x0_0x2_phys_0_eng_0_engtype_3d", 70.0),
            ("pid_1_luid_0x0_0x1_phys_0_eng_0_engtype_3d", 5.0),
        ]);
        let memory = table(&[
            ("luid_0x0_0x1_phys_0", 100.0),
            ("luid_0x0_0x2_phys_0", 900.0),
        ]);
        let gpus = aggregate(&engines, &memory);
        assert_eq!(gpus.len(), 2);
        assert_eq!(gpus[0].id, format!("{:016x}", 1));
        assert_eq!(gpus[0].utilization, Some(5.0));
        assert_eq!(gpus[0].vram_in_use, Some(100));
        assert_eq!(gpus[1].utilization, Some(70.0));
        assert_eq!(gpus[1].vram_in_use, Some(900));
    }

    /// 占着显存但一个进程都没在用的卡（休眠中的独显）仍然要列出来，
    /// 忙闲如实报 0——用户得能在切换按钮里找到它。
    #[test]
    fn idle_adapter_with_vram_still_shows_up() {
        let gpus = aggregate(&HashMap::new(), &table(&[("luid_0x0_0x3_phys_0", 512.0)]));
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].utilization, Some(0.0));
        assert_eq!(gpus[0].vram_in_use, Some(512));
    }

    /// 第一拍两张表都是空的（差值型计数器还没有前一拍）：什么都别报，
    /// 否则 UI 会以为「这台机器没有 GPU」。
    #[test]
    fn empty_sample_reports_no_adapters() {
        assert!(aggregate(&HashMap::new(), &HashMap::new()).is_empty());
    }
}
