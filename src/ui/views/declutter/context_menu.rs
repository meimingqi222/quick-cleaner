//! 冗余整理右键悬浮上下文菜单 (Declutter Context Menu)

use crate::ui::components::icons::{icon_folder_large, icon_search, icon_zip};
use crate::ui::components::menu::{Menu, MenuBorder, MenuItem, MenuTone};
use crate::ui::components::overlay::OverlayPolicy;
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::prelude::*;
use gpui::{div, point, px, rgb, AnyElement, Context};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct DeclutterContextMenu {
    pub path: PathBuf,
    pub filename: String,
    pub x: f32,
    pub y: f32,
}

/// 菜单左上角相对右键位置的回退量，让光标落在菜单内。
const MENU_CURSOR_OFFSET: f32 = 10.;

pub fn render_declutter_context_menu(root: &Root, cx: &mut Context<Root>) -> Option<AnyElement> {
    let lang = root.language;
    let menu = root.declutter.context_menu.as_ref()?;
    let path = menu.path.clone();
    let filename = menu.filename.clone();
    let path_display = path.to_string_lossy().to_string();

    #[cfg(target_os = "macos")]
    let ctx_reveal = tr_declutter_ctx_reveal_finder(lang);
    #[cfg(windows)]
    let ctx_reveal = tr_declutter_ctx_reveal_explorer(lang);
    #[cfg(not(any(target_os = "macos", windows)))]
    let ctx_reveal = tr_declutter_ctx_reveal_generic(lang);

    let ctx_open = tr_declutter_ctx_open(lang);

    let ctx_copy = tr_declutter_ctx_copy_path(lang);

    let p_reveal = path.clone();
    let p_open = path.clone();
    let p_copy = path_display.clone();

    let header = div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(TEXT))
                .overflow_hidden()
                .child(filename),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(OUTLINE))
                .overflow_hidden()
                .child(path_display),
        );

    let view = cx.entity();
    let menu_view = Menu::new(
        "declutter-context-menu-card",
        point(
            px(menu.x - MENU_CURSOR_OFFSET),
            px(menu.y - MENU_CURSOR_OFFSET),
        ),
    )
    .width(250.)
    .border(MenuBorder::Soft)
    .header(header)
    .on_dismiss(move |_, app| {
        view.update(app, |this, cx| {
            this.dismiss_overlay_outside(crate::ui::shell::OverlayId::DeclutterMenu, cx);
            cx.notify();
        })
    })
    .child(
        MenuItem::new("ctx-declutter-reveal", ctx_reveal)
            .tone(MenuTone::Accent)
            .icon(icon_folder_large(PRIMARY_BRIGHT, 14.))
            .on_click(cx.listener(move |this, _, _, cx| {
                crate::platform::reveal_in_explorer(&p_reveal);
                this.close_declutter_context_menu();
                cx.notify();
            })),
    )
    .child(
        MenuItem::new("ctx-declutter-open", ctx_open)
            .tone(MenuTone::Accent)
            .icon(icon_search(ACCENT, 14.))
            .on_click(cx.listener(move |this, _, _, cx| {
                crate::platform::open_in_default_app(&p_open);
                this.close_declutter_context_menu();
                cx.notify();
            })),
    )
    .child(
        MenuItem::new("ctx-declutter-copy", ctx_copy)
            .tone(MenuTone::Accent)
            .icon(icon_zip(SUCCESS, 14.))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(p_copy.clone()));
                this.status = crate::core::i18n::Text::new(
                    format!("已复制路径: {}", p_copy),
                    format!("Copied path: {}", p_copy),
                );
                this.close_declutter_context_menu();
                cx.notify();
            })),
    );

    Some(
        OverlayPolicy::POPUP.layer_at(
            menu_view,
            root.overlays
                .priority(crate::ui::shell::OverlayId::DeclutterMenu),
        ),
    )
}
