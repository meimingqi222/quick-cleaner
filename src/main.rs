//! QuickCleaner 极速磁盘分析与清理工具
//! 应用程序主入口

// GUI 程序不该带控制台窗口。Rust 默认链接成 console 子系统，
// Windows 会额外开一个黑框，所以 debug/release 一律切到 windows 子系统。
// 本二进制运行时没有 stdout 输出（调试用的 println! 全在 #[test] 里，
// 走的是 cargo test 的独立测试二进制，不受影响）。
#![windows_subsystem = "windows"]

use gpui::{
    px, size, App, AppContext, Application, Bounds, WindowBounds, WindowHandle, WindowOptions,
};
use quick_cleaner::ui::tray_host::{TrayHost, TrayMenu};
use quick_cleaner::ui::tray_overview::OverviewController;
use quick_cleaner::ui::{desktop, Root};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

fn main() {
    // 风扇特权守护进程的两个非 GUI 入口（详见 platform::macos::fanhelper）：
    //
    //   --fanhelper          launchd 以 root 拉起的常驻守护进程本体，
    //                        在 unix socket 上收「auto / pct <1-100>」两条命令。
    //   --fanhelper-install  安装流程里由 osascript 以 root 跑一次，写 plist 并
    //                        launchctl bootstrap，之后立即退出。
    //
    // 两者都必须在 GUI 初始化之前分流：守护进程不该建窗口，也不该碰 Dock。
    #[cfg(target_os = "macos")]
    {
        let args: Vec<String> = std::env::args().collect();
        if args.len() == 2 {
            match args[1].as_str() {
                "--fanhelper" => {
                    quick_cleaner::platform::macos::fanhelper::run_daemon();
                    return;
                }
                "--fanhelper-install" => {
                    std::process::exit(
                        quick_cleaner::platform::macos::fanhelper::run_install_step(),
                    );
                }
                _ => {}
            }
        }
    }

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

    type OpenMain = Arc<dyn Fn(&mut App) + Send + Sync>;
    let reopen_handler = std::rc::Rc::new(std::cell::RefCell::new(None::<OpenMain>));
    let application = Application::new();
    let reopen = reopen_handler.clone();
    application.on_reopen(move |cx| {
        let handler = reopen.borrow().clone();
        if let Some(open) = handler {
            open(cx);
        }
    });
    application.run(move |cx: &mut App| {
        #[cfg(target_os = "macos")]
        quick_cleaner::platform::macos::set_dock_icon(cx);

        let tray = TrayHost::new(360., 560.);
        let tray_available = Arc::new(AtomicBool::new(false));

        let main_window_holder: Arc<Mutex<Option<WindowHandle<Root>>>> = Arc::new(Mutex::new(None));
        let root_holder = Arc::new(Mutex::new(None::<gpui::Entity<Root>>));
        let on_main_closed: Arc<dyn Fn(&mut App) + Send + Sync> = {
            let holder = main_window_holder.clone();
            let available = tray_available.clone();
            Arc::new(move |cx| {
                *holder.lock().unwrap() = None;
                desktop::main_window_closed(cx, available.load(Ordering::Acquire));
            })
        };
        desktop::bind_actions(cx, {
            let main = main_window_holder.clone();
            let tray = tray.clone();
            let closed = on_main_closed.clone();
            move |cx| {
                let Some(active) = cx.active_window() else {
                    return;
                };
                if tray.close_active(active, cx) {
                    return;
                }
                let main_handle = *main.lock().unwrap();
                if main_handle.is_some_and(|h| gpui::AnyWindowHandle::from(h) == active) {
                    let closed = closed.clone();
                    desktop::close_window(active, cx, move |cx| closed(cx));
                }
            }
        });

        // 打开或聚焦主窗口
        let open_main_window: Arc<dyn Fn(&mut App) + Send + Sync> = {
            let holder = main_window_holder.clone();
            let root_holder = root_holder.clone();
            let on_main_closed = on_main_closed.clone();
            Arc::new(move |cx: &mut App| {
                desktop::show_main_window();
                let mut guard = holder.lock().unwrap();
                if let Some(ref handle) = *guard {
                    cx.activate(true);
                    let _ = handle.update(cx, |_, window, _| {
                        window.activate_window();
                    });
                    return;
                }

                let bounds = Bounds::centered(None, size(px(1280.), px(880.)), cx);
                let handle = cx
                    .open_window(
                        WindowOptions {
                            window_bounds: Some(WindowBounds::Windowed(bounds)),
                            ..Default::default()
                        },
                        |window, cx| {
                            let closed = on_main_closed.clone();
                            desktop::install_close_handler(window, cx, move |cx| closed(cx));

                            let mut saved = root_holder.lock().unwrap();
                            if let Some(root) = saved.as_ref() {
                                return root.clone();
                            }
                            let root = cx.new(Root::new);
                            *saved = Some(root.clone());
                            let startup = root.clone();
                            cx.spawn(async move |cx| {
                                cx.background_executor()
                                    .timer(Duration::from_millis(100))
                                    .await;
                                startup
                                    .update(cx, |root, cx| {
                                        root.start_update_scheduler(cx);
                                    })
                                    .ok();
                            })
                            .detach();
                            root
                        },
                    )
                    .unwrap();

                *guard = Some(handle);
                cx.activate(true);
            })
        };

        let open_main: Arc<dyn Fn(&mut App) + Send + Sync> = {
            let tray = tray.clone();
            let open = open_main_window.clone();
            Arc::new(move |cx| tray.dismiss_then(open.clone(), cx))
        };
        let action_dispatcher: quick_cleaner::ui::tray_overview::OverviewDispatcher = {
            let open = open_main_window.clone();
            let main = main_window_holder.clone();
            Arc::new(move |action, cx: &mut App| {
                use quick_cleaner::ui::tray_overview::OverviewAction;
                match action {
                    OverviewAction::OpenMain => open(cx),
                    OverviewAction::Quit => cx.quit(),
                    OverviewAction::ScanJunk => {
                        open(cx);
                        let handle = *main.lock().unwrap();
                        if let Some(handle) = handle {
                            let _ = handle.update(cx, |root, _, cx| {
                                root.view = quick_cleaner::ui::shell::View::Junk;
                                root.start_scan(cx);
                            });
                        }
                    }
                    OverviewAction::OpenView(view) => {
                        open(cx);
                        let handle = *main.lock().unwrap();
                        if let Some(handle) = handle {
                            let _ = handle.update(cx, |root, _, cx| {
                                root.view = view;
                                match view {
                                    quick_cleaner::ui::shell::View::Junk => root.start_scan(cx),
                                    quick_cleaner::ui::shell::View::Apps => root.start_apps_scan(cx),
                                    quick_cleaner::ui::shell::View::Disk => root.start_mft_scan(cx),
                                    quick_cleaner::ui::shell::View::Status => root.start_status_monitor(cx),
                                    quick_cleaner::ui::shell::View::Search => root.start_search_index(cx),
                                    quick_cleaner::ui::shell::View::Declutter => root.start_declutter_scan(cx),
                                    _ => {}
                                }
                            });
                        }
                    }
                    OverviewAction::AnalyzeDisk(label) => {
                        open(cx);
                        let handle = *main.lock().unwrap();
                        if let Some(handle) = handle {
                            let _ = handle.update(cx, |root, _, cx| {
                                root.view = quick_cleaner::ui::shell::View::Disk;
                                #[cfg(windows)]
                                {
                                    if let Some(ch) = label.chars().next() {
                                        if ch.is_ascii_alphabetic() {
                                            let vol = quick_cleaner::core::disk::VolumeId::from_drive_letter(ch);
                                            root.switch_disk_volume(vol, cx);
                                        }
                                    }
                                }
                                #[cfg(not(windows))]
                                {
                                    let vol = quick_cleaner::core::disk::VolumeId::from_mount_point(std::path::PathBuf::from(&label));
                                    root.switch_disk_volume(vol, cx);
                                }
                                root.start_mft_scan(cx);
                            });
                        }
                    }
                    OverviewAction::ReleaseMemory => {}
                }
            })
        };
        let overview = OverviewController::new(action_dispatcher);
        let actions = overview.clone();
        let lang = quick_cleaner::core::settings::Settings::load().language;
        let available = tray.install(
            cx,
            TrayMenu {
                open_main: quick_cleaner::ui::i18n::tr_tray_open_main(lang),
                quit: quick_cleaner::ui::i18n::tr_tray_quit(lang),
                on_open_main: open_main.clone(),
            },
            Arc::new(move |action, cx| actions.handle(action, cx)),
            move |session, _, cx| overview.content(session, cx),
        );
        tray_available.store(available, Ordering::Release);
        *reopen_handler.borrow_mut() = Some(open_main.clone());
        open_main(cx);
    });
}
