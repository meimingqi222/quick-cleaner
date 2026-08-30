//! 系统状态采样：状态监控页的 CPU / 内存 / 网络 / 进程 / 温度快照。
//!
//! [`StatusSampler`] 是一个**长生命周期**的采样器：sysinfo 的 CPU 占用、
//! 进程 CPU%、网络速率都是「相对上一次刷新」的差值，`System` /
//! `Networks` 必须跨采样存活，不能每拍新建。采样器本身被状态监控的后台
//! 轮询任务持有（见 `ui::actions::status`），每拍产出一个语言中立的
//! [`StatusSnapshot`]，主线程拿到后只做渲染。

use sysinfo::{Networks, ProcessesToUpdate, System};
/// 单个风扇的读数。
#[derive(Clone, Debug)]
pub struct FanInfo {
    /// 风扇名（如 "Left fan"）；拿不到名字时是 "Fan N"。
    pub label: String,
    pub rpm: f32,
}

/// 风扇控制档位。`Auto` 是系统默认策略；`Percent(60)` 是温度联动的降温档，
/// `Percent(100)` 是全速。降温档的实际目标会随温度从 60% 向 100% 提升。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FanMode {
    #[default]
    Auto,
    Percent(u8),
}

impl FanMode {
    /// 产品与 helper 协议共同支持的档位。放在共享类型旁边，避免客户端和
    /// SMC 实现各留一份白名单，像之前那样 UI 放开 60、入口却仍拒绝。
    pub fn is_supported(self) -> bool {
        matches!(self, Self::Auto | Self::Percent(60 | 100))
    }
}

/// 风扇控制的失败原因。
///
/// 用类型而不是错误文案分流：UI 靠 [`FanError::NeedsRoot`] 决定是否回退到
/// 管理员授权通道，靠 [`FanError::Canceled`] 区分「用户主动放弃」和真失败。
/// 早先是 `err.contains("root required")`，底层换个措辞就会静默退化成
/// 「不回退、直接报错」，这类退化在编译期看不见。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FanError {
    /// SMC 固件拒绝非特权写入，必须以 root 重试。
    NeedsRoot(String),
    /// 用户在管理员授权框上点了取消。
    Canceled,
    /// 特权守护进程没装。调用方应当先征得用户同意再跑一次安装——那是整个
    /// 功能唯一一次「装系统组件」确认框。
    NotInstalled,
    /// 守护进程在，但握手对不上（重新打包后的旧二进制）。调用方应当直接
    /// 覆盖安装，不必先卸再装，也不再弹应用内确认——用户已经同意过一次。
    NeedsUpgrade,
    /// 其它失败：键不可读、机型不支持、解锁超时……
    Other(String),
}

impl std::fmt::Display for FanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FanError::NeedsRoot(detail) | FanError::Other(detail) => f.write_str(detail),
            FanError::Canceled => f.write_str("canceled"),
            FanError::NotInstalled => f.write_str("风扇守护进程未安装"),
            FanError::NeedsUpgrade => f.write_str("风扇守护进程需要更新"),
        }
    }
}

/// 一张 GPU 的一次采样。拿不到的字段是 `None`，UI 显示「不可用」。
#[derive(Clone, Debug, Default)]
pub struct GpuReading {
    /// 这张卡的稳定标识（Windows 是 LUID，macOS 是 IORegistry 里的次序）。
    /// 双显卡机器上 UI 靠它记住用户选了哪张——不能用下标，卡片顺序会随
    /// 采样结果变。
    pub id: String,
    /// 芯片型号（如 "AGXAcceleratorG13X"）。
    pub name: Option<String>,
    /// 整体利用率 0~100。
    pub utilization: Option<f32>,
    /// 渲染器占用 0~100。与整体利用率的差值大致反映有多少时间花在计算而非绘制。
    pub renderer_utilization: Option<f32>,
    /// GPU 当前占用的系统内存（统一内存架构上没有独立显存）。
    pub vram_in_use: Option<u64>,
    /// 芯片温度，摄氏度。Windows 上来自 N 卡驱动自带的 NVML，别家显卡
    /// 没有免驱动通道，是 `None`。
    pub temp_c: Option<f32>,
}

/// 多显卡时切换按钮上的短标签。
///
/// 卡片只有约 1/4 行宽，"NVIDIA GeForce RTX 4060 Laptop GPU" 这种全名一个都
/// 塞不下；取第一段（厂商名）就足够区分核显和独显了，全名仍在卡片脚注里。
/// 两张卡的第一段撞车（同厂双卡）时整体退回「GPU 1 / GPU 2」——宁可标签没
/// 信息，也不能给出两个一模一样的按钮。
pub fn gpu_labels(gpus: &[GpuReading]) -> Vec<String> {
    let labels: Vec<String> = gpus
        .iter()
        .enumerate()
        .map(|(i, gpu)| {
            gpu.name
                .as_deref()
                .and_then(|name| name.split_whitespace().next())
                .map(|vendor| vendor.chars().take(10).collect())
                .unwrap_or_else(|| format!("GPU {}", i + 1))
        })
        .collect();
    let unique: std::collections::HashSet<&String> = labels.iter().collect();
    if unique.len() == labels.len() {
        labels
    } else {
        (1..=gpus.len()).map(|i| format!("GPU {i}")).collect()
    }
}

/// 一次电池采样。台式机 / 无电池设备是 `None`。
#[derive(Clone, Debug)]
pub struct BatteryReading {
    /// 当前电量百分比 0~100。
    pub percent: f32,
    pub charging: bool,
    /// 是否接着电源（接着但没在充 = 已充满或被充电管理策略暂停）。
    pub external: bool,
    pub fully_charged: bool,
    pub cycle_count: Option<u32>,
    /// 厂商标称的循环次数上限（现代 Mac 多为 1000）。
    pub design_cycle_count: Option<u32>,
    /// 最大容量 / 设计容量，与「设置 → 电池 → 最大容量」同口径。
    pub health_percent: Option<f32>,
    pub temp_c: Option<f32>,
    /// 充电中是「充满还需」，放电中是「还能用」。驱动说不准时是 `None`。
    pub minutes_remaining: Option<u32>,
}

/// 一次热度采样：风扇转速列表 + CPU 温度（拿得到才算 Some）。
#[derive(Clone, Debug, Default)]
pub struct ThermalReading {
    pub fans: Vec<FanInfo>,
    pub cpu_temp: Option<f32>,
}

/// 进程表里的一行。
#[derive(Clone, Debug)]
pub struct ProcInfo {
    pub pid: u32,
    /// 进程启动时间（Unix 时间秒）。Windows 用它和进程句柄上的创建时间对账。
    pub start_time: u64,
    /// Darwin `p_uniqueid`。macOS 结束进程靠它识别身份；Windows 没有对等
    /// 概念，是 `None`，那边用进程句柄绑定。
    pub unique_id: Option<u64>,
    /// 给用户看的名字：优先外层 `.app` 的显示名，helper 再带上自身名称
    /// （`Warp` + `stable` → `Warp stable`）。对不上任何 bundle 时才用可执行文件名。
    pub name: String,
    /// 自上一拍以来的 CPU 占用百分比（全核折算，0~100×核数可能超 100）。
    pub cpu: f32,
    pub mem_bytes: u64,
    /// 取图标用的路径：macOS 是进程所属的 `.app` 包，Windows 是可执行文件本身。
    /// 守护进程、命令行工具这类不属于任何应用的进程是 `None`，UI 回退到首字母。
    pub icon_source: Option<std::path::PathBuf>,
}

/// 一拍采样结果的纯数据快照。错误信息与文案全部语言中立，本地化在 UI 层做。
#[derive(Clone, Debug, Default)]
pub struct StatusSnapshot {
    /// 全局 CPU 占用（0~100，所有核心折算）。
    pub cpu_usage: f32,
    pub core_count: usize,
    pub mem_used: u64,
    pub mem_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    /// 网络下行 / 上行速率（字节每秒，所有物理接口求和）。
    pub rx_bps: f64,
    pub tx_bps: f64,
    /// 按 CPU 占用降序的前几个进程。
    pub processes: Vec<ProcInfo>,
    pub process_count: usize,
    pub thermal: ThermalReading,
    pub uptime_secs: u64,
    /// 机器上的每张 GPU。笔记本普遍是核显 + 独显两张，UI 给切换按钮。
    pub gpus: Vec<GpuReading>,
    pub battery: Option<BatteryReading>,
    /// 系统名（如 "macOS 15.6" / "Windows 11"），健康卡片的小徽章。
    /// 由 [`short_os_name`] 砍掉 SKU / 代号，徽章那格放不下完整版本名。
    pub os_name: String,
    /// 物理内存总量（字节），格式化成 "32 GB" 徽章用。
    pub mem_total_label_bytes: u64,
}

pub struct StatusSampler {
    sys: System,
    nets: Networks,
    /// 每个接口上一拍的累计收发字节数 (rx, tx)。
    ///
    /// 速率按**单接口**对基线求差再汇总，而不是对「所有接口之和」求差：
    /// VPN、USB 网卡、手机热点都可能在运行中出现，它们的计数器一上来
    /// 就是「开机以来累计字节」，若直接混进总和求差，会折算出一个虚假的
    /// 巨大速率。新接口的首拍只建基线、不计增量。
    net_baseline: std::collections::HashMap<String, (u64, u64)>,
    last_sample: Option<std::time::Instant>,
}

impl Default for StatusSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusSampler {
    pub fn new() -> Self {
        let mut sampler = Self {
            sys: System::new(),
            nets: Networks::new_with_refreshed_list(),
            net_baseline: std::collections::HashMap::new(),
            last_sample: None,
        };
        // CPU 占用是相对上一次刷新的差值，先拍一次基线，否则第一拍的
        // 读数是从「进程诞生」算起的，会顶满 100%。
        sampler.sys.refresh_cpu_usage();
        sampler.sys.refresh_memory();
        // 网络速率也要基线：先记下首拍每个接口的累计字节数。
        sampler.nets.refresh(true);
        sampler.update_net_baseline();
        sampler.last_sample = Some(std::time::Instant::now());
        sampler
    }

    /// 逐接口刷新基线，返回本拍非回环接口的增量 (rx, tx)。
    ///
    /// 见 [`StatusSampler::net_baseline`]：新出现的接口首拍只记基线，
    /// 贡献 0 增量；已消失的接口残留一条基线，不再参与求和（计数器
    /// 归零的接口由 saturating_sub 兜住，重置当拍按 0 计）。
    fn update_net_baseline(&mut self) -> (u64, u64) {
        let mut rx_delta = 0u64;
        let mut tx_delta = 0u64;
        for (name, data) in self.nets.iter() {
            if is_loopback_interface(name) {
                continue;
            }
            let current = (data.total_received(), data.total_transmitted());
            if let Some(&(prev_rx, prev_tx)) = self.net_baseline.get(name.as_str()) {
                rx_delta += current.0.saturating_sub(prev_rx);
                tx_delta += current.1.saturating_sub(prev_tx);
            }
            self.net_baseline.insert(name.to_string(), current);
        }
        (rx_delta, tx_delta)
    }

    /// 采一拍。阻塞几百毫秒以内的系统调用，只允许在后台线程跑。
    pub fn sample(&mut self) -> StatusSnapshot {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        self.nets.refresh(true);

        let (rx_bytes, tx_bytes) = self.update_net_baseline();
        let elapsed = self
            .last_sample
            .take()
            .map_or(1.0, |t| t.elapsed().as_secs_f64())
            .max(0.001);
        let rx_bps = rx_bytes as f64 / elapsed;
        let tx_bps = tx_bytes as f64 / elapsed;
        self.last_sample = Some(std::time::Instant::now());

        let cpu_usage = self.sys.global_cpu_usage();
        let core_count = self.sys.cpus().len();
        let mem_total = self.sys.total_memory();
        let mem_used = self.sys.used_memory();

        let mut processes: Vec<ProcInfo> = self
            .sys
            .processes()
            .values()
            .filter_map(|p| {
                // Windows 的 PID 0 是 System Idle Process：它不是进程，是
                // 「没人用 CPU 的那部分时间」的记账条目，既结束不掉也没有
                // 可执行文件。留着它，进程表里就多出一行点「结束」必然失败
                // 的幽灵。macOS 的 PID 0 是 kernel_task，那是真进程（活动
                // 监视器里也列着），不能一起滤掉。
                #[cfg(windows)]
                if p.pid().as_u32() == 0 {
                    return None;
                }
                let raw_name = p.name().to_string_lossy().trim().to_string();
                if raw_name.is_empty() {
                    return None;
                }
                let exe = p.exe();
                Some(ProcInfo {
                    pid: p.pid().as_u32(),
                    start_time: p.start_time(),
                    unique_id: crate::platform::process_unique_id(p.pid().as_u32()),
                    name: process_display_name(&raw_name, exe),
                    cpu: p.cpu_usage(),
                    mem_bytes: p.memory(),
                    // exe() 对别的用户 / root 的进程会拿不到，那种情况下留 None，
                    // UI 显示首字母占位。
                    icon_source: exe.and_then(icon_source_path),
                })
            })
            .collect();
        let process_count = processes.len();
        processes.sort_by(|a, b| {
            b.cpu
                .partial_cmp(&a.cpu)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.mem_bytes.cmp(&a.mem_bytes))
        });
        // 不再截断：进程表现在是可滚动 + 可按列排序的完整列表，截断会让
        // 「按内存排序」只在 CPU 前几名里排，排出来的结果是错的。虚拟滚动
        // 保证行数多也不影响渲染开销（只构造视口内的十几行）。

        StatusSnapshot {
            cpu_usage,
            core_count,
            mem_used,
            mem_total,
            swap_used: self.sys.used_swap(),
            swap_total: self.sys.total_swap(),
            rx_bps,
            tx_bps,
            processes,
            process_count,
            thermal: crate::platform::read_thermal(),
            gpus: crate::platform::read_gpus(),
            battery: crate::platform::read_battery(),
            uptime_secs: crate::platform::system_uptime_secs(),
            os_name: os_display_name(),
            mem_total_label_bytes: mem_total,
        }
    }
}

fn is_loopback_interface(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("loopback")
        || name == "lo"
        || name
            .strip_prefix("lo")
            .is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()))
}

/// 健康分的输入。全部取自已经采到的量，不额外做系统调用。
#[derive(Clone, Copy, Debug, Default)]
pub struct HealthInputs {
    /// 当前卷的剩余空间比例（0~1）。拿不到卷信息时是 `None`，该项不扣分。
    pub disk_free_ratio: Option<f32>,
    pub mem_used: u64,
    pub mem_total: u64,
    pub swap_used: u64,
    /// 最近若干拍的**平均** CPU 占用（0~100）。用均值而不是瞬时值：
    /// 单拍冲到 90% 是正常的，持续 90% 才是问题。
    pub cpu_avg: f32,
    pub uptime_secs: u64,
}

/// 扣分最多的那一项，UI 拿它告诉用户「分是怎么掉的」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthFactor {
    Disk,
    Swap,
    Memory,
    Cpu,
    Uptime,
}

#[derive(Clone, Copy, Debug)]
pub struct HealthReport {
    /// 0~100，连续取值。
    pub score: u32,
    /// 主要扣分项；各项都没扣多少时是 `None`（系统确实没毛病）。
    pub worst: Option<HealthFactor>,
}

/// 单项的满分权重。加起来正好 100，所以「全维度都烂到底」才会得 0 分。
///
/// 配比的取舍：这是个**清理工具**，磁盘是用户唯一能在本应用里直接改善的
/// 维度，给到接近一半；内存拆成「占用率」和「换页」两项且换页权重更高，
/// 因为 macOS 会主动吃满内存做缓存和压缩，占用率高本身不代表卡——真正
/// 掉速的信号是开始动 swap；CPU 看的是持续负载；运行时长权重最低，它更像
/// 一条提示而不是病症。
const W_DISK: f32 = 45.0;
const W_SWAP: f32 = 15.0;
const W_MEM: f32 = 10.0;
const W_CPU: f32 = 20.0;
const W_UPTIME: f32 = 10.0;

/// 低于这个扣分的项不值得单独拎出来说，避免「主要扣分项：运行时长（-0.4 分）」
/// 这种噪音提示。
const WORTH_MENTIONING: f32 = 3.0;

/// 线性扣分：`value` 从 `start`（开始扣分）到 `full`（扣满 `weight`）之间
/// 线性插值，两端夹紧。
///
/// 用连续函数而不是阈值台阶：老实现是 `>=0.20 → 95 分 / >=0.10 → 80 分`，
/// 剩余空间从 20.1% 掉到 19.9% 会让分数直接砸 15 分，用户完全无从理解。
fn penalty(value: f32, start: f32, full: f32, weight: f32) -> f32 {
    if !value.is_finite() || (full - start).abs() < f32::EPSILON {
        return 0.0;
    }
    ((value - start) / (full - start)).clamp(0.0, 1.0) * weight
}

/// 系统健康分：从 100 开始按各维度的压力扣分。
///
/// 同类工具（各家「电脑管家」「体检评分」）基本都是这个扣分模型——从满分
/// 起扣，每项病症扣固定分——差别只在选哪些指标、扣多少。这里没有照搬它们
/// 常见的「启动项数量」「注册表条目」这类玄学项，只用本应用真正采得准的量。
pub fn health_report(input: HealthInputs) -> HealthReport {
    let disk = match input.disk_free_ratio {
        // 剩余 25% 以上不扣分，塞满扣满：清理工具最该盯的就是这条。
        Some(free) => penalty(1.0 - free, 0.75, 1.0, W_DISK),
        None => 0.0,
    };
    let mem_ratio = if input.mem_total > 0 {
        input.mem_used as f32 / input.mem_total as f32
    } else {
        0.0
    };
    // 80% 才开始扣：macOS 平时就把内存吃到七八成做缓存，那是设计如此。
    let memory = penalty(mem_ratio, 0.80, 1.0, W_MEM);
    // 换页量按占物理内存的比例算，绝对值在 8G 和 64G 机器上没有可比性。
    let swap_ratio = if input.mem_total > 0 {
        input.swap_used as f32 / input.mem_total as f32
    } else {
        0.0
    };
    let swap = penalty(swap_ratio, 0.01, 0.25, W_SWAP);
    let cpu = penalty(input.cpu_avg, 50.0, 95.0, W_CPU);
    let uptime = penalty(input.uptime_secs as f32 / 86_400.0, 7.0, 30.0, W_UPTIME);

    let total = disk + memory + swap + cpu + uptime;
    let score = (100.0 - total).clamp(0.0, 100.0).round() as u32;

    let worst = [
        (HealthFactor::Disk, disk),
        (HealthFactor::Swap, swap),
        (HealthFactor::Cpu, cpu),
        (HealthFactor::Memory, memory),
        (HealthFactor::Uptime, uptime),
    ]
    .into_iter()
    .filter(|(_, p)| *p >= WORTH_MENTIONING)
    .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    .map(|(factor, _)| factor);

    HealthReport { score, worst }
}

/// 进程表卡片的固定高度。和软件管理表同一套做法：列表在卡片内部自己滚，
/// 不把整页撑成一条几千像素的长卷。
pub const STATUS_PROCESS_TABLE_H: f32 = 420.0;

/// 从进程的可执行文件路径推出「该显示谁的图标」。
///
/// macOS 取**最外层**的 `.app` 祖先，不是最近的那个：Edge 的渲染进程实际躺在
/// `Microsoft Edge.app/Contents/Frameworks/.../Microsoft Edge Helper (Renderer).app/`
/// 里，最近的那个 helper bundle 根本没有图标资源，取到最外层才是用户认得的
/// 那个「Microsoft Edge」图标。Chrome / Electron 系应用是同样的套娃结构。
///
/// Windows 上图标直接从 exe 的资源段取，路径本身就是答案。
#[cfg(target_os = "macos")]
fn icon_source_path(exe: &std::path::Path) -> Option<std::path::PathBuf> {
    exe.ancestors()
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
        })
        // Ancestors 是从完整路径逐级往上走，所以「最后一个」匹配的就是最外层。
        .last()
        .map(|p| p.to_path_buf())
}

#[cfg(windows)]
fn icon_source_path(exe: &std::path::Path) -> Option<std::path::PathBuf> {
    Some(exe.to_path_buf())
}

#[cfg(not(any(target_os = "macos", windows)))]
fn icon_source_path(_exe: &std::path::Path) -> Option<std::path::PathBuf> {
    None
}

/// 进程表上的展示名。macOS 用最外层 `.app` 当应用名，套娃 helper 用内层
/// bundle 名；可执行文件名只有和它们都对不上时才拼上去（Warp 的 `stable`）。
fn process_display_name(exe_name: &str, exe: Option<&std::path::Path>) -> String {
    #[cfg(target_os = "macos")]
    {
        let Some(exe) = exe else {
            return exe_name.to_string();
        };
        let bundles: Vec<_> = exe
            .ancestors()
            .filter(|p| {
                p.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
            })
            .collect();
        if bundles.is_empty() {
            return exe_name.to_string();
        }
        let nearest = bundles[0];
        let outer = *bundles.last().unwrap();
        let app = bundle_label(outer);
        let nested = (nearest != outer).then(|| bundle_label(nearest));
        compose_process_name(exe_name, &app, nested.as_deref())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = exe;
        exe_name.to_string()
    }
}

#[cfg(target_os = "macos")]
fn bundle_label(app: &std::path::Path) -> String {
    app.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
}

#[cfg(any(target_os = "macos", test))]
fn compose_process_name(exe_name: &str, app: &str, nested_helper: Option<&str>) -> String {
    if app.is_empty() {
        return exe_name.to_string();
    }
    if let Some(helper) = nested_helper.filter(|s| !s.is_empty()) {
        if same_label(helper, app) || label_contains(helper, app) {
            return helper.to_string();
        }
        return format!("{app} {helper}");
    }
    if same_label(exe_name, app) || label_contains(exe_name, app) {
        app.to_string()
    } else {
        format!("{app} {exe_name}")
    }
}

#[cfg(any(target_os = "macos", test))]
fn same_label(a: &str, b: &str) -> bool {
    fold_label(a) == fold_label(b)
}

#[cfg(any(target_os = "macos", test))]
fn fold_label(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

#[cfg(any(target_os = "macos", test))]
fn label_contains(haystack: &str, needle: &str) -> bool {
    let hay = fold_label(haystack);
    let needle = fold_label(needle);
    !needle.is_empty() && hay.contains(&needle)
}

/// "macOS 15.6" / "Windows 11 Pro" 这种短展示名。拿不到就退回内核名。
fn os_display_name() -> String {
    let long = System::long_os_version().unwrap_or_default();
    if long.trim().is_empty() {
        System::name().unwrap_or_else(|| String::from("?"))
    } else {
        short_os_name(&long)
    }
}

/// 只留「系统名 + 版本号」，砍掉后面的 SKU / 代号。
///
/// 这个串画在健康卡片右上角的徽章里，四列布局下那格只有约 100px 宽。
/// 各平台的完整版本名都比它长：Windows 是 "Windows 11 Home China"
/// （中文版 SKU 更长），macOS 是 "macOS 15.6 Sequoia"，Linux 是
/// "Ubuntu 24.04 LTS"。真机上前者直接画到了卡片外面。
///
/// 规则：截到**第一个以数字开头的段**为止（含）。版本号后面跟着的都是
/// 修饰词，对「这是什么系统」没有增量信息。一个数字段都没有时原样返回。
fn short_os_name(long: &str) -> String {
    let mut kept = Vec::new();
    for token in long.split_whitespace() {
        kept.push(token);
        if token.starts_with(|c: char| c.is_ascii_digit()) {
            break;
        }
    }
    kept.join(" ")
}

#[cfg(test)]
mod tests {
    fn healthy() -> HealthInputs {
        HealthInputs {
            disk_free_ratio: Some(0.5),
            mem_used: 8 * 1024 * 1024 * 1024,
            mem_total: 32 * 1024 * 1024 * 1024,
            swap_used: 0,
            cpu_avg: 10.0,
            uptime_secs: 3600,
        }
    }

    /// 双显卡切换按钮上的标签要能一眼分辨，撞车就退回序号。
    #[test]
    fn gpu_labels_fall_back_to_numbers_when_vendors_collide() {
        use super::{gpu_labels, GpuReading};
        let named = |name: &str| GpuReading {
            name: Some(name.into()),
            ..GpuReading::default()
        };
        assert_eq!(
            gpu_labels(&[
                named("AMD Radeon(TM) 610M"),
                named("NVIDIA GeForce RTX 4060 Laptop GPU"),
            ]),
            vec!["AMD", "NVIDIA"]
        );
        // 同厂双卡：厂商名分不出谁是谁，退回序号。
        assert_eq!(
            gpu_labels(&[named("NVIDIA A"), named("NVIDIA B")]),
            vec!["GPU 1", "GPU 2"]
        );
        // 名字都读不到时也得有个能点的标签。
        assert_eq!(gpu_labels(&[GpuReading::default()]), vec!["GPU 1"]);
    }

    /// 徽章那格只有约 100px：完整版本名画出去过一次，别再画第二次。
    #[test]
    fn os_name_keeps_version_and_drops_the_sku() {
        use super::short_os_name;
        assert_eq!(short_os_name("Windows 11 Home China"), "Windows 11");
        assert_eq!(short_os_name("macOS 15.6 Sequoia"), "macOS 15.6");
        assert_eq!(short_os_name("Ubuntu 24.04 LTS"), "Ubuntu 24.04");
        // 版本号之前的段一个都不能丢。
        assert_eq!(short_os_name("Mac OS X 10.6 Snow Leopard"), "Mac OS X 10.6");
        // 没有版本号就原样保留，宁可长也不要截出个残缺的名字。
        assert_eq!(short_os_name("Windows"), "Windows");
    }

    #[test]
    fn healthy_system_scores_full_marks_with_nothing_to_report() {
        let r = health_report(healthy());
        assert_eq!(r.score, 100);
        assert_eq!(r.worst, None);
    }

    /// 老实现的核心毛病：剩余空间 20.1% → 95 分，19.9% → 80 分，一步砸 15 分。
    /// 换成连续函数后，相邻输入的分差必须是平滑的。
    #[test]
    fn score_moves_continuously_instead_of_jumping_between_tiers() {
        let at = |free: f32| {
            health_report(HealthInputs {
                disk_free_ratio: Some(free),
                ..healthy()
            })
            .score as i32
        };
        // 相邻 1 个百分点之间的分差上限。磁盘项是 45 分摊在 25 个百分点上，
        // 斜率约 1.8 分/%，取 3 留出舍入余量——关键是它必须远小于老实现那
        // 一步 15 分的断崖。
        const MAX_STEP: i32 = 3;
        for pct in 1..100 {
            let a = at(pct as f32 / 100.0);
            let b = at((pct + 1) as f32 / 100.0);
            assert!(
                (a - b).abs() <= MAX_STEP,
                "剩余 {pct}% → {a} 分，{}% → {b} 分，跳变过大",
                pct + 1
            );
        }
    }

    #[test]
    fn disk_pressure_dominates_and_is_reported() {
        let r = health_report(HealthInputs {
            disk_free_ratio: Some(0.02),
            ..healthy()
        });
        assert!(r.score < 65, "几乎塞满的盘不该还有 {} 分", r.score);
        assert_eq!(r.worst, Some(HealthFactor::Disk));
    }

    /// macOS 平时就把内存吃到七八成做缓存压缩，占用率高本身不是病；
    /// 真正掉速的信号是开始动 swap。两者的扣分力度必须体现这个差别。
    #[test]
    fn swap_costs_more_than_a_merely_high_memory_ratio() {
        let cached = health_report(HealthInputs {
            mem_used: 25 * 1024 * 1024 * 1024, // 78%，无换页
            ..healthy()
        });
        assert_eq!(cached.score, 100, "纯缓存吃满不该扣分");

        let swapping = health_report(HealthInputs {
            swap_used: 6 * 1024 * 1024 * 1024,
            ..healthy()
        });
        assert!(swapping.score < 92);
        assert_eq!(swapping.worst, Some(HealthFactor::Swap));
    }

    #[test]
    fn sustained_cpu_load_is_penalised() {
        let r = health_report(HealthInputs {
            cpu_avg: 95.0,
            ..healthy()
        });
        assert_eq!(r.worst, Some(HealthFactor::Cpu));
        assert!(r.score <= 80);
    }

    /// 拿不到卷信息时磁盘项不能算作「满分健康」也不能算作「爆满」，
    /// 直接不参与扣分。
    #[test]
    fn unknown_disk_does_not_move_the_score() {
        let r = health_report(HealthInputs {
            disk_free_ratio: None,
            ..healthy()
        });
        assert_eq!(r.score, 100);
    }

    #[test]
    fn worst_case_bottoms_out_at_zero_not_below() {
        let r = health_report(HealthInputs {
            disk_free_ratio: Some(0.0),
            mem_used: 32 * 1024 * 1024 * 1024,
            mem_total: 32 * 1024 * 1024 * 1024,
            swap_used: 32 * 1024 * 1024 * 1024,
            cpu_avg: 100.0,
            uptime_secs: 400 * 86_400,
        });
        assert_eq!(r.score, 0);
    }

    /// Edge / Chrome / Electron 的子进程都躺在套娃 bundle 里，最近的那层
    /// helper bundle 没有图标资源，必须取最外层才是用户认得的那个应用。
    #[cfg(target_os = "macos")]
    #[test]
    fn icon_source_takes_the_outermost_app_bundle() {
        use std::path::{Path, PathBuf};
        let nested = Path::new(
            "/Applications/Microsoft Edge.app/Contents/Frameworks/\
             Microsoft Edge Framework.framework/Versions/1.0/Helpers/\
             Microsoft Edge Helper (Renderer).app/Contents/MacOS/Microsoft Edge Helper",
        );
        assert_eq!(
            icon_source_path(nested),
            Some(PathBuf::from("/Applications/Microsoft Edge.app"))
        );

        // 普通单层应用
        assert_eq!(
            icon_source_path(Path::new(
                "/Applications/Telegram.app/Contents/MacOS/Telegram"
            )),
            Some(PathBuf::from("/Applications/Telegram.app"))
        );

        // 守护进程 / 命令行工具不属于任何 .app，交给 UI 回退首字母
        assert_eq!(icon_source_path(Path::new("/usr/sbin/cfprefsd")), None);
        assert_eq!(icon_source_path(Path::new("/bin/zsh")), None);
    }

    #[test]
    fn process_display_name_uses_outer_app_and_keeps_distinct_helpers() {
        assert_eq!(
            compose_process_name("Telegram", "Telegram", None),
            "Telegram"
        );
        assert_eq!(compose_process_name("stable", "Warp", None), "Warp stable");
        assert_eq!(
            compose_process_name("quick-cleaner", "QuickCleaner", None),
            "QuickCleaner"
        );
        assert_eq!(
            compose_process_name(
                "Microsoft Edge Helper",
                "Microsoft Edge",
                Some("Microsoft Edge Helper (Renderer)"),
            ),
            "Microsoft Edge Helper (Renderer)"
        );
        assert_eq!(
            compose_process_name(
                "Termius Helper (GPU)",
                "Termius",
                Some("Termius Helper (GPU)"),
            ),
            "Termius Helper (GPU)"
        );
        assert_eq!(
            compose_process_name("Helper (GPU)", "Termius", Some("Helper (GPU)")),
            "Termius Helper (GPU)"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn process_display_name_reads_bundle_from_the_exe_path() {
        use std::path::Path;
        assert_eq!(
            process_display_name(
                "stable",
                Some(Path::new("/Applications/Warp.app/Contents/MacOS/stable")),
            ),
            "Warp stable"
        );
        assert_eq!(
            process_display_name(
                "Telegram",
                Some(Path::new(
                    "/Applications/Telegram.app/Contents/MacOS/Telegram"
                )),
            ),
            "Telegram"
        );
        assert_eq!(
            process_display_name(
                "Microsoft Edge Helper",
                Some(Path::new(
                    "/Applications/Microsoft Edge.app/Contents/Frameworks/\
                     Microsoft Edge Framework.framework/Versions/1.0/Helpers/\
                     Microsoft Edge Helper (Renderer).app/Contents/MacOS/Microsoft Edge Helper",
                )),
            ),
            "Microsoft Edge Helper (Renderer)"
        );
        assert_eq!(
            process_display_name("zsh", Some(Path::new("/bin/zsh"))),
            "zsh"
        );
    }

    use super::*;

    #[test]
    fn fan_mode_supports_all_three_ui_choices() {
        assert!(FanMode::Auto.is_supported());
        assert!(FanMode::Percent(60).is_supported());
        assert!(FanMode::Percent(100).is_supported());
        assert!(!FanMode::Percent(59).is_supported());
    }

    /// 采样器至少要能连续采两拍且不出错；单测跑在 CI 的 macOS/Windows
    /// 上，这里只验证结构成立，不锁定数值。
    #[test]
    fn sampler_produces_snapshots() {
        let mut s = StatusSampler::new();
        std::thread::sleep(std::time::Duration::from_millis(
            sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.as_millis() as u64 + 50,
        ));
        let snap = s.sample();
        assert!(snap.core_count > 0);
        assert!(snap.mem_total > 0);
        assert!((0.0..=100.0 * snap.core_count as f32).contains(&snap.cpu_usage));
        assert!(!snap.processes.is_empty(), "系统至少应有几个进程");
        for p in &snap.processes {
            assert!(p.pid > 0);
        }
    }

    #[test]
    fn loopback_names_do_not_hide_regular_windows_adapters() {
        assert!(is_loopback_interface("lo"));
        assert!(is_loopback_interface("lo0"));
        assert!(is_loopback_interface("Loopback Pseudo-Interface 1"));
        assert!(!is_loopback_interface("Local Area Connection"));
        assert!(!is_loopback_interface("lower-deck-ethernet"));
    }
}
