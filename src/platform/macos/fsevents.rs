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

/// FSEvents 回放结果。
#[derive(Debug)]
pub struct Changes {
    pub paths: Vec<PathBuf>,
    pub last_event_id: u64,
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
    let mut requires_full_scan = false;
    let mut full_scan_reason: Option<&'static str> = None;
    let mut filtered_cache_events = 0usize;
    for (path, flags) in collector.events {
        if cache_dir.as_ref().is_some_and(|dir| path.starts_with(dir)) {
            filtered_cache_events += 1;
            continue;
        }
        // 检测哪个 flag 触发了全量扫描
        if flags & kFSEventStreamEventFlagMustScanSubDirs != 0 {
            requires_full_scan = true;
            if full_scan_reason.is_none() {
                full_scan_reason = Some("MustScanSubDirs");
            }
        }
        if flags & kFSEventStreamEventFlagUserDropped != 0 {
            requires_full_scan = true;
            if full_scan_reason.is_none() {
                full_scan_reason = Some("UserDropped");
            }
        }
        if flags & kFSEventStreamEventFlagKernelDropped != 0 {
            requires_full_scan = true;
            if full_scan_reason.is_none() {
                full_scan_reason = Some("KernelDropped");
            }
        }
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

    crate::log!(
        "FSEvents 回放完成：since={} → latest={}，原始 {} 事件，过滤缓存 {}，有效 {} 路径，full_scan={}({:?})，耗时 {:?}",
        since,
        latest,
        raw_event_count,
        filtered_cache_events,
        paths.len(),
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

    Some(Changes {
        paths,
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
