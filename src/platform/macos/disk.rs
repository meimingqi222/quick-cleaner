//! macOS 磁盘与权限查询

use crate::core::disk::{MftError, MftScan};

extern "C" {
    fn geteuid() -> u32;
}

/// macOS 上「已提权」等价于 euid == 0。
///
/// 直接声明 libc 符号，避免只为一个调用引入 `libc` 依赖。
pub fn is_elevated() -> bool {
    unsafe { geteuid() == 0 }
}

/// macOS 只有一个根卷，用 `/` 代表。
pub fn list_ntfs_volumes() -> Vec<char> {
    vec!['/']
}

/// 整树空间分析目前依赖 NTFS 的 `$MFT`，APFS 上没有等价的快速通道。
///
/// 返回 `NotNtfs` 后界面会自动回落到 walkdir 并行扫描。
pub fn scan_volume(_vol: char, _top_n: usize) -> Result<MftScan, MftError> {
    Err(MftError::NotNtfs)
}

pub fn get_volume_space(_vol: char) -> Option<(u64, u64)> {
    None
}

pub fn relaunch_as_admin_if_needed() -> bool {
    true
}


/// macOS 的界面语言：按 POSIX 环境变量判断。
///
/// 更准的做法是读 `AppleLanguages` 用户默认值，但那要拉 Objective-C 运行时；
/// 环境变量在终端启动和 Finder 启动下都够用，何况这只是首次启动的默认值，
/// 用户切一次就会被 `core::settings` 记住。
pub fn detect_system_language() -> crate::core::i18n::Language {
    crate::core::i18n::Language::from_locale_tag(&crate::platform::posix_locale_tag())
}
