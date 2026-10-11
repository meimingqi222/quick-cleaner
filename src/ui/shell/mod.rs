//! 直接持有 `Root` 的整窗口外壳：侧栏与顶栏。

pub mod sidebar;
pub mod topbar;

pub use sidebar::{render_sidebar, View};
pub use topbar::render_top_bar;
