//! macOS FSEvents 历史事件读取。
//!
//! FSEvents 是增量索引的日志，不提供文件大小；收到事件后仍需重新读取受影响的
//! 目录。事件被丢弃、回绕或历史日志不可用时，调用方必须回退完整扫描。

use fsevent_sys::core_foundation::{
    kCFAllocatorDefault, kCFRunLoopDefaultMode, kCFStringEncodingUTF8, CFArrayAppendValue,
    CFArrayCreateMutable, CFRelease, CFRunLoopGetCurrent, CFStringCreateWithCString, CFStringRef,
};
use fsevent_sys::{
    kFSEventStreamCreateFlagFileEvents, kFSEventStreamCreateFlagNoDefer,
    kFSEventStreamEventFlagEventIdsWrapped, kFSEventStreamEventFlagHistoryDone,
    kFSEventStreamEventFlagKernelDropped, kFSEventStreamEventFlagMustScanSubDirs,
    kFSEventStreamEventFlagRootChanged, kFSEventStreamEventFlagUserDropped, FSEventStreamContext,
    FSEventStreamCreate, FSEventStreamEventFlags, FSEventStreamEventId, FSEventStreamFlushSync,
    FSEventStreamGetLatestEventId, FSEventStreamInvalidate, FSEventStreamRelease,
    FSEventStreamScheduleWithRunLoop, FSEventStreamStart, FSEventStreamStop,
};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_int, c_void};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 单轮历史回放的时间预算（秒）。
///
/// 实测旧水位差 2000 万+ 事件 ID 时，回放密度约 2.5 万 ID/秒，拉到当前要
/// 15 分钟量级；而卷根 `MustScanSubDirs` 这类「历史已不可信」的信号会在
/// 中途冒头。预算放到 120s 只会让人干等——3–5s 内看清能不能推进，不能
/// 推进就让调用方决定重建/续传。超时续传见 [`Changes::incomplete`]。
const REPLAY_BUDGET_SECS: u64 = 5;

/// 把数据卷镜像路径折叠回正规路径。
///
/// macOS 的 `/` 是合成根，`/Users` 等顶层目录经 firmlink 指向数据卷；
/// 同一批文件因此有 `/Users/…` 和 `/System/Volumes/Data/Users/…` 两个
/// 路径形态，FSEvents 两种都可能报。统一折叠成前者，增量刷新才不会
/// 在索引里找到（或建立）镜像侧的节点——walk 已经不再收录镜像子树，
/// 事件再以镜像形态进来就会凭空造出第二份。
pub(crate) fn canonicalize_event_path(path: &Path) -> PathBuf {
    const MIRROR: &str = "/System/Volumes/Data";
    let s = path.to_string_lossy();
    if s == MIRROR {
        PathBuf::from("/")
    } else if let Some(rest) = s.strip_prefix(&format!("{MIRROR}/")) {
        PathBuf::from("/").join(rest)
    } else {
        path.to_path_buf()
    }
}

/// 事件是否要求重扫它携带的那个路径。
///
/// 三个 flag 语义相同：「这个路径下面的明细我丢了，你自己重扫」，作用域是
/// 事件携带的路径，不是整个卷。按 Apple 文档 `UserDropped` / `KernelDropped`
/// 总是伴随 `MustScanSubDirs` 出现；返回哪个名字只影响日志措辞。
fn subtree_rescan_reason(flags: FSEventStreamEventFlags) -> Option<&'static str> {
    if flags & kFSEventStreamEventFlagMustScanSubDirs != 0 {
        Some("MustScanSubDirs")
    } else if flags & kFSEventStreamEventFlagUserDropped != 0 {
        Some("UserDropped")
    } else if flags & kFSEventStreamEventFlagKernelDropped != 0 {
        Some("KernelDropped")
    } else {
        None
    }
}

/// 这条事件是不是在说「整个卷都要重扫」：它要求重扫的路径正好是被监听的根。
///
/// 两侧都先折叠镜像形态再比——FSEvents 可能把卷根报成 `/System/Volumes/Data`，
/// 而调用方比对 `must_rescan` 时用的是折叠后的形态（`volume.mount_point()`）。
/// 两边形态不一致就会漏判，回放白跑一趟。
fn is_root_rescan(flags: FSEventStreamEventFlags, event_path: &str, canonical_root: &Path) -> bool {
    subtree_rescan_reason(flags).is_some()
        && canonicalize_event_path(Path::new(event_path)) == canonical_root
}

/// FSEvents 回放结果。
#[derive(Debug, Default)]
pub struct Changes {
    pub paths: Vec<PathBuf>,
    /// 必须整棵重扫的子树。
    ///
    /// FSEvents 在某个目录下事件太多时会把它们合并成一条，给这条事件打上
    /// `MustScanSubDirs`（`UserDropped` / `KernelDropped` 同理，二者按 Apple
    /// 文档总是伴随前者出现）。这几个 flag 的语义都是「**这一个路径**下面
    /// 的明细我丢了，你自己重扫」，作用域是事件携带的那个路径，不是整个卷。
    /// 早先的实现把它们折叠成一个全局 `requires_full_scan` 并丢掉路径，一条
    /// `/Library/Keychains` 的合并事件就能换来一次 60 秒以上的整盘重建；
    /// 日志里 4 次全量重建全部来自这里。现在改成把路径收进来，交给增量刷新
    /// 当成普通变更根重扫。
    pub must_rescan: Vec<PathBuf>,
    pub last_event_id: u64,
    /// 整个卷的水位不再可信，必须全量重建（`EventIdsWrapped` / `RootChanged`，
    /// 以及「被监听的根自己需要整棵重扫」——那等价于整盘，见
    /// [`changes_since`] 里对 `RootMustScanSubDirs` 的处理）。
    pub requires_full_scan: bool,
    /// 触发 `requires_full_scan` 的 flag 名称，用于日志诊断。
    pub full_scan_reason: Option<&'static str>,
    /// 过滤掉的自家缓存目录事件数。
    pub filtered_cache_events: usize,
    /// 收到的原始事件总数（过滤前）。
    ///
    /// 注意：`requires_full_scan` 因「根自己要重扫」置位时，回放是被提前
    /// 截断的，这里是**截断处的部分计数**，不是本次历史的事件总量。
    pub raw_event_count: usize,
    /// 历史回放未等到 `HistoryDone`，本批只是前缀。
    ///
    /// `last_event_id` 此时是**回调里见过的最大事件 ID**，不是整段历史的
    /// 末尾。调用方应把本批路径当普通增量应用，并让下一轮从该水位续传；
    /// 不要因为「不完整」就丢掉整棵树去做整盘重建——那正是超时路径曾经
    /// 付出 57s+ 代价的地方。
    pub incomplete: bool,
}

struct Collector {
    events: Vec<(PathBuf, FSEventStreamEventFlags)>,
    /// 回调里见过的最大事件 ID。超时续传时用它推进水位——只推进到
    /// **已经投递到的位置**，未回放的历史留给下一轮。
    max_event_id: u64,
    history_done: bool,
    /// 被监听的根，**已折叠成正规形态**。用它认出「根自己需要整棵重扫」
    /// 这种等价全量的信号。折叠在建 `Collector` 时做一次：回调可能被调用
    /// 上千次，每次重算就是每次多分配一个 `PathBuf`。
    canonical_root: PathBuf,
    /// 回调已经看到根自己带了重扫 flag。置位后排空循环立刻停下。
    root_must_rescan: bool,
}

// CoreFoundation 中的当前运行循环模式调用。fsevent-sys 4.x 没有导出它，
// 这里只声明公开的 CoreFoundation C API，不依赖私有接口。
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRunLoopRunInMode(
        mode: CFStringRef,
        seconds: c_double,
        return_after_source_handled: u8,
    ) -> c_int;
}

extern "C" fn event_callback(
    _stream: fsevent_sys::FSEventStreamRef,
    info: *mut c_void,
    count: usize,
    event_paths: *mut c_void,
    event_flags: *const FSEventStreamEventFlags,
    event_ids: *const FSEventStreamEventId,
) {
    if info.is_null() || event_paths.is_null() || event_flags.is_null() {
        return;
    }

    // 未启用 UseCFTypes 时，Apple API 将 eventPaths 作为 char** 传入。
    let collector = unsafe { &mut *(info as *mut Collector) };
    let paths = event_paths as *const *const c_char;
    for i in 0..count {
        let flags = unsafe { *event_flags.add(i) };
        if !event_ids.is_null() {
            let id = unsafe { *event_ids.add(i) };
            if id > collector.max_event_id {
                collector.max_event_id = id;
            }
        }
        let path_ptr = unsafe { *paths.add(i) };
        if !path_ptr.is_null() {
            let path = unsafe { CStr::from_ptr(path_ptr) };
            if let Ok(path) = path.to_str() {
                // 根自己要重扫 == 整盘要重扫，增量路径无事可做。这条信号在
                // 回调里就能认出来，剩下的几十万个事件不用再收——收完也是
                // 整包丢掉，实测一次回放 39s 全花在排空上，一个路径都没用。
                //
                // 置位之后连收都不收：本批剩下的事件同样是要整包丢的，
                // 每条还要白分配一个 `PathBuf`。
                if !collector.root_must_rescan
                    && is_root_rescan(flags, path, &collector.canonical_root)
                {
                    collector.root_must_rescan = true;
                }
                if !collector.root_must_rescan {
                    collector.events.push((PathBuf::from(path), flags));
                }
            }
        }

        if flags & kFSEventStreamEventFlagHistoryDone != 0 {
            collector.history_done = true;
        }
    }
}

/// 回放 `root` 从 `since` 之后发生的事件。
///
/// 返回 `None` 表示无法创建或启动事件流，调用方必须完整重扫。
pub fn changes_since(root: &Path, since: u64) -> Option<Changes> {
    let t0 = Instant::now();
    let path = CString::new(root.to_string_lossy().as_bytes()).ok()?;
    let cf_path = unsafe {
        CFStringCreateWithCString(kCFAllocatorDefault, path.as_ptr(), kCFStringEncodingUTF8)
    };
    if cf_path.is_null() {
        crate::log!("FSEvents: CFString 创建失败");
        return None;
    }

    let paths = unsafe {
        CFArrayCreateMutable(
            kCFAllocatorDefault,
            1,
            &fsevent_sys::core_foundation::kCFTypeArrayCallBacks,
        )
    };
    if paths.is_null() {
        unsafe { CFRelease(cf_path) };
        crate::log!("FSEvents: CFArray 创建失败");
        return None;
    }
    unsafe { CFArrayAppendValue(paths, cf_path) };

    let mut collector = Collector {
        events: Vec::new(),
        max_event_id: 0,
        history_done: false,
        // 根可能在事件里以镜像形态出现（`/System/Volumes/Data`），两边都归
        // 一后再比，和调用方最终比对 `must_rescan` 用的形态保持一致。
        canonical_root: canonicalize_event_path(root),
        root_must_rescan: false,
    };
    let context = FSEventStreamContext {
        version: 0,
        info: &mut collector as *mut Collector as *mut c_void,
        retain: None,
        release: None,
        copy_description: None,
    };
    let flags = kFSEventStreamCreateFlagFileEvents | kFSEventStreamCreateFlagNoDefer;
    let stream = unsafe {
        FSEventStreamCreate(
            kCFAllocatorDefault,
            event_callback,
            &context,
            paths,
            since,
            0.0,
            flags,
        )
    };
    unsafe {
        CFRelease(cf_path);
        CFRelease(paths);
    }
    if stream.is_null() {
        crate::log!("FSEvents: FSEventStreamCreate 返回 null");
        return None;
    }

    let run_loop = unsafe { CFRunLoopGetCurrent() };
    unsafe {
        FSEventStreamScheduleWithRunLoop(stream, run_loop, kCFRunLoopDefaultMode);
    }
    let started = unsafe { FSEventStreamStart(stream) } != 0;
    if !started {
        unsafe {
            FSEventStreamInvalidate(stream);
            FSEventStreamRelease(stream);
        }
        crate::log!("FSEvents: FSEventStreamStart 失败");
        return None;
    }

    // 历史事件需要运行当前线程的 run loop 才会进入 callback。没有事件时
    // 不能无限等待，因此设置一个有限上限。超时**不再**直接判死刑：只要
    // 回调已经投递过事件（水位可推进），就按本批前缀续传，下一轮接着回放。
    let deadline = Instant::now() + Duration::from_secs(REPLAY_BUDGET_SECS);
    while !collector.history_done && !collector.root_must_rescan && Instant::now() < deadline {
        let result = unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, 1) };
        // kCFRunLoopRunFinished / kCFRunLoopRunStopped 都意味着本轮没有更多源。
        if result == 1 && since == fsevent_sys::kFSEventStreamEventIdSinceNow {
            break;
        }
    }

    let history_done = collector.history_done;
    let root_must_rescan = collector.root_must_rescan;
    let raw_event_count = collector.events.len();
    let timed_out = since != fsevent_sys::kFSEventStreamEventIdSinceNow && !history_done;
    // 只推进到「回调已经见过」的位置。比 `since` 还旧的 ID（或 SinceNow）
    // 都不能当进度。
    let max_event_id = collector.max_event_id;
    let progressed = max_event_id > since;

    // 根重扫走 `requires_full_scan`，水位一步不推。超时但有进度时填
    // `max_event_id`（已投递到的位置）——再往后是还没看过的历史，留给
    // 下一轮续传；绝不能填 `GetLatestEventId`，那会把未回放段静默跳过。
    //
    // 完成路径才 `FlushSync` + `GetLatestEventId`。`FlushSync` 不是免费的：
    // 它会把 pending 事件再逼出一批。实测一次间隔 2.9 小时的回放，超时后
    // 还在 `FlushSync` 里堵了 12.6s。超时路径我们已经打算续传，更不该堵。
    let latest = if root_must_rescan {
        since
    } else if timed_out {
        if progressed {
            max_event_id
        } else {
            since
        }
    } else {
        unsafe {
            FSEventStreamFlushSync(stream);
        }
        unsafe { FSEventStreamGetLatestEventId(stream) }
    };

    unsafe {
        FSEventStreamStop(stream);
        FSEventStreamInvalidate(stream);
        FSEventStreamRelease(stream);
    }

    // 根自己要重扫 == 其下一切可能已变。**不再** `requires_full_scan` 整盘
    // 重建：那条路径在冷启动（水位差极大、根事件埋在几十万条中间）上，先
    // 白放一遍历史再 57s 重建，两头挨打。
    //
    // 改成把**直接子目录**收进 `must_rescan`，水位钉在 `current_event_id`
    // ——接下来按当前文件系统状态重扫这些子树，未回放历史与这次重扫重叠，
    // 再回放没有增量信息。成本估算（`refresh_macos_index`）若判定覆盖面
    // 过大仍会回退全量，那是一次干净的 `full_macos_scan`，检查点也在当前。
    //
    // 能省多少取决于第一条根重扫事件在流里的位置；`REPLAY_BUDGET_SECS`
    // 把最坏白等压到秒级。`incomplete` 在这条路径上是 false：要做的事
    // 已经明确（重扫子树），不是「回放前缀、下轮续传」。
    if root_must_rescan {
        let children = direct_children(root);
        if children.is_empty() {
            crate::log!(
                "FSEvents: 卷根 {} 需整棵重扫且读不到子项，放弃回放（已收 {} 事件）转全量",
                root.display(),
                raw_event_count
            );
            return Some(Changes {
                paths: Vec::new(),
                must_rescan: Vec::new(),
                last_event_id: latest,
                requires_full_scan: true,
                full_scan_reason: Some("RootMustScanSubDirs"),
                filtered_cache_events: 0,
                raw_event_count,
                incomplete: false,
            });
        }
        let checkpoint = current_event_id();
        crate::log!(
            "FSEvents: 卷根 {} 需整棵重扫（已收 {} 事件）→ 拆成 {} 个直接子目录增量重扫，水位钉 {}",
            root.display(),
            raw_event_count,
            children.len(),
            checkpoint
        );
        return Some(Changes {
            paths: Vec::new(),
            must_rescan: children,
            last_event_id: checkpoint,
            requires_full_scan: false,
            full_scan_reason: None,
            filtered_cache_events: 0,
            raw_event_count,
            incomplete: false,
        });
    }

    if timed_out && !progressed {
        crate::log!(
            "FSEvents: 历史回放超时且无进度（{}s 未收到 HistoryDone），since={}，原始事件 {}",
            REPLAY_BUDGET_SECS,
            since,
            raw_event_count
        );
        return None;
    }
    if timed_out {
        crate::log!(
            "FSEvents: 历史回放超时（{}s 未收到 HistoryDone），since={} → 已推进到 {}，原始事件 {}，下轮续传",
            REPLAY_BUDGET_SECS,
            since,
            latest,
            raw_event_count
        );
    }

    // 索引文件保存在用户目录内，保存索引本身也会产生 FSEvents；这些事件
    // 不代表用户文件发生变化，否则每次启动都会被自己的写盘操作触发重扫。
    let cache_dir = super::cache::cache_dir_path();
    let mut paths = Vec::new();
    let mut must_rescan: Vec<PathBuf> = Vec::new();
    let mut requires_full_scan = false;
    let mut full_scan_reason: Option<&'static str> = None;
    let mut filtered_cache_events = 0usize;
    for (path, flags) in collector.events {
        if cache_dir.as_ref().is_some_and(|dir| path.starts_with(dir)) {
            filtered_cache_events += 1;
            continue;
        }
        // 作用域是**这一个路径**的 flag：只要求重扫该子树。
        let subtree_reason = subtree_rescan_reason(flags);
        if let Some(reason) = subtree_reason {
            crate::log!("  FSEvents 子树需重扫（{}）: {}", reason, path.display());
            must_rescan.push(canonicalize_event_path(&path));
        }

        // 作用域是整个卷的 flag：水位本身不再可信，只能全量重建。
        // EventIdsWrapped 表示事件 ID 空间回绕，任何已存水位都失去意义；
        // RootChanged 表示被监听的根自己被移动/删除/替换，树的锚点没了。
        if flags & kFSEventStreamEventFlagEventIdsWrapped != 0 {
            requires_full_scan = true;
            if full_scan_reason.is_none() {
                full_scan_reason = Some("EventIdsWrapped");
            }
        }
        if flags & kFSEventStreamEventFlagRootChanged != 0 {
            requires_full_scan = true;
            if full_scan_reason.is_none() {
                full_scan_reason = Some("RootChanged");
            }
        }
        paths.push(path);
    }

    must_rescan.sort();
    must_rescan.dedup();

    crate::log!(
        "FSEvents 回放{}：since={} → latest={}，原始 {} 事件，过滤缓存 {}，有效 {} 路径，子树重扫 {}，full_scan={}({:?})，耗时 {:?}",
        if timed_out { "超时续传" } else { "完成" },
        since,
        latest,
        raw_event_count,
        filtered_cache_events,
        paths.len(),
        must_rescan.len(),
        requires_full_scan,
        full_scan_reason,
        t0.elapsed()
    );

    // 打印前 10 条变更路径帮助定位热点区域
    if !paths.is_empty() && paths.len() <= 50 {
        for p in &paths {
            crate::log!("  FSEvents 变更: {}", p.display());
        }
    } else if paths.len() > 50 {
        for p in paths.iter().take(10) {
            crate::log!("  FSEvents 变更: {}", p.display());
        }
        crate::log!("  ... 共 {} 条变更路径（仅显示前 10 条）", paths.len());
    }

    // 镜像路径折叠 + 去重：同一变更可能以两种形态各报一次。
    let mut paths: Vec<PathBuf> = paths.iter().map(|p| canonicalize_event_path(p)).collect();
    paths.sort();
    paths.dedup();

    Some(Changes {
        paths,
        must_rescan,
        last_event_id: latest,
        requires_full_scan,
        full_scan_reason,
        filtered_cache_events,
        raw_event_count,
        incomplete: timed_out,
    })
}

/// 获取当前系统 FSEvents 水位，用于在全量扫描完成后建立一致的检查点。
pub fn current_event_id() -> u64 {
    unsafe { fsevent_sys::FSEventsGetCurrentEventId() }
}

/// 列出 `root` 的直接子目录（折叠镜像形态）。卷根 `MustScanSubDirs` 拆
/// 增量重扫时用。读不到就返回空，调用方回退全量。
fn direct_children(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.push(canonicalize_event_path(&path));
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_event_id_is_nonzero_on_macos() {
        assert!(current_event_id() > 0);
    }

    #[test]
    fn rescan_flags_are_recognized_and_others_are_not() {
        assert_eq!(
            subtree_rescan_reason(kFSEventStreamEventFlagMustScanSubDirs),
            Some("MustScanSubDirs")
        );
        // UserDropped / KernelDropped 按 Apple 文档总伴随前者，实测也单独出现过
        assert_eq!(
            subtree_rescan_reason(kFSEventStreamEventFlagUserDropped),
            Some("UserDropped")
        );
        assert_eq!(
            subtree_rescan_reason(kFSEventStreamEventFlagKernelDropped),
            Some("KernelDropped")
        );
        // 普通变更与「历史回放结束」都不是重扫信号
        assert_eq!(subtree_rescan_reason(0), None);
        assert_eq!(
            subtree_rescan_reason(kFSEventStreamEventFlagHistoryDone),
            None
        );
    }

    /// 卷根的两种形态都要认出来，否则回放白跑：FSEvents 可能把根报成
    /// 镜像路径 `/System/Volumes/Data`，而调用方比对用的是折叠后的 `/`。
    #[test]
    fn root_rescan_detected_in_both_path_forms() {
        let root = canonicalize_event_path(Path::new("/"));
        assert!(is_root_rescan(
            kFSEventStreamEventFlagMustScanSubDirs,
            "/",
            &root
        ));
        assert!(is_root_rescan(
            kFSEventStreamEventFlagMustScanSubDirs,
            "/System/Volumes/Data",
            &root
        ));
        // 子目录要重扫只影响那一棵子树，不是整盘
        assert!(!is_root_rescan(
            kFSEventStreamEventFlagMustScanSubDirs,
            "/Users/foo",
            &root
        ));
        // 根上来的普通变更（权限、mtime）不算重扫
        assert!(!is_root_rescan(0, "/", &root));
    }

    #[test]
    #[ignore]
    fn replay_from_current_event_id_returns() {
        let id = current_event_id();
        let result = changes_since(Path::new("/tmp"), id);
        assert!(result.is_some(), "FSEvents 应能从当前水位建立回放流");
    }
}

#[cfg(test)]
mod canonicalize_tests {
    use super::canonicalize_event_path;
    use std::path::{Path, PathBuf};

    #[test]
    fn mirror_paths_fold_to_canonical_form() {
        assert_eq!(
            canonicalize_event_path(Path::new("/System/Volumes/Data/Users/me/Library/Caches/x")),
            PathBuf::from("/Users/me/Library/Caches/x")
        );
        // 镜像根自身折叠为卷根
        assert_eq!(
            canonicalize_event_path(Path::new("/System/Volumes/Data")),
            PathBuf::from("/")
        );
    }

    #[test]
    fn plain_paths_pass_through_untouched() {
        assert_eq!(
            canonicalize_event_path(Path::new("/Users/me/a.txt")),
            PathBuf::from("/Users/me/a.txt")
        );
        // 前缀相似但不是镜像的路径不能误折
        assert_eq!(
            canonicalize_event_path(Path::new("/System/Volumes/Preboot/x")),
            PathBuf::from("/System/Volumes/Preboot/x")
        );
        assert_eq!(
            canonicalize_event_path(Path::new("/System/Volumes/DataBase")),
            PathBuf::from("/System/Volumes/DataBase")
        );
    }
}
