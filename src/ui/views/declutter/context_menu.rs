//! 冗余整理右键悬浮上下文菜单 (Declutter Context Menu)

use crate::ui::components::icons::{icon_folder_large, icon_search, icon_zip};
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::prelude::*;
use gpui::{div, px, rgb, AnyElement, Context};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct DeclutterContextMenu {
    pub path: PathBuf,
    pub filename: String,
    pub x: f32,
    pub y: f32,
}

pub fn render_declutter_context_menu(root: &Root, cx: &mut Context<Root>) -> Option<AnyElement> {
    let lang = root.language;
    let menu = root.declutter.context_menu.as_ref()?;
    let path = menu.path.clone();
    let filename = menu.filename.clone();
    let path_display = path.to_string_lossy().to_string();

    let x = (menu.x - 10.).clamp(10., 1040.);
    let y = (menu.y - 10.).clamp(10., 620.);

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

    Some(
        div()
            .id("declutter-context-menu-backdrop")
            .absolute()
            .inset_0()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.close_declutter_context_menu();
                    cx.notify();
                }),
            )
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(|this, _, _, cx| {
                    this.close_declutter_context_menu();
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("declutter-context-menu-card")
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(250.))
                    .p_1()
                    .rounded_xl()
                    .bg(rgb(CARD))
                    .border_1()
                    .border_color(rgba(OUTLINE_VAR, 0.45))
                    .shadow_xl()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_mouse_down(gpui::MouseButton::Right, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .border_b_1()
                            .border_color(rgba(OUTLINE_VAR, 0.3))
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
                            ),
                    )
                    // 1. 在访达/资源管理器中定位
                    .child(
                        div()
                            .id("ctx-declutter-reveal")
                            .px_3()
                            .py(px(7.))
                            .rounded_md()
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(rgb(TEXT))
                            .hover(|h| h.bg(rgb(SURF_HIGH)).text_color(rgb(PRIMARY)))
                            .child(icon_folder_large(0x0078d4, 14.))
                            .child(ctx_reveal)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                crate::platform::reveal_in_explorer(&p_reveal);
                                this.close_declutter_context_menu();
                                cx.notify();
                            })),
                    )
                    // 2. 打开文件
                    .child(
                        div()
                            .id("ctx-declutter-open")
                            .px_3()
                            .py(px(7.))
                            .rounded_md()
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(rgb(TEXT))
                            .hover(|h| h.bg(rgb(SURF_HIGH)).text_color(rgb(PRIMARY)))
                            .child(icon_search(0x7547ab, 14.))
                            .child(ctx_open)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                crate::platform::open_in_default_app(&p_open);
                                this.close_declutter_context_menu();
                                cx.notify();
                            })),
                    )
                    // 3. 复制完整路径
                    .child(
                        div()
                            .id("ctx-declutter-copy")
                            .px_3()
                            .py(px(7.))
                            .rounded_md()
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(rgb(TEXT))
                            .hover(|h| h.bg(rgb(SURF_HIGH)).text_color(rgb(PRIMARY)))
                            .child(icon_zip(0x059669, 14.))
                            .child(ctx_copy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    p_copy.clone(),
                                ));
                                this.status = crate::core::i18n::Text::new(
                                    format!("已复制路径: {}", p_copy),
                                    format!("Copied path: {}", p_copy),
                                );
                                this.close_declutter_context_menu();
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element(),
    )
}
