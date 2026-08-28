//! 清理目标的占用检测：正在运行的应用 + 被进程打开的文件。
//!
//! 目标是给每个 `ScanItem` 两种徽标（对标商业清理工具的「应用打开中 /
//! 系统占用」），并让清理入口跳过这些目标——macOS 允许删除正被打开的文件，
//! 不拦的话清"成功"了，应用却在写一个已消失的路径。
//!
//! 性能边界：扫描阶段的整轮检测只有两次子进程调用——`ps -axo
//! pid=,ppid=,comm=` 一次拿全部进程快照（毫秒级），`lsof -F0n` 一次列全部
//! 打开文件（本机实测约 15 秒、2 万条打开路径），加上 O(打开文件数 × 路径
//! 深度) 的哈希查表。检测在后台线程与扫描并发跑，且结果**不阻塞**首屏、
//! 也不推迟第二阶段发现式扫描——列表先出，徽标在检测完成后合并进条目
//! （见 `ui::actions::junk` 的扫描任务编排）。
//!
//! **fail closed**：`ps`/`lsof` 调用失败、超时、非零退出码、输出被截断，
//! 都不能悄悄当成"啥也没测到 = 都不忙"处理——以前 `lsof` 一失败就返回
//! 空表，效果等价于"全部空闲"，恰好是最危险的默认值。为此 `Busy` 是三态
//! （`app` / `open` / `unknown`），`unknown` 按 `open` 同等严格程度拦截，
//! 详见下面的定义。
//!
//! **删除边界的定点复检**：扫描阶段的占用快照有十几秒到几分钟的陈旧期，
//! 从检测完成到用户点下"清理"之间，应用完全可能刚刚启动。`spot_check`
//! 在 macOS 对文件做精确查询、对目录用 `lsof +D` 递归查询；后者不能省略——普通
//! 路径参数只匹配目录句柄，抓不到目录内部新打开的文件。其他平台没有
//! 句柄级检测，退化为再问一次活数据库。在 `cleaner::clean_targets` 和
//! `clean_arbitrary_items` 入口调用，发现新占用或者复检本身测不出，一律
//! 直接拒删。

use crate::core::i18n::{bilingual, Text};
use crate::core::scanner::CategorySummary;
use std::collections::HashMap;
#[cfg(any(target_os = "macos", test))]
use std::path::Path;
use std::path::PathBuf;

#[cfg(target_os = "macos")]
use std::collections::HashSet;
#[cfg(target_os = "macos")]
use std::ffi::{OsStr, OsString};
#[cfg(target_os = "macos")]
use std::process::Command;
#[cfg(target_os = "macos")]
use std::time::Duration;

/// 一个目标的占用状态：三态，`unknown` 是"测不出"专用状态。
///
/// `app` 是按目录名推出来的归属应用（正在运行）；`open` 表示上一轮
/// `lsof` 全表扫描确实看到有进程打开着目标子树内的路径；`unknown` 表示
/// 那一轮 `lsof` 调用本身失败、超时、非零退出或者输出被截断——**测不出
/// 不等于没事**，所以 `unknown` 按 `open` 同等严格程度处理（见
/// `is_empty` 与 `ui::actions::clean::start_clean` 的拦截逻辑，二者都不
/// 区分 `open` 和 `unknown`）。三者可同时成立，徽标优先展示 `app`——它
/// 对用户更有解释力。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Busy {
    pub app: Option<String>,
    pub open: bool,
    /// 占用检测本身失败/超时/输出不完整，测不出真实状态。按占用处理。
    pub unknown: bool,
}

impl Busy {
    fn is_empty(&self) -> bool {
        self.app.is_none() && !self.open && !self.unknown
    }

    /// 徽标文案：`Some((文案, 是否应用级))`。
    ///
    /// 这里的 `bool` 以前的文档说"应用级徽标是提示（可强勾），纯 `open`
    /// 徽标偏阻断语义"，暗示两者在清理时会被区别对待——这与实际代码不符：
    /// `ui::actions::clean::start_clean` 对 `busy.is_some()` 的条目一视
    /// 同仁地从勾选里剔除，不管是 `app`、`open` 还是 `unknown`。这个
    /// `bool` 现在只是 UI 配色的开关（应用级用暖色提示、其余用错误色），
    /// 不代表"可以强行清理"。
    pub fn badge(&self) -> Option<(Text, bool)> {
        if let Some(app) = &self.app {
            return Some((
                bilingual(|l| match l {
                    crate::core::i18n::Language::Zh => format!("应用打开中 · {app}"),
                    crate::core::i18n::Language::En => format!("{app} running"),
                }),
                true,
            ));
        }
        if self.open {
            return Some((
                bilingual(|l| match l {
                    crate::core::i18n::Language::Zh => String::from("系统占用"),
                    crate::core::i18n::Language::En => String::from("In use"),
                }),
                false,
            ));
        }
        if self.unknown {
            return Some((
                bilingual(|l| match l {
                    crate::core::i18n::Language::Zh => String::from("占用状态未知"),
                    crate::core::i18n::Language::En => String::from("Busy status unknown"),
                }),
                false,
            ));
        }
        None
    }
}

/// 对全部固定目标跑一轮占用检测。只在 macOS 有实现，其余平台返回空表
/// （徽标不显示，清理也不跳过——行为与没有这个模块时完全一致；这是
/// "未实现"的既定契约，不是探测失败，所以不产出 `unknown`）。
pub fn detect(targets: &[PathBuf]) -> HashMap<PathBuf, Busy> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = targets;
        HashMap::new()
    }
    #[cfg(target_os = "macos")]
    detect_macos(targets)
}

/// 把检测结果合入扫描条目：写 `busy`，并给被占用的条目降级 `recommended`。
///
/// 返回受影响的条目数。降级而不是在勾选层过滤，是为了让
/// `selection_is_recommended` 这类「勾选态是否等于推荐态」的比对天然一致；
/// 调用方（UI）负责在需要时重新执行 `select_recommended`。
pub fn apply_busy(categories: &mut [CategorySummary], busy: &HashMap<PathBuf, Busy>) -> usize {
    let mut n = 0;
    for cat in categories {
        for item in &mut cat.items {
            if let Some(b) = busy.get(&item.path) {
                if !b.is_empty() {
                    item.busy = Some(b.clone());
                    item.recommended = false;
                    n += 1;
                }
            }
        }
    }
    n
}

/// 定点复检结果：给定目标"现在"是否仍然干净可删。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpotCheck {
    /// 复检没发现任何进程打开着这个目标。
    Clear,
    /// 复检发现有进程正打开着——扫描完成之后新起的应用/新写入的文件。
    Busy,
    /// 复检本身没跑成（lsof 调用失败/超时/非零退出/输出截断），测不出
    /// 就不能当"没事"，按占用处理，与 `Busy::unknown` 同一套 fail-closed
    /// 逻辑。
    Unknown,
}

/// 只对给定的一小批路径做占用复检，不做全表扫描。
///
/// 背景见模块顶部：扫描阶段的占用检测是十几秒到几分钟前的快照，从检测
/// 完成到用户点「清理」，中间应用完全可能启动。文件按给定路径精确匹配；
/// 目录必须用 `+D` 递归匹配，否则只打开目录内文件、不持有目录句柄的进程
/// 会被漏掉。`+D` 比精确查询贵，因此仍分批并受超时保护；超时按 Unknown
/// 拒删，不会为了速度退回不完整的精确匹配。
///
/// macOS 用 lsof 定点复检。其他平台没有句柄级打开文件检测，退化为以下
/// 规则：对**文件**再问一次 [`crate::core::safety::is_live_database`]（必须
/// 主库 + 伴随文件才算活库）；目录不在这里拦——目录级「顶层任意 .db」
/// 过宽，会把整棵缓存根判成 Busy，嵌套活库由 `delete_tree` 的家族闸门
/// 兜底；已经消失的路径判 Clear。
pub fn spot_check(paths: &[PathBuf]) -> HashMap<PathBuf, SpotCheck> {
    #[cfg(target_os = "macos")]
    {
        spot_check_macos(paths)
    }
    #[cfg(not(target_os = "macos"))]
    {
        spot_check_fallback(paths)
    }
}

#[cfg(not(target_os = "macos"))]
fn spot_check_fallback(paths: &[PathBuf]) -> HashMap<PathBuf, SpotCheck> {
    paths
        .iter()
        .map(|p| {
            let status = match std::fs::symlink_metadata(p) {
                Err(_) => SpotCheck::Clear,
                Ok(md) if md.is_file() && crate::core::safety::is_live_database(p) => {
                    SpotCheck::Busy
                }
                Ok(_) => SpotCheck::Clear,
            };
            (p.clone(), status)
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn detect_macos(targets: &[PathBuf]) -> HashMap<PathBuf, Busy> {
    // lsof 报的是内核解析后的真实路径（/var/folders vs /private/var/
    // folders），目标表里的路径必须先 canonicalize 一份，否则 per-user
    // 临时目录一族永远匹配不上。映射的值仍是目标表里的原始路径——结果
    // 表的键必须与 ScanItem.path 对齐。canonicalize 失败（悬空/权限）就
    // 只留原路径自身。
    let mut want: HashMap<PathBuf, PathBuf> = HashMap::new();
    for t in targets {
        match t.canonicalize() {
            Ok(c) => {
                want.insert(c, t.clone());
            }
            Err(_) => {
                want.insert(t.clone(), t.clone());
            }
        }
    }

    let mut result: HashMap<PathBuf, Busy> = HashMap::new();

    let Some(procs) = ps_snapshot() else {
        // ps 本身失败：它是 owning_app 唯一的数据源，静默返回空表会让
        // 应用级占用检测整轮失明，却对外报告成"没有应用在用"——和 lsof
        // 全表扫描失败是同一个问题，只是换了个数据源。ps 还顺带给
        // lsof 那边的自我遮蔽提供子进程排除表，两个用途共享同一个失败
        // 点，没必要硬撑着只跑一半：直接按 fail closed 处理，整轮标记
        // 未知，不再尝试 lsof。
        mark_all_unknown(&mut result, targets);
        return result;
    };

    let procs_lower: Vec<String> = procs.iter().map(|r| r.comm_lower.clone()).collect();
    for path in targets {
        if let Some(app) = owning_app(path, &procs_lower) {
            result.entry(path.clone()).or_default().app = Some(app);
        }
    }

    let excluded = self_and_descendant_pids(std::process::id(), &procs);
    match open_file_paths(&excluded) {
        Some(open_paths) => {
            for open in open_paths {
                // 从打开的路径沿父链向上找目标：打开的是目标内部的文件时，
                // 目标本身就是它的某个祖先。深度 ≈ 路径层级，单次查表 O(1)。
                let mut cur = open;
                loop {
                    if let Some(raw) = want.get(&cur) {
                        result.entry(raw.clone()).or_default().open = true;
                        break;
                    }
                    if !cur.pop() {
                        break;
                    }
                }
            }
        }
        None => {
            // 全表扫描没跑成：不知道到底有没有人在用任何一个目标，按
            // "全部未知"处理——宁可这一轮多拦几个，也不要把"测不出"
            // 悄悄等同于"确实空闲"。
            mark_all_unknown(&mut result, targets);
        }
    }
    result
}

/// fail-closed 兜底：把给定目标全部标记为 `unknown`，不覆盖已有的
/// `app`/`open` 信息（用 `or_default()` 取到的条目上再置位）。抽成独立
/// 函数是为了不用真的让 `/bin/ps`/`lsof` 失败就能直接单测这条分支。
#[cfg(target_os = "macos")]
fn mark_all_unknown(result: &mut HashMap<PathBuf, Busy>, targets: &[PathBuf]) {
    for t in targets {
        result.entry(t.clone()).or_default().unknown = true;
    }
}

/// 单次 `ps` 快照里的一条进程记录：pid/ppid 用于定位"本进程 fork 出来的
/// 子进程"，`comm_lower` 用于 `owning_app` 的形态匹配。
#[cfg(target_os = "macos")]
struct ProcRecord {
    pid: u32,
    ppid: u32,
    comm_lower: String,
}

/// 全部进程的 pid/ppid/可执行路径快照。一次 `ps` 调用，毫秒级。
///
/// 以前只取 `comm=`（可执行路径），后来发现"本进程 fork 出的测量子进程
/// 让目标误判成占用"这个缺口必须知道进程的父子关系才能堵——干脆一次性
/// 把 pid/ppid 也要出来，仍然只有一次 `ps` 调用，不增加子进程数。
///
/// 返回 `None` 表示这一轮测不出（`ps_output_is_usable` 判定失败/非零
/// 退出码/输出为空或被截断），调用方必须按 fail closed 处理。
#[cfg(target_os = "macos")]
fn ps_snapshot() -> Option<Vec<ProcRecord>> {
    let out = Command::new("/bin/ps").args(["-axo", "pid=,ppid=,comm="]).output().ok()?;
    if !ps_output_is_usable(out.status.success(), &out.stdout) {
        return None;
    }
    Some(
        out.stdout
            .split(|&b| b == b'\n')
            .filter_map(|line| std::str::from_utf8(line).ok())
            .filter_map(parse_ps_line)
            .collect(),
    )
}

/// `ps` 输出是否可信：退出码必须是 0；stdout 不能是空的——一台正常运行
/// 的机器上，`ps -ax` 至少会列出 `ps` 自己和 pid 1，空输出只可能是调用
/// 出了问题；stdout 还必须以换行收尾，否则最后一行大概率是被截断的半截
/// 记录。三条判据和 `looks_complete`（lsof 那边的完整性检查）是同一套
/// 思路，只是换了个输出格式。
#[cfg(target_os = "macos")]
fn ps_output_is_usable(status_success: bool, stdout: &[u8]) -> bool {
    status_success && !stdout.is_empty() && stdout.last() == Some(&b'\n')
}

/// 解析 `ps -axo pid=,ppid=,comm=` 的一行。
///
/// 列之间用不定数量的空格对齐，`comm` 本身还可能带空格（`Google Chrome
/// Helper` 这类），不能整行按空白切分——只在 pid 和 ppid 后各切一次，
/// 剩下原样交给 `comm`。
#[cfg(target_os = "macos")]
fn parse_ps_line(line: &str) -> Option<ProcRecord> {
    let s = line.trim_start();
    let (pid_str, rest) = s.split_once(char::is_whitespace)?;
    let pid: u32 = pid_str.parse().ok()?;
    let rest = rest.trim_start();
    let (ppid_str, comm) = rest.split_once(char::is_whitespace)?;
    let ppid: u32 = ppid_str.parse().ok()?;
    let comm = comm.trim_start();
    if comm.is_empty() {
        return None;
    }
    Some(ProcRecord {
        pid,
        ppid,
        comm_lower: comm.to_lowercase(),
    })
}

/// 本进程及其全部子孙进程的 pid 集合。
///
/// 占用检测/定点复检都可能在跑的同时另外 fork 子进程去测量某个目录
/// （比如 `du -sk` 探测一个缓存的真实大小），那个子进程的命令行/打开的
/// 文件里天然带着被测目录，`lsof` 会把它算成"有人打开着"——不排除的话，
/// 越是测得慢的目标越容易被误判成占用（`extract_lsof_paths` 原来只排了
/// `self_pid` 一个，漏掉了这一层子进程）。BFS 是因为子进程还可能再起
/// 子进程（比如经由 shell 包装启动的外部命令）。
#[cfg(target_os = "macos")]
fn self_and_descendant_pids(self_pid: u32, procs: &[ProcRecord]) -> HashSet<u32> {
    let mut set = HashSet::new();
    set.insert(self_pid);
    loop {
        let before = set.len();
        for rec in procs {
            if set.contains(&rec.ppid) {
                set.insert(rec.pid);
            }
        }
        if set.len() == before {
            break;
        }
    }
    set
}

/// 候选词下限：目录名逐级剥壳可能产出很短或很通用的词（比如
/// `notion.id.ShipIt` 剥完后段间还剩一个孤零零的 "id"）。这类词就算真的
/// 撞上了某个 `/{词}.app/` 形态的进程路径也没有区分度——命中的是运气好，
/// 不是识别对了。这里收紧的是候选词本身的准入门槛，匹配方式仍然是原来的
/// "进程可执行路径落在一个同名 .app 包里"，不是换算法。
#[cfg(target_os = "macos")]
const MIN_CANDIDATE_LEN: usize = 4;

/// 即便长度达标，这几个词在缓存目录名里也太通用，撞上同名 `.app` 纯属
/// 巧合概率不可忽略，直接排除在候选词之外。
#[cfg(target_os = "macos")]
const GENERIC_CANDIDATE_WORDS: &[&str] = &["default", "updater", "helper", "service", "data"];

#[cfg(target_os = "macos")]
fn is_usable_candidate(low: &str) -> bool {
    low.len() >= MIN_CANDIDATE_LEN && !GENERIC_CANDIDATE_WORDS.contains(&low)
}

/// 从目录名推归属应用：`/<stem>.app/` 形态的进程路径才算命中。
///
/// 候选词由目录名逐级剥壳得到：`@zcodedesktop-updater` → `zcodedesktop`，
/// `notion.id.ShipIt` → `notion.id` → 再按 `.` 取首段 → `notion`，
/// `com.google.antigravity` → 末段 → `antigravity`。命中哪个候选词，就用
/// 它的原文（非小写）做展示名。推不出候选词的目录（go-build、Logs 之类）
/// 自然没有候选，永不误报。候选词还要过 `is_usable_candidate` 的下限/
/// 黑名单一关，见其文档。
#[cfg(target_os = "macos")]
fn owning_app(target: &Path, procs_lower: &[String]) -> Option<String> {
    let name = target.file_name()?.to_string_lossy().into_owned();
    let stem = name
        .strip_suffix("-updater")
        .or_else(|| name.strip_suffix(".ShipIt"))
        .unwrap_or(&name)
        .trim_start_matches('@')
        .to_owned();
    if stem.is_empty() {
        return None;
    }

    let mut candidates: Vec<(String, String)> = vec![(stem.to_lowercase(), stem.clone())];
    if let Some((first, _)) = stem.split_once('.') {
        candidates.push((first.to_lowercase(), first.to_owned()));
    }
    if let Some((_, last)) = stem.rsplit_once('.') {
        candidates.push((last.to_lowercase(), last.to_owned()));
    }
    candidates.sort_by_key(|(low, _)| std::cmp::Reverse(low.len()));
    candidates.dedup_by(|a, b| a.0 == b.0);
    candidates.retain(|(low, _)| is_usable_candidate(low));

    // 最长候选词优先：`notion.id` 整体撞不上时才轮到 `notion`，避免短词
    // 抢先把展示名截短。
    candidates
        .iter()
        .find(|(low, _)| {
            procs_lower
                .iter()
                .any(|p| p.contains(&format!("/{low}.app/")))
        })
        .map(|(_, display)| display.clone())
}

/// 全表扫描允许的超时。本机实测约 15 秒是常态，网络卷或者打开文件数
/// 特别多的机器可能拖到更久；给 3 倍经验余量，既不把正常慢速探测误判成
/// "测不出"，也兜住真正失控/悬挂的 `lsof`。
#[cfg(target_os = "macos")]
const FULL_SCAN_TIMEOUT: Duration = Duration::from_secs(45);

/// 定点复检允许的超时。目录目标使用 `+D` 递归枚举，可能明显慢于文件精确
/// 查询；超时只会让该批变成 Unknown 并拒删，不会放行。
#[cfg(target_os = "macos")]
const SPOT_CHECK_TIMEOUT: Duration = Duration::from_secs(3);

/// 一次 `lsof` 调用的结果。exit 状态单独存着，是因为 `+D` 模式下空结果和
/// 命中都可能返回 1，而被信号终止则没有退出码；必须结合 stdout/stderr
/// 判断，见 `spot_check_macos` 里的说明。带超时的执行本身在 `core::proc`；
/// 那份实现里有管道死锁与僵尸进程两个陷阱，残留清理的 `mdfind` 反查也要
/// 用同一套，所以不在这里留第二份拷贝。
#[cfg(target_os = "macos")]
use crate::core::proc::{run_with_timeout, ProcRun as LsofRun};

/// 带超时地跑一次 `lsof`。
#[cfg(target_os = "macos")]
fn run_lsof<S: AsRef<OsStr>>(args: &[S], timeout: Duration) -> Option<LsofRun> {
    run_with_timeout("/usr/sbin/lsof", args, timeout)
}

/// `-F0` 格式里每个字段都以 NUL 结尾；lsof 自己还会在整个输出末尾加一个
/// `\n`。正常收尾的输出去掉那个末尾换行后，最后一个字节一定是 NUL。如果
/// 不是，说明输出在写入/管道读取途中被截断——半截数据比没数据更危险，
/// 它可能恰好在一条"正忙"记录中间断掉，被下游误解析成"空闲"。
#[cfg(target_os = "macos")]
fn looks_complete(stdout: &[u8]) -> bool {
    match stdout.strip_suffix(b"\n").unwrap_or(stdout) {
        [] => true,
        s => s.last() == Some(&0),
    }
}

/// 全部进程当前打开的文件路径。一次 `lsof` 调用，本机实测约 15 秒——
/// 全程被首屏渲染与第二阶段发现式扫描掩盖，不占用用户等待时间。
///
/// 返回 `None` 表示这一轮"测不出"（调用失败/超时/非零退出/输出截断），
/// 调用方必须按 fail closed 处理，不能当成"没有任何打开文件"。
#[cfg(target_os = "macos")]
fn open_file_paths(excluded_pids: &HashSet<u32>) -> Option<Vec<PathBuf>> {
    let run = run_lsof(&["-F0n", "-w"], FULL_SCAN_TIMEOUT)?;
    if !run.ok || !looks_complete(&run.stdout) {
        return None;
    }
    Some(extract_lsof_paths(&run.stdout, excluded_pids))
}

/// 解析 `lsof -F0n` 输出：字段以字段字符开头、`\0` 分隔，进程组以 `p<pid>`
/// 起头，只关心其中的 `n`（路径）；非绝对路径（socket 别名、内核内部名）丢弃。
///
/// **必须跳过"自己"这一整组 pid**，且"自己"不只是 `self_pid` 一个：占用
/// 检测/定点复检都可能与本进程 fork 出的测量子进程（比如 `du -sk` 探测
/// 某个缓存的真实大小）并发跑，那个子进程持有目标目录的句柄，父链回溯
/// 会把我们自己正在称重的目录标成占用。实测一个正在遍历 `~` 的 `find`
/// 进程，lsof 报了 8 条绝对路径，其中一条就是 `~/go/pkg/mod/...`——那是
/// 本工具自己的 PackageCache 目标。不排除的后果是"越大/越慢的缓存越容易
/// 被自己误判成占用"，表现为同一条目这次预选、下次不预选。`excluded_pids`
/// 由调用方传入本进程 + 全部子孙进程的 pid 集合。
#[cfg(target_os = "macos")]
fn extract_lsof_paths(bytes: &[u8], excluded_pids: &HashSet<u32>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut skipping = false;
    for field in bytes.split(|&b| b == 0) {
        match field.first() {
            // 进程组边界：之后每个 `n` 都属于这个 pid，直到下一个 `p`
            Some(&b'p') => {
                skipping = std::str::from_utf8(&field[1..])
                    .ok()
                    .and_then(|s| s.trim().parse::<u32>().ok())
                    .is_some_and(|pid| excluded_pids.contains(&pid));
            }
            Some(&b'n') if !skipping => {
                if let Some(path) = std::str::from_utf8(&field[1..])
                    .ok()
                    .filter(|s| s.starts_with('/'))
                {
                    out.push(PathBuf::from(path));
                }
            }
            _ => {}
        }
    }
    out
}

/// 一批定点复检最多带几个路径。纯粹是为了不把命令行拼到系统 `ARG_MAX`
/// 上限——真要一次清理上千个目标时才用得上分批，普通场景一批就够。
#[cfg(target_os = "macos")]
const SPOT_CHECK_BATCH: usize = 50;

#[cfg(target_os = "macos")]
fn is_recursive_spot_target(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

/// 递归目录不能和其它目标共享一次调用：一个巨型 `+D` 目录超时，不应把
/// 同批几十个本来毫秒级可查完的文件一起拖成 Unknown。文件仍按 50 个一批。
#[cfg(target_os = "macos")]
fn spot_check_batches(paths: Vec<PathBuf>) -> Vec<Vec<PathBuf>> {
    let mut batches = Vec::new();
    let mut files = Vec::new();
    for path in paths {
        if is_recursive_spot_target(&path) {
            if !files.is_empty() {
                batches.push(std::mem::take(&mut files));
            }
            batches.push(vec![path]);
        } else {
            files.push(path);
            if files.len() == SPOT_CHECK_BATCH {
                batches.push(std::mem::take(&mut files));
            }
        }
    }
    if !files.is_empty() {
        batches.push(files);
    }
    batches
}

/// 构造一批 lsof 参数：普通文件精确匹配，真实目录用 `+D` 递归匹配。
///
/// `lsof <目录>` 只会命中目录句柄，不会命中目录树内部被打开的文件；这里
/// 不能为了保留“毫秒级”假设而省略 `+D`，否则删除边界复检对最常见的
/// `remove_dir: false` 缓存目录形同虚设。
#[cfg(target_os = "macos")]
fn spot_check_args(batch: &[PathBuf]) -> Vec<OsString> {
    // 定点复检故意不用 `-w`：+D 遍历中若有目录无权读取，warning 是“结果
    // 不完整”的唯一证据，必须通过 stderr 让下游判 Unknown，不能压掉后把
    // exit 1 + 空输出误认成确实无人占用。
    let mut args: Vec<OsString> = vec!["-F0n".into()];
    for path in batch {
        if is_recursive_spot_target(path) {
            args.push("+D".into());
        }
        args.push(path.as_os_str().to_owned());
    }
    args
}

/// lsof 定点查询的结果是否完整可用。
///
/// `+D` 模式实测即使命中目录内部的打开文件也会正常返回 exit 1，因此不能
/// 把 exit 1 一概当成“空结果”，必须继续解析 stdout；exit 0/1、空 stderr、
/// 完整字段同时成立才可信。被信号终止时 `exit_code=None`，即使 stdout
/// 恰好停在一个完整字段边界也不能据此放行。
#[cfg(target_os = "macos")]
fn spot_lsof_result_is_usable(run: &LsofRun) -> bool {
    matches!(run.exit_code, Some(0 | 1))
        && run.stderr.iter().all(u8::is_ascii_whitespace)
        && looks_complete(&run.stdout)
}

#[cfg(target_os = "macos")]
fn mark_spot_open_paths(
    result: &mut HashMap<PathBuf, SpotCheck>,
    want: &HashMap<PathBuf, PathBuf>,
    open_paths: impl IntoIterator<Item = PathBuf>,
) {
    for open in open_paths {
        // +D 报告的是目录内部实际打开的文件，不是传给 lsof 的目标目录；
        // 沿父链回溯才能映射回 CleanTarget。不要命中后 break：若目标存在
        // 父子重叠，两者都会受这条打开文件影响，都必须阻断。
        let mut current = open;
        loop {
            if let Some(raw) = want.get(&current) {
                result.insert(raw.clone(), SpotCheck::Busy);
            }
            if !current.pop() {
                break;
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn spot_check_macos(paths: &[PathBuf]) -> HashMap<PathBuf, SpotCheck> {
    // 默认全部 Clear，后面只在发现占用/测不出的时候覆盖。
    let mut result: HashMap<PathBuf, SpotCheck> =
        paths.iter().cloned().map(|p| (p, SpotCheck::Clear)).collect();

    // 不存在的路径（已经被删掉、还没落地，或者压根不是真实文件系统路径——
    // 比如 Docker 镜像/本地快照编码出来的虚拟"路径"）没法用名字去问内核
    // 谁打开着它，按定义就是干净的。更重要的是：这类路径一旦混进 lsof
    // 的参数列表，lsof 会对整次调用报 "status error" 并把退出码置成非零，
    // 拖累同一批里其它本来正常的路径被误判成"测不出"，所以必须提前滤掉，
    // 不能指望 lsof 自己优雅地跳过。
    let existing: Vec<PathBuf> = paths
        .iter()
        .filter(|p| p.symlink_metadata().is_ok())
        .cloned()
        .collect();
    if existing.is_empty() {
        return result;
    }

    // ps 失败时退化成只排除 self_pid（`self_and_descendant_pids` 对空
    // 进程表的行为本就如此），不必因为排不出子孙进程就把整批复检判成
    // unknown——定点复检不会 fork 子进程去测量正在删除的路径本身，
    // 排除表不完整顶多让个别路径被过度保守地多拦一次，不是"把占用误判
    // 成空闲"的数据丢失风险，跟 detect_macos 里 ps 失败必须 fail closed
    // 不是同一类问题。
    let procs = ps_snapshot().unwrap_or_default();
    let excluded = self_and_descendant_pids(std::process::id(), &procs);

    // 同 detect_macos：lsof 报内核解析后的真实路径，回查前先各自
    // canonicalize 一份。
    let mut want: HashMap<PathBuf, PathBuf> = HashMap::new();
    for p in &existing {
        match p.canonicalize() {
            Ok(c) => {
                want.insert(c, p.clone());
            }
            Err(_) => {
                want.insert(p.clone(), p.clone());
            }
        }
    }

    for batch in spot_check_batches(existing) {
        let args = spot_check_args(&batch);

        match run_lsof(&args, SPOT_CHECK_TIMEOUT) {
            None => {
                // 连子进程都没跑起来/等到超时被杀掉：这一批全部测不出。
                for p in &batch {
                    result.insert(p.clone(), SpotCheck::Unknown);
                }
            }
            Some(run) if spot_lsof_result_is_usable(&run) => {
                // exit 1 既可能是“没有匹配”，也可能带着 +D 找到的记录；
                // 两种情况都要解析 stdout，不能只看退出状态。
                mark_spot_open_paths(
                    &mut result,
                    &want,
                    extract_lsof_paths(&run.stdout, &excluded),
                );
            }
            Some(_) => {
                // 其它退出状态、错误输出或截断输出都测不出。尤其被信号终止
                // 时 exit_code=None，空 stderr 也不能维持默认 Clear。
                for p in &batch {
                    result.insert(p.clone(), SpotCheck::Unknown);
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn parses_lsof_field_output() {
        let raw =
            b"p123\0cnode\0n/Users/x/a.txt\0p456\0nsocket\0n/private/var/folders/x/T\0nrelative\0\0";
        let none: HashSet<u32> = HashSet::new();
        // 谁都不排除：两组都收，非绝对路径丢掉
        assert_eq!(
            extract_lsof_paths(raw, &none),
            vec![
                PathBuf::from("/Users/x/a.txt"),
                PathBuf::from("/private/var/folders/x/T"),
            ]
        );
        // 456 在排除集合里：那一组的路径整组丢掉，包括 cwd 与 DIR 句柄
        let excl: HashSet<u32> = [456].into_iter().collect();
        assert_eq!(extract_lsof_paths(raw, &excl), vec![PathBuf::from("/Users/x/a.txt")]);
        // pid 解析不出来（异常输出）时按"不是被排除的"处理，宁可多收不误漏
        assert_eq!(extract_lsof_paths(b"p??\0n/Users/x/b.txt\0", &none).len(), 1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn extract_lsof_paths_excludes_self_and_descendants() {
        // 100 是本进程，200 是本进程 fork 出来测量目录大小的子进程；
        // 300 是无关进程。descendant 排除生效时，前两组都该被丢掉。
        let raw =
            b"p100\0n/Users/x/self-cwd\0p200\0n/Users/x/Library/Caches/com.foo\0p300\0n/Users/x/other.txt\0";
        let excluded: HashSet<u32> = [100, 200].into_iter().collect();
        assert_eq!(
            extract_lsof_paths(raw, &excluded),
            vec![PathBuf::from("/Users/x/other.txt")]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn open_file_marks_target_via_ancestors() {
        let dir = std::env::temp_dir().join(format!("qc_inuse_{}", std::process::id()));
        let inner = dir.join("sub/deep/file.bin");
        std::fs::create_dir_all(inner.parent().unwrap()).unwrap();
        std::fs::write(&inner, b"x").unwrap();

        // 模拟 detect_macos 的 want 映射与父链回溯。lsof 报告的是内核
        // canonical 路径，所以回溯也从 canonical 形态出发。
        let canonical = dir.canonicalize().unwrap();
        let mut want: HashMap<PathBuf, PathBuf> = HashMap::new();
        want.insert(canonical.clone(), dir.clone());
        let mut result: HashMap<PathBuf, Busy> = HashMap::new();
        for open in [canonical.join("sub/deep/file.bin"), canonical.join("sub")] {
            let mut cur = open;
            loop {
                if let Some(raw) = want.get(&cur) {
                    result.entry(raw.clone()).or_default().open = true;
                    break;
                }
                if !cur.pop() {
                    break;
                }
            }
        }
        assert!(result.get(&dir).unwrap().open);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn owning_app_matches_by_dir_name_stem() {
        let procs = vec![
            "/applications/microsoft edge.app/contents/macos/microsoft edge".into(),
            "/applications/notion.app/contents/macos/notion".into(),
            "/applications/antigravity.app/contents/macos/antigravity".into(),
            "/system/applications/safari.app/contents/macos/safari".into(),
        ];
        let cases = [
            (
                "/Users/x/Library/Caches/Microsoft Edge",
                Some("Microsoft Edge"),
            ),
            ("/Users/x/Library/Caches/notion-updater", Some("notion")),
            ("/Users/x/Library/Caches/notion.id.ShipIt", Some("notion")),
            (
                "/Users/x/Library/Caches/com.google.antigravity",
                Some("antigravity"),
            ),
            ("/Users/x/Library/Caches/com.apple.Safari", Some("Safari")),
            ("/Users/x/Library/Caches/termius-updater", None),
            ("/Users/x/Library/Caches/go-build", None),
            ("/Users/x/Library/Logs", None),
        ];
        for (path, want) in cases {
            assert_eq!(
                owning_app(Path::new(path), &procs).as_deref(),
                want,
                "{path}"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn owning_app_rejects_generic_word_as_candidate() {
        // 目录名剥壳后恰好只剩 "helper"：即便真有一个 helper.app 在跑，
        // 通用词也不该被当成候选词去撞。
        let procs = vec!["/applications/helper.app/contents/macos/helper".to_string()];
        assert_eq!(
            owning_app(Path::new("/Users/x/Library/Caches/helper"), &procs),
            None
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn owning_app_rejects_short_candidate() {
        // "id" 只有 2 个字符，低于下限，不该被当成候选词。
        let procs = vec!["/applications/id.app/contents/macos/id".to_string()];
        assert_eq!(
            owning_app(Path::new("/Users/x/Library/Caches/id"), &procs),
            None
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parse_ps_line_handles_padded_columns() {
        let rec = parse_ps_line("    1     0 /sbin/launchd").unwrap();
        assert_eq!(rec.pid, 1);
        assert_eq!(rec.ppid, 0);
        assert_eq!(rec.comm_lower, "/sbin/launchd");

        // comm 本身带空格也要原样保留，只在 pid/ppid 后各切一次
        let rec = parse_ps_line("  123   45 /Applications/Google Chrome.app/foo").unwrap();
        assert_eq!(rec.pid, 123);
        assert_eq!(rec.ppid, 45);
        assert_eq!(rec.comm_lower, "/applications/google chrome.app/foo");

        assert!(parse_ps_line("").is_none());
        assert!(parse_ps_line("not-a-pid stuff").is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn self_and_descendant_pids_walks_bfs() {
        let self_pid = 1000u32;
        let procs = vec![
            ProcRecord {
                pid: 1001,
                ppid: self_pid,
                comm_lower: "child".into(),
            },
            ProcRecord {
                pid: 1002,
                ppid: 1001,
                comm_lower: "grandchild".into(),
            },
            ProcRecord {
                pid: 9999,
                ppid: 1,
                comm_lower: "unrelated".into(),
            },
        ];
        let set = self_and_descendant_pids(self_pid, &procs);
        assert!(set.contains(&self_pid));
        assert!(set.contains(&1001));
        assert!(set.contains(&1002));
        assert!(!set.contains(&9999));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn looks_complete_detects_truncation() {
        assert!(looks_complete(b""));
        assert!(looks_complete(b"p1\0n/a\0\n"));
        assert!(looks_complete(b"p1\0n/a\0")); // 没有末尾换行也算完整
        assert!(!looks_complete(b"p1\0n/a")); // 半截字段，没有收尾 NUL
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ps_output_is_usable_rejects_failure_signals() {
        assert!(!ps_output_is_usable(false, b"1 0 /sbin/launchd\n")); // 非零退出码
        assert!(!ps_output_is_usable(true, b"")); // 空输出：正常机器不可能一个进程都没有
        assert!(!ps_output_is_usable(true, b"1 0 /sbin/launchd")); // 没有换行收尾，像是截断
        assert!(ps_output_is_usable(true, b"1 0 /sbin/launchd\n"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mark_all_unknown_sets_unknown_without_clobbering_existing_fields() {
        // 证明 ps 失败会让 detect_macos 整轮变成 unknown：detect_macos
        // 在 `ps_snapshot()` 返回 `None` 时就是直接调用这个函数再返回，
        // 这里验证它的效果——全部目标都被标记 unknown，且不会抹掉调用方
        // 已经写进去的 app/open 字段（对应 lsof 全表失败但 owning_app 已
        // 经成功识别出应用的那条路径）。
        let targets = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        let mut result: HashMap<PathBuf, Busy> = HashMap::new();
        result.entry(PathBuf::from("/a")).or_default().app = Some("Edge".into());

        mark_all_unknown(&mut result, &targets);

        assert!(result[&PathBuf::from("/a")].unknown);
        assert_eq!(result[&PathBuf::from("/a")].app.as_deref(), Some("Edge"));
        assert!(result[&PathBuf::from("/b")].unknown);
    }

    #[test]
    fn badge_prefers_app_over_open() {
        let b = Busy {
            app: Some("Edge".into()),
            open: true,
            unknown: false,
        };
        let (text, app_level) = b.badge().unwrap();
        assert!(text.get(crate::core::i18n::Language::Zh).contains("Edge"));
        assert!(app_level);

        let b = Busy {
            app: None,
            open: true,
            unknown: false,
        };
        let (_, app_level) = b.badge().unwrap();
        assert!(!app_level);

        assert!(Busy::default().badge().is_none());
    }

    #[test]
    fn badge_shows_unknown_when_detection_failed() {
        let b = Busy {
            app: None,
            open: false,
            unknown: true,
        };
        let (text, app_level) = b.badge().unwrap();
        assert!(!text.get(crate::core::i18n::Language::Zh).is_empty());
        assert!(!app_level);
    }

    #[test]
    fn is_empty_accounts_for_unknown() {
        assert!(Busy::default().is_empty());
        let b = Busy {
            app: None,
            open: false,
            unknown: true,
        };
        assert!(!b.is_empty());
    }

    #[test]
    fn apply_busy_downgrades_recommended() {
        use crate::core::categories::CategoryId;
        use crate::core::i18n::Text;
        use crate::core::scanner::{CategorySummary, ScanItem};
        use std::path::PathBuf;

        let item = |p: &str| ScanItem {
            path: PathBuf::from(p),
            label: Text::same("x"),
            size: 1,
            file_count: 0,
            category: CategoryId::UserTemp,
            last_modified: 0,
            recommended: true,
            busy: None,
            identity: None,
        };
        let mut cats = vec![CategorySummary {
            category: CategoryId::UserTemp,
            total_size: 2,
            items: vec![item("/a"), item("/b")],
        partial: false,
        }];
        let mut busy: HashMap<PathBuf, Busy> = HashMap::new();
        busy.insert(
            PathBuf::from("/a"),
            Busy {
                app: Some("A".into()),
                open: false,
                unknown: false,
            },
        );
        // 空的 Busy 不算占用，不该碰条目
        busy.insert(PathBuf::from("/c"), Busy::default());

        assert_eq!(apply_busy(&mut cats, &busy), 1);
        assert!(cats[0].items[0].busy.is_some());
        assert!(!cats[0].items[0].recommended);
        assert!(cats[0].items[1].busy.is_none());
        assert!(cats[0].items[1].recommended);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn spot_check_fallback_clears_missing_paths() {
        let paths = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        let result = spot_check(&paths);
        assert_eq!(result.get(&PathBuf::from("/a")), Some(&SpotCheck::Clear));
        assert_eq!(result.get(&PathBuf::from("/b")), Some(&SpotCheck::Clear));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn spot_check_fallback_marks_live_database_file_busy_not_the_parent_dir() {
        let base = std::env::temp_dir().join("qc_spot_fallback_live_db");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let db = base.join("cache.db");
        std::fs::write(&db, b"x").unwrap();
        std::fs::write(base.join("cache.db-wal"), b"x").unwrap();

        let result = spot_check(&[base.clone(), db.clone()]);
        assert_eq!(
            result.get(&base),
            Some(&SpotCheck::Clear),
            "目录顶层有 .db 不能把整棵缓存根判成占用"
        );
        assert_eq!(result.get(&db), Some(&SpotCheck::Busy));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn spot_check_clears_nonexistent_paths_without_calling_lsof() {
        // 不存在的路径不该被送进 lsof 参数列表（会拖累整批报错），直接
        // 判 Clear。用一个几乎不可能存在的路径验证。
        let ghost = PathBuf::from("/tmp/qc_inuse_spot_ghost_does_not_exist_xyz");
        let result = spot_check(std::slice::from_ref(&ghost));
        assert_eq!(result.get(&ghost), Some(&SpotCheck::Clear));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn spot_check_uses_recursive_lsof_for_directories() {
        let base = std::env::temp_dir().join("qc_spot_check_args");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let file = base.join("one.bin");
        std::fs::write(&file, b"x").unwrap();

        let args = spot_check_args(&[base.clone(), file.clone()]);
        assert_eq!(
            args,
            vec![
                OsString::from("-F0n"),
                OsString::from("+D"),
                base.as_os_str().to_owned(),
                file.as_os_str().to_owned(),
            ]
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn recursive_directories_do_not_share_a_spot_check_batch() {
        let base = std::env::temp_dir().join("qc_spot_check_batches");
        let first = base.join("first");
        let second = base.join("second");
        let file = base.join("one.bin");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(&file, b"x").unwrap();

        let batches = spot_check_batches(vec![file.clone(), first.clone(), second.clone()]);

        assert_eq!(batches, vec![vec![file], vec![first], vec![second]]);
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn recursive_lsof_descendant_marks_the_target_busy() {
        let raw = PathBuf::from("/tmp/qc-cache");
        let canonical = PathBuf::from("/private/tmp/qc-cache");
        let mut want = HashMap::new();
        want.insert(canonical.clone(), raw.clone());
        let mut result = HashMap::from([(raw.clone(), SpotCheck::Clear)]);

        mark_spot_open_paths(
            &mut result,
            &want,
            [canonical.join("nested/open.bin")],
        );

        assert_eq!(result.get(&raw), Some(&SpotCheck::Busy));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn signaled_lsof_is_not_a_usable_empty_result() {
        let signaled = LsofRun {
            stdout: Vec::new(),
            stderr: Vec::new(),
            exit_code: None,
            ok: false,
        };
        assert!(!spot_lsof_result_is_usable(&signaled));

        let no_matches = LsofRun {
            stdout: Vec::new(),
            stderr: Vec::new(),
            exit_code: Some(1),
            ok: false,
        };
        assert!(spot_lsof_result_is_usable(&no_matches));

        // +D 即使命中也可能返回 1；完整输出仍应被解析，而不是丢掉命中。
        let recursive_match = LsofRun {
            stdout: b"p123\0n/private/tmp/cache/open.bin\0\n".to_vec(),
            stderr: Vec::new(),
            exit_code: Some(1),
            ok: false,
        };
        assert!(spot_lsof_result_is_usable(&recursive_match));
    }
}
