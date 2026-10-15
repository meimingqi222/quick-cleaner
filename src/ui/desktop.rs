//! 应用菜单文案适配；桌面窗口生命周期由公共宿主维护。
pub use gpui_app_host::desktop::{
    close_window, install_close_handler, main_window_closed, show_main_window,
};

pub fn bind_actions(cx: &mut gpui::App, on_close: impl Fn(&mut gpui::App) + 'static) {
    use crate::ui::i18n::*;
    let lang = crate::core::settings::Settings::load().language;
    gpui_app_host::desktop::bind_actions(
        cx,
        gpui_app_host::desktop::DesktopLabels {
            app: "QuickCleaner",
            hide: tr_desktop_hide(lang),
            hide_others: tr_desktop_hide_others(lang),
            quit: tr_tray_quit(lang),
            window: tr_desktop_window(lang),
            close: tr_desktop_close(lang),
        },
        on_close,
    );
}
