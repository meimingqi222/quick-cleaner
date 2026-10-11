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

use gpui::{App, ClickEvent, Window};
use std::rc::Rc;

pub(crate) type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

pub mod action_bar;
pub mod badge;
pub mod button;
pub mod cards;
pub mod controls;
pub mod dialog;
pub mod donut;
pub mod feedback;
pub mod icons;
pub mod input;
pub mod list_row;
pub mod menu;
pub mod meter;
pub mod overlay;
pub mod popover;
pub mod progress;
pub mod scroll;
pub mod scroll_area;
pub mod search_input;
pub mod segmented;
pub mod tooltip;

pub use cards::card;
pub use controls::{checkbox, page_heading, CheckState};
pub use icons::*;
pub use progress::render_scan_line;
pub use tooltip::{path_tooltip, text_tooltip};

pub use search_input::{search_box, SearchBoxSpec};
