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

use crate::core::categories::Safety;
use crate::core::model::Check;
use crate::ui::components::CheckState;
use crate::ui::theme::{
    CAUTION, CAUTION_CONTAINER, ERROR, ERROR_CONTAINER, PRIMARY, PRIMARY_FIXED,
};

impl From<Check> for CheckState {
    fn from(c: Check) -> Self {
        match c {
            Check::Off => CheckState::Unchecked,
            Check::On => CheckState::Checked,
            Check::Partial => CheckState::Partial,
        }
    }
}

/// 安全等级 -> 强调色（用在大小数字、进度条、状态点上）。
///
/// 配色只属于 UI 层：领域层的 `Safety` 只表达风险等级，不该知道十六进制色值。
pub fn safety_color(s: Safety) -> u32 {
    match s {
        Safety::Safe => PRIMARY,
        Safety::Caution => CAUTION,
        Safety::Danger => ERROR,
    }
}

/// 安全等级 -> 容器色（图标底板这类大色块用）。
pub fn safety_container(s: Safety) -> u32 {
    match s {
        Safety::Safe => PRIMARY_FIXED,
        Safety::Caution => CAUTION_CONTAINER,
        Safety::Danger => ERROR_CONTAINER,
    }
}
