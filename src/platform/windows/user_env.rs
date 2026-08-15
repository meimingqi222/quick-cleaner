//! 跨账户提权与前台真实用户环境感知
//!
//! 当标准受限用户通过 Windows UAC 跨账户提权（输入管理员凭据）时，提权后的
//! 新进程运行在管理员的 Profile 下。本模块确保所有扫描、安全白名单和回收站清理
//! 精确锚定实际在屏幕前操作的真实前台用户。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct UserContext {
    pub home: PathBuf,
    pub sid: Option<String>,
}

static USER_CTX: OnceLock<UserContext> = OnceLock::new();

/// 初始化前台真实用户上下文（从命令行参数解析，或回退至当前环境）
pub fn init_user_context() {
    let _ = get_user_context();
}

pub fn get_user_context() -> &'static UserContext {
    USER_CTX.get_or_init(|| {
        let args: Vec<String> = std::env::args().collect();
        let mut passed_home: Option<PathBuf> = None;
        let mut passed_sid: Option<String> = None;

        let mut i = 1;
        while i < args.len() {
            if args[i] == "--orig-user-home" && i + 1 < args.len() {
                passed_home = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            } else if args[i] == "--orig-user-sid" && i + 1 < args.len() {
                passed_sid = Some(args[i + 1].clone());
                i += 2;
            } else {
                i += 1;
            }
        }

        let home = passed_home
            .filter(|p| p.exists())
            .or_else(dirs::home_dir)
            .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("C:\\Users\\Default"));

        let sid = passed_sid.or_else(super::security::current_user_sid);

        UserContext { home, sid }
    })
}

/// 真实前台用户的根目录（如 `C:\Users\Alice`）
pub fn real_user_home() -> &'static Path {
    &get_user_context().home
}

/// 真实前台用户的 Local AppData（如 `C:\Users\Alice\AppData\Local`）
pub fn real_user_local_appdata() -> PathBuf {
    real_user_home().join("AppData\\Local")
}

/// 真实前台用户的 Roaming AppData（如 `C:\Users\Alice\AppData\Roaming`）
pub fn real_user_roaming_appdata() -> PathBuf {
    real_user_home().join("AppData\\Roaming")
}

/// 真实前台用户的 Temp 目录（如 `C:\Users\Alice\AppData\Local\Temp`）
pub fn real_user_temp() -> PathBuf {
    let local_temp = real_user_local_appdata().join("Temp");
    if local_temp.exists() {
        local_temp
    } else {
        std::env::temp_dir()
    }
}

/// 真实前台用户的 Windows SID
pub fn real_user_sid() -> Option<String> {
    get_user_context().sid.clone()
}

/// 系统 UI 语言：中文返回 `Zh`，其余一律 `En`。
///
/// 用 `GetUserDefaultUILanguage`（Windows 的「显示语言」，不是区域格式——
/// 很多人区域是中国但界面装的是英文版，按区域猜会猜反）。
///
/// LANGID 的低 10 位是主语言 ID，`LANG_CHINESE` = 0x04，因此简体、繁体、
/// 港澳台各变体都会落到 `Zh`。
///
/// 跨账户提权（OTS）时这里拿到的是**管理员**的显示语言而不是前台用户的——
/// 拿另一个用户的 UI 语言得去加载他的注册表 hive，代价过大。影响有限：
/// 这只是**首次启动**的默认值，用户切一次语言就会被 `core::settings` 记住。
pub fn detect_system_language() -> crate::core::i18n::Language {
    use crate::core::i18n::Language;
    use winapi::um::winnls::{GetSystemDefaultUILanguage, GetUserDefaultUILanguage};

    const LANG_CHINESE: u16 = 0x04;

    let langid = unsafe {
        let user = GetUserDefaultUILanguage();
        if user == 0 {
            GetSystemDefaultUILanguage()
        } else {
            user
        }
    };

    if langid & 0x3ff == LANG_CHINESE {
        Language::Zh
    } else {
        Language::En
    }
}
