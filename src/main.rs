//! QuickCleaner 极速磁盘分析与清理工具
//! 应用程序主入口

use gpui::{
    actions, px, size, App, AppContext, Application, Bounds, KeyBinding, WindowBounds,
    WindowOptions,
};
use quick_cleaner::ui::Root;

actions!(quick_cleaner, [Quit]);

fn main() {
    #[cfg(windows)]
    {
        quick_cleaner::platform::windows::init_user_context();
        if !std::env::args().any(|a| a == "--no-elevate") {
            quick_cleaner::platform::relaunch_as_admin_if_needed();
        }
    }

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
