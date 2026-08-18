//! QuickCleaner 极速磁盘分析与清理工具
//! 应用程序主入口

// GUI 程序不该带控制台窗口。Rust 默认链接成 console 子系统，
// Windows 会额外开一个黑框，所以 debug/release 一律切到 windows 子系统。
// 本二进制运行时没有 stdout 输出（调试用的 println! 全在 #[test] 里，
// 走的是 cargo test 的独立测试二进制，不受影响）。
#![windows_subsystem = "windows"]

use gpui::{actions, px, size, App, AppContext, Application, Bounds, WindowBounds, WindowOptions};
use quick_cleaner::ui::Root;

actions!(quick_cleaner, [Quit]);

fn main() {
    #[cfg(windows)]
    {
        // 顺序有讲究：日志路径要锚定真实前台用户，得等 init_user_context 之后；
        // 又要在自提权之前，否则提权前那次启动出的问题一行都留不下来。
        quick_cleaner::platform::windows::init_user_context();
        quick_cleaner::core::log::init();
        // 越早越好：这之后的任何 panic 才有现场可查
        quick_cleaner::core::log::install_panic_hook();
        if !std::env::args().any(|a| a == "--no-elevate") {
            quick_cleaner::platform::relaunch_as_admin_if_needed();
        }
    }
    #[cfg(not(windows))]
    {
        quick_cleaner::core::log::init();
        quick_cleaner::core::log::install_panic_hook();
    }

    Application::new().run(move |cx: &mut App| {
        #[cfg(target_os = "macos")]
        quick_cleaner::platform::macos::set_dock_icon();

        // macOS 上退出走系统菜单的 Cmd-Q，不自己绑；`KeyBinding` 也就只有
        // 非 macOS 分支用得到，导入要跟着一起门禁，否则 macOS 上是个未使用导入。
        #[cfg(not(target_os = "macos"))]
        cx.bind_keys([gpui::KeyBinding::new("ctrl-q", Quit, None)]);

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
                    // macOS 上若需要显示完全磁盘访问权限引导，先不自动扫描——
                    // 否则扫描会立刻访问 ~/Library/Caches、Safari 缓存等受保护
                    // 目录，在用户还没决定是否授权前就弹出一堆 TCC 权限窗口。
                    // 引导弹窗里的「检查权限」或「稍后」会负责触发首次扫描。
                    #[cfg(target_os = "macos")]
                    if !root.show_fda_onboarding {
                        root.start_scan(cx);
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        root.start_scan(cx);
                    }
                    // 软件列表扫描主要读 /Applications，不触发 TCC，可安全启动
                    root.start_apps_scan(cx);
                    root
                })
            },
        )
        .unwrap();

        cx.activate(true);
    });
}
