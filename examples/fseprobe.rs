//! 量一次 FSEvents 历史回放的实际代价，不是产品入口。
//!
//! ```text
//! cargo run --example fseprobe --features fseprobe -- / 576966680
//! cargo run --example fseprobe --features fseprobe -- / now
//! ```
//!
//! 存在的理由：`changes_since` 的耗时到底花在哪里，从日志里推不出来。
//! 实测过一次 `since=0`、只匹配到 3 条事件的回放也要 35s——说明大头是
//! fseventsd 翻自己的历史日志，不是我们收事件。而「卷根需整棵重扫」的
//! 早退能省多少，取决于那条根事件在流里出现的位置，只能实测。
//!
//! 未加 `--features fseprobe` 时 Cargo 不会编译它，默认构建和
//! `cargo bundle` 都不会带上。

#![cfg(target_os = "macos")]

use quick_cleaner::platform::macos::fsevents;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = args.get(1).map(String::as_str).unwrap_or("/");
    let since_arg = args.get(2).map(String::as_str).unwrap_or("now");
    let since = match since_arg {
        "now" => fsevents::current_event_id(),
        s => match s.parse::<u64>() {
            Ok(v) => v,
            Err(_) => {
                eprintln!("水位要么是 now，要么是一个整数事件 ID，收到：{s}");
                std::process::exit(2);
            }
        },
    };

    println!(
        "回放 {root}，since={since}（当前水位 {}）",
        fsevents::current_event_id()
    );
    let t0 = std::time::Instant::now();
    let changes = fsevents::changes_since(Path::new(root), since);
    let elapsed = t0.elapsed();

    match changes {
        Some(c) => {
            println!("耗时          {elapsed:?}");
            println!("原始事件      {}", c.raw_event_count);
            println!("有效路径      {}", c.paths.len());
            println!("子树需重扫    {}", c.must_rescan.len());
            println!("过滤缓存事件  {}", c.filtered_cache_events);
            println!("水位          {}", c.last_event_id);
            println!(
                "整盘重建      {}（原因 {:?}）",
                c.requires_full_scan, c.full_scan_reason
            );
            if c.full_scan_reason == Some("RootMustScanSubDirs") {
                println!(
                    "\n>>> 早退生效：收了 {} 条就停了。和日志里同一水位「不早退」的\n\
                     >>> 原始事件数/耗时相比，差值就是这次改动省下的部分。",
                    c.raw_event_count
                );
            }
        }
        None => println!("耗时 {elapsed:?}，回放不可用（None）——调用方会转全量"),
    }
}
