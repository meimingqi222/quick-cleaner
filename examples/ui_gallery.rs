//! Synthetic component fixtures: no scanning, settings writes or destructive actions.
#![windows_subsystem = "windows"]
use gpui::{
    div, point, prelude::*, px, rgb, size, App, AppContext, Application, Bounds, Context, Render,
    UniformListScrollHandle, Window, WindowBounds, WindowOptions,
};
use quick_cleaner::core::i18n::Language;
use quick_cleaner::ui::{
    components::{
        badge::Badge,
        button::{Button, ButtonVariant},
        dialog::DialogFrame,
        list_row::ListRow,
        menu::{Menu, MenuItem},
        meter::Meter,
        overlay::OverlayPolicy,
        scroll_area::{virtual_list, ScrollArea},
        segmented::{SegmentItem, SegmentedControl},
    },
    i18n::*,
    theme::*,
};
use quick_cleaner::ui::{shell::View, state::StatusTab, Root};

struct Gallery {
    language: Language,
    selected: usize,
    menu: bool,
    dialog: bool,
    scroll: UniformListScrollHandle,
}

struct StaticPage {
    root: gpui::Entity<Root>,
    candidates: bool,
    _subscription: gpui::Subscription,
}
impl Render for StaticPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.candidates {
            self.root.update(cx, |root, cx| {
                if root.monitor.input.text != "mem>" {
                    root.monitor.input.set_text("mem>");
                    cx.notify();
                }
            });
        }
        // Fixtures display real views, but pointer input cannot reach product actions.
        div().size_full().relative().child(self.root.clone()).child(
            gpui::deferred(
                div()
                    .absolute()
                    .inset_0()
                    .occlude()
                    .on_any_mouse_down(|_, _, cx| cx.stop_propagation()),
            )
            .priority(1000),
        )
    }
}
impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.language;
        let mut root = div().size_full().relative().bg(rgb(BG)).text_color(rgb(TEXT)).p_6().flex().flex_col().gap_4()
            .child(div().text_xl().child("QuickCleaner · UI gallery"))
            .child(div().flex().gap_3()
                .child(Button::new("normal", ButtonVariant::Primary).label(tr_start_smart_scan(lang)).on_click(cx.listener(|this, _, _, cx| { this.menu = !this.menu; cx.notify(); })))
                .child(Button::new("disabled", ButtonVariant::Primary).label(tr_start_smart_scan(lang)).disabled(true))
                .child(Button::new("busy", ButtonVariant::Primary).label(tr_start_smart_scan(lang)).busy(true))
                .child(Button::new("dialog", ButtonVariant::Ghost).label(tr_btn_cancel(lang)).on_click(cx.listener(|this, _, _, cx| { this.dialog = true; cx.notify(); }))))
            .child(Badge::new(tr_start_smart_scan(lang)))
            .child(Meter::new(0.64, PRIMARY))
            .child(SegmentedControl::new("segments", self.selected, cx.listener(|this, value, _, cx| { this.selected = *value; cx.notify(); })).items([
                SegmentItem::new(0, tr_start_smart_scan(lang)), SegmentItem::new(1, tr_btn_cancel(lang))]))
            .child(ListRow::new("selected-row").height(36.).selected(true).content(div().child("中文长文本 / Long text / 🙂 / a deliberately long label for clipping verification")))
            .child(div().flex_1().min_h(px(0.)).child(ScrollArea::new("rows", self.scroll.0.borrow().base_handle.clone(), 400., 320_000.,
                virtual_list("virtual", 10_000, self.scroll.clone(), |range, _, _| range.map(|i| ListRow::new(("fixture",i)).height(32.).content(div().child(format!("Fixture {i}")))).collect::<Vec<_>>()))));
        if self.menu {
            let view = cx.entity();
            root = root.child(
                OverlayPolicy::POPUP.layer(
                    Menu::new("menu", point(px(60.), px(110.)))
                        .on_dismiss(move |_, app| {
                            view.update(app, |this, cx| {
                                this.menu = false;
                                cx.notify();
                            })
                        })
                        .child(MenuItem::new("menu-item", tr_btn_cancel(lang)).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.menu = false;
                                cx.notify();
                            }),
                        ))
                        .child(
                            MenuItem::new("disabled-item", tr_start_smart_scan(lang))
                                .disabled(true),
                        ),
                ),
            );
        }
        if self.dialog {
            root = root.child(
                DialogFrame::new("dialog-fixture")
                    .header(div().child(tr_start_smart_scan(lang)))
                    .child(div().child("中文 / English / 🙂"))
                    .footer(
                        Button::new("close", ButtonVariant::Primary)
                            .label(tr_btn_cancel(lang))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dialog = false;
                                cx.notify();
                            })),
                    ),
            );
        }
        root
    }
}
fn main() {
    let page = std::env::args().find_map(|arg| {
        arg.strip_prefix("--page=")
            .and_then(|index| index.parse::<usize>().ok())
    });
    let candidates = std::env::args().any(|arg| arg == "--candidates");
    let loading = std::env::args().any(|arg| arg == "--loading");
    let language = if std::env::args().any(|arg| arg == "--en") {
        Language::En
    } else {
        Language::Zh
    };
    let menu = std::env::args().any(|arg| arg == "--menu");
    let dialog = std::env::args().any(|arg| arg == "--dialog");
    Application::new().run(move |cx: &mut App| {
        if let Some(page) = page {
            let bounds = Bounds::centered(None, size(px(1280.), px(880.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |window, cx| {
                    window.on_window_should_close(cx, |_, cx| {
                        cx.quit();
                        true
                    });
                    let root = cx.new(|cx| {
                        let mut root = Root::new(cx);
                        root.language = language;
                        root.view = View::ALL[page.min(View::ALL.len() - 1)];
                        root.show_fda_onboarding = false;
                        root.fda_status = true;
                        root.elevated = false;
                        root.disk.volumes.clear();
                        root.disk.volume_spaces.clear();
                        root.disk.space = None;
                        root.settings.whitelist.clear();
                        root.junk.scanned = !loading;
                        root.junk.scanning = loading;
                        root.apps.scanned = !loading;
                        root.apps.scanning = loading;
                        root.disk.scanning = loading;
                        root.search.indexing = loading;
                        if !loading {
                            root.monitor.snapshot =
                                Some(quick_cleaner::core::status::StatusSnapshot::default());
                        }
                        if candidates {
                            root.view = View::Status;
                            root.monitor.tab = StatusTab::Processes;
                            root.monitor.input.set_text("mem>");
                            root.monitor.input.focus_handle.focus(window);
                        }
                        root
                    });
                    cx.new(|cx| StaticPage {
                        _subscription: cx.observe(&root, |_, _, cx| cx.notify()),
                        root,
                        candidates,
                    })
                },
            )
            .unwrap();
            cx.activate(true);
            return;
        }
        let bounds = Bounds::centered(None, size(px(1000.), px(760.)), cx);
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
                cx.new(|_| Gallery {
                    language,
                    selected: 0,
                    menu,
                    dialog,
                    scroll: UniformListScrollHandle::new(),
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
