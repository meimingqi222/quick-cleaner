//! 可复用 UI 控件库
//!
//! # 关于 `use ...::*`
//!
//! 视图文件里对 `theme` / `i18n` / `icons` 一律用 glob 导入，这是刻意的：
//! 这三个模块导出的都是**带统一前缀的词汇表**——`PRIMARY`/`SURF_HIGH`、
//! `tr_btn_*`、`icon_*`——名字本身就说明了出处，逐个列出来只会让每个视图
//! 顶着几十行 import。
//!
//! 本模块和 `views` 不适用这条：`card` / `checkbox` / `small_button` 之间
//! 没有共同前缀，glob 进来就分不清谁来自哪儿，所以调用方显式列出。

pub mod buttons;
pub mod cards;
pub mod controls;
pub mod dialogs;
pub mod donut;
pub mod icons;
pub mod progress;
pub mod scroll;
pub mod search_box;
pub mod sidebar;
pub mod tooltip;
pub mod topbar;

pub use buttons::{danger_button, ghost_button, primary_button, small_button};
pub use cards::{card, stat_tile};
pub use controls::{badge, checkbox, page_heading};
pub use dialogs::{
    render_confirm_dialog, render_fda_onboarding_modal, render_residual_modal, ConfirmKind,
    ConfirmRequest,
};
pub use icons::*;
pub use progress::{render_progress_bar, render_scan_line, render_uninstall_progress};
pub use scroll::{drag_to_offset, scroll_metrics, scrollbar, SCROLLBAR_W};
pub use search_box::{search_box, SearchBoxSpec};
pub use sidebar::{render_sidebar, View};
pub use tooltip::{path_tooltip, text_tooltip};
pub use topbar::render_top_bar;
