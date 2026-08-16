//! QuickCleaner 极速磁盘分析与清理工具
//! 应用程序主入口

// GUI 程序不该带控制台窗口。Rust 默认链接成 console 子系统，
// Windows 会额外开一个黑框，所以 debug/release 一律切到 windows 子系统。
// 本二进制运行时没有 stdout 输出（调试用的 println! 全在 #[test] 里，
// 走的是 cargo test 的独立测试二进制，不受影响）。
#![windows_subsystem = "windows"]

use gpui::{
    actions, px, size, App, AppContext, Application, Bounds, KeyBinding, WindowBounds,
    WindowOptions,
};
use quick_cleaner::ui::Root;

actions!(quick_cleaner, [Quit]);

fn main() {
    #[cfg(windows)]
    {
        // 顺序有讲究：日志路径要锚定真实前台用户，得等 init_user_context 之后；
        // 又要在自提权之前，否则提权前那次启动出的问题一行都留不下来。
        quick_cleaner::platform::windows::init_user_context();
        quick_cleaner::core::log::init();
        if !std::env::args().any(|a| a == "--no-elevate") {
            quick_cleaner::platform::relaunch_as_admin_if_needed();
        }
    }
    #[cfg(not(windows))]
    quick_cleaner::core::log::init();

    Application::new().run(move |cx: &mut App| {
        #[cfg(not(target_os = "macos"))]
        cx.bind_keys([KeyBinding::new("ctrl-q", Quit, None)]);

        cx.on_action(|_: &Quit, cx| cx.quit());

        let bounds = Bounds::centered(None, size(px(1280.), px(880.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.on_window_should_close(cx, |_, cx| {
                    cx.quit();
                    true
                });

                cx.new(|cx| {
                    let mut root = Root::new(cx);
                    // 两个扫描互相独立：垃圾扫描每次清理后都会重跑，而软件
                    // 列表只在启动、用户主动刷新或卸载完成后重扫。
                    root.start_scan(cx);
                    root.start_apps_scan(cx);
                    root
                })
            },
        )
        .unwrap();

        cx.activate(true);
    });
}
