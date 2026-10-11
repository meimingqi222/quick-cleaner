//! 把 `Root` / `core` 的数据与回调接到 components 上的适配层。
//! components 不得依赖这里。

pub mod dialogs;
pub mod input;
pub mod progress;
pub mod updater_dialog;

pub use dialogs::{
    render_confirm_dialog, render_fda_onboarding_modal, render_residual_modal, ConfirmKind,
    ConfirmRequest,
};
pub use progress::{render_progress_bar, render_uninstall_progress};
pub use updater_dialog::render_update_dialog;

use crate::core::model::Check;
use crate::ui::components::CheckState;

impl From<Check> for CheckState {
    fn from(c: Check) -> Self {
        match c {
            Check::Off => CheckState::Unchecked,
            Check::On => CheckState::Checked,
            Check::Partial => CheckState::Partial,
        }
    }
}
