//! Material 3 浅色设计系统调色板与基础样式常量

use crate::core::categories::Safety;
use gpui::{rgb, Hsla};

pub const BG: u32 = 0xf9f9f9; // surface / background：侧边栏、底栏
pub const CARD: u32 = 0xffffff; // surface-container-lowest：卡片、主内容区
pub const SURF_LOW: u32 = 0xf3f3f3; // 展开区、行悬停
pub const SURF: u32 = 0xeeeeee; // 分段控件底槽
pub const SURF_HIGH: u32 = 0xe8e8e8; // 导航选中态
pub const SURF_HIGHEST: u32 = 0xe2e2e2; // 进度条/环形轨道
pub const TEXT: u32 = 0x1a1c1c; // on-surface
pub const MUTED: u32 = 0x404752; // on-surface-variant
pub const OUTLINE: u32 = 0x717783;
pub const OUTLINE_VAR: u32 = 0xc0c7d4;
pub const PRIMARY: u32 = 0x005faa;
pub const PRIMARY_BRIGHT: u32 = 0x0078d4; // primary-container，渐变亮端
pub const PRIMARY_FIXED: u32 = 0xd3e3ff;
pub const ON_PRIMARY: u32 = 0xffffff;
pub const CAUTION: u32 = 0x974700;
pub const CAUTION_CONTAINER: u32 = 0xffdbc8;
pub const ERROR: u32 = 0xba1a1a;
pub const ERROR_CONTAINER: u32 = 0xffdad6;

/// 生成带透明度的 Hsla 颜色
pub fn rgba(hex: u32, alpha: f32) -> Hsla {
    let mut c = Hsla::from(rgb(hex));
    c.a = alpha;
    c
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
