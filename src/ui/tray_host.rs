//! 应用只提供图标、本地化菜单和业务内容；窗口机制由 gpui-app-host 维护。
use gpui::{App, Entity, Render, Window};
pub use gpui_app_host::{ActionHandler, AppCallback, PopoverLifetime, TrayAction, TraySession};
use gpui_app_host::{TrayConfig, TrayHost as SharedTrayHost, TrayMenuItem};
use std::sync::Arc;

pub struct TrayMenu<'a> {
    pub open_main: &'a str,
    pub quit: &'a str,
    pub on_open_main: AppCallback,
}

#[derive(Clone)]
pub struct TrayHost(SharedTrayHost);
impl std::ops::Deref for TrayHost {
    type Target = SharedTrayHost;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl TrayHost {
    pub fn new(width: f32, height: f32) -> Self {
        Self(SharedTrayHost::new(width, height))
    }
    pub fn install<A: TrayAction, V: Render + 'static>(
        &self,
        cx: &mut App,
        menu: TrayMenu<'_>,
        handler: ActionHandler<A>,
        content: impl Fn(TraySession<A>, &mut Window, &mut App) -> Entity<V> + 'static,
    ) -> bool {
        #[cfg(target_os = "macos")]
        let bytes: &[u8] = include_bytes!("../../assets/tray_icon_mac.png");
        #[cfg(not(target_os = "macos"))]
        let bytes: &[u8] = include_bytes!("../../assets/tray_icon_win.ico");
        let icon = image::load_from_memory(bytes);
        let result = icon.map_err(|error| error.to_string()).and_then(|image| {
            let icon = image.into_rgba8();
            let (icon_width, icon_height) = icon.dimensions();
            self.0.install(
                cx,
                TrayConfig {
                    tooltip: "QuickCleaner".into(),
                    icon_rgba: icon.into_raw(),
                    icon_width,
                    icon_height,
                    template: cfg!(target_os = "macos"),
                    menu: vec![
                        TrayMenuItem {
                            label: menu.open_main.into(),
                            action: menu.on_open_main,
                            dismiss_popover: true,
                        },
                        TrayMenuItem {
                            label: menu.quit.into(),
                            action: Arc::new(|cx| cx.quit()),
                            dismiss_popover: true,
                        },
                    ],
                },
                handler,
                content,
            )
        });
        if let Err(error) = &result {
            crate::core::log::write(format_args!("tray initialization failed: {error}"));
        }
        result.is_ok()
    }
}
