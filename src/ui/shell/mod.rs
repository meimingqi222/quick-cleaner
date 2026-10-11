//! 直接持有 `Root` 的整窗口外壳：侧栏、顶栏与浮层栈。

pub mod input;
pub mod overlay_stack;
pub mod sidebar;
pub mod topbar;

pub use overlay_stack::{escape_target, EscapeOutcome, OverlayEntry, OverlayId, OverlayStack};
pub use sidebar::{render_sidebar, View};
pub use topbar::render_top_bar;
