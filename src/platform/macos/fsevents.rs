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

/// FSEvents 回放结果。
#[derive(Debug)]
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
    /// 整个卷的水位不再可信，必须全量重建（`EventIdsWrapped` / `RootChanged`）。
    pub requires_full_scan: bool,
    /// 触发 `requires_full_scan` 的 flag 名称，用于日志诊断。
    pub full_scan_reason: Option<&'static str>,
    /// 过滤掉的自家缓存目录事件数。
    pub filtered_cache_events: usize,
    /// 收到的原始事件总数（过滤前）。
    pub raw_event_count: usize,
}

struct Collector {
    events: Vec<(PathBuf, FSEventStreamEventFlags)>,
    history_done: bool,
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
    _event_ids: *const FSEventStreamEventId,
) {
    if info.is_null() || event_paths.is_null() || event_flags.is_null() {
        return;
    }

    // 未启用 UseCFTypes 时，Apple API 将 eventPaths 作为 char** 传入。
    let collector = unsafe { &mut *(info as *mut Collector) };
    let paths = event_paths as *const *const c_char;
    for i in 0..count {
        let flags = unsafe { *event_flags.add(i) };
        let path_ptr = unsafe { *paths.add(i) };
        if !path_ptr.is_null() {
            let path = unsafe { CStr::from_ptr(path_ptr) };
            if let Ok(path) = path.to_str() {
                collector.events.push((PathBuf::from(path), flags));
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
        history_done: false,
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
    // 不能无限等待，因此设置一个有限上限；超时不代表数据正确，直接要求全扫。
    let deadline = Instant::now() + Duration::from_secs(30);
    while !collector.history_done && Instant::now() < deadline {
        let result = unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, 1) };
        // kCFRunLoopRunFinished / kCFRunLoopRunStopped 都意味着本轮没有更多源。
        if result == 1 && since == fsevent_sys::kFSEventStreamEventIdSinceNow {
            break;
        }
    }

    unsafe {
        FSEventStreamFlushSync(stream);
    }
    let latest = unsafe { FSEventStreamGetLatestEventId(stream) };
    unsafe {
        FSEventStreamStop(stream);
        FSEventStreamInvalidate(stream);
        FSEventStreamRelease(stream);
    }

    let history_done = collector.history_done;
    let raw_event_count = collector.events.len();

    if since != fsevent_sys::kFSEventStreamEventIdSinceNow && !history_done {
        crate::log!(
            "FSEvents: 历史回放超时（30s 未收到 HistoryDone），since={}，原始事件 {}",
            since,
            raw_event_count
        );
        return None;
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
        let subtree_reason = if flags & kFSEventStreamEventFlagMustScanSubDirs != 0 {
            Some("MustScanSubDirs")
        } else if flags & kFSEventStreamEventFlagUserDropped != 0 {
            Some("UserDropped")
        } else if flags & kFSEventStreamEventFlagKernelDropped != 0 {
            Some("KernelDropped")
        } else {
            None
        };
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
        "FSEvents 回放完成：since={} → latest={}，原始 {} 事件，过滤缓存 {}，有效 {} 路径，子树重扫 {}，full_scan={}({:?})，耗时 {:?}",
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
    })
}

/// 获取当前系统 FSEvents 水位，用于在全量扫描完成后建立一致的检查点。
pub fn current_event_id() -> u64 {
    unsafe { fsevent_sys::FSEventsGetCurrentEventId() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_event_id_is_nonzero_on_macos() {
        assert!(current_event_id() > 0);
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
