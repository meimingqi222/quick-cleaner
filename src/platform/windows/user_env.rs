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

/// 需要纳入删除保护的「已知文件夹」在 `User Shell Folders` 里的值名。
///
/// `Personal` 是文档，那串 GUID 是下载——键名和显示名对不上是 Shell 的历史包袱。
const KNOWN_FOLDER_VALUES: &[&str] = &[
    "Desktop",
    "Personal",
    "My Pictures",
    "My Video",
    "My Music",
    "{374DE290-123F-4565-9164-39C4925E467B}",
    "Favorites",
];

/// 真实前台用户「桌面 / 文档 / 下载 / 图片……」的**实际**落点。
///
/// 不能靠 `%USERPROFILE%\Desktop` 硬拼：
///
/// - OneDrive 的「备份重要文件夹」会把桌面、文档、图片整体重定向到
///   `%USERPROFILE%\OneDrive\桌面`，硬拼出来的那个目录甚至不存在；
/// - 中文系统上这些目录在磁盘上的名字本身就是本地化的；
/// - 企业环境常把它们重定向到网络盘或另一个分区。
///
/// 三种情况下硬拼的路径都保护不到用户真正的桌面，而那正是最不该被误删的地方。
///
/// 读注册表而不是调 `SHGetKnownFolderPath`：后者返回**当前进程**用户的路径，
/// 跨账户提权（OTS）时那是管理员的桌面——正好保护错了人。
pub fn real_user_known_folders() -> &'static [PathBuf] {
    static FOLDERS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    FOLDERS.get_or_init(|| {
        use winapi::um::winreg::{HKEY_CURRENT_USER, HKEY_USERS};
        const SUBPATH: &str =
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders";

        // 优先读真实前台用户的 hive；OTS 提权时 HKCU 指向的是管理员。
        // 拿不到 SID、或那个 hive 没加载时退回 HKCU。
        let entries = real_user_sid()
            .map(|sid| {
                super::registry::enum_string_values(HKEY_USERS, &format!(r"{sid}\{SUBPATH}"), 0)
            })
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| super::registry::enum_string_values(HKEY_CURRENT_USER, SUBPATH, 0));

        entries
            .into_iter()
            .filter(|(name, _)| {
                KNOWN_FOLDER_VALUES
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case(name))
            })
            .map(|(_, raw)| expand_user_profile(&raw))
            .filter(|p| p.is_absolute())
            .collect()
    })
}

/// 展开 `User Shell Folders` 里的 `%USERPROFILE%` 前缀。
///
/// 这些值是 `REG_EXPAND_SZ`，绝大多数形如 `%USERPROFILE%\Desktop`。展开时
/// 锚定**真实前台用户**的主目录，而不是进程环境里的那个。
fn expand_user_profile(raw: &str) -> PathBuf {
    const VAR: &str = "%USERPROFILE%";
    if raw.len() >= VAR.len() && raw[..VAR.len()].eq_ignore_ascii_case(VAR) {
        return real_user_home().join(raw[VAR.len()..].trim_start_matches(std::path::is_separator));
    }
    PathBuf::from(raw)
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
