//! 文件日志
//!
//! GUI 程序链接的是 windows 子系统（见 `main.rs` 顶部），没有控制台可看，
//! `println!` 写出去也无人接收。用户报「扫描很慢」「卸载失败」时，唯一能拿到
//! 的现场就是这个文件。性能调优同理——把耗时打进日志，比每次去跑
//! `#[ignore]` 的计时测试更贴近真实启动路径。
//!
//! # 存在哪
//!
//! 和 `settings.json` 同目录：`%APPDATA%\QuickCleaner\quick-cleaner.log`。
//! 同样锚定**真实前台用户**而非 `dirs::config_dir()`——本程序会自提权，
//! 跨账户提权（OTS）时后者返回管理员的 AppData，日志就会散落在两个账户下。
//!
//! # 设计取舍
//!
//! - **尽力而为，绝不打断**：写不进去（磁盘满、权限不足、杀毒软件锁文件）
//!   就静默放弃。清理工具不该因为记不了日志而崩掉或卡住。
//! - **不引 `tracing`/`log`**：本项目只需要「按时间顺序往一个文件追加行」，
//!   为此拉进一整套 facade + subscriber 不划算。
//! - **启动时滚动一次**：超过 [`MAX_BYTES`] 就把旧文件推成 `.1`，只留两代。
//!   不做按大小实时切分——一次运行写不了多少，进程内切分纯属复杂度。

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// 日志文件名。目录复用 [`Settings::dir`](crate::core::settings::Settings)。
const FILE_NAME: &str = "quick-cleaner.log";

/// 上一代日志（滚动后的文件名）。
const OLD_FILE_NAME: &str = "quick-cleaner.log.1";

/// 超过这个大小就在下次启动时滚动。
///
/// 2 MB 大约能装十几万行，够覆盖很多次运行；两代加起来占用也在可接受范围。
const MAX_BYTES: u64 = 2 * 1024 * 1024;

/// 全局单例。`None` 表示日志不可用（目录建不出来、文件打不开），
/// 此后所有写入都是空操作。
static SINK: OnceLock<Option<Mutex<File>>> = OnceLock::new();

/// 日志文件的完整路径。
pub fn path() -> Option<PathBuf> {
    Some(dir()?.join(FILE_NAME))
}

fn dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        // 与 settings.rs 保持一致：锚定真实前台用户，不用 dirs::config_dir()
        Some(crate::platform::windows::real_user_roaming_appdata().join("QuickCleaner"))
    }
    #[cfg(not(windows))]
    {
        dirs::config_dir().map(|d| d.join("QuickCleaner"))
    }
}

/// 打开日志文件，必要时先滚动。任何一步失败都返回 `None`。
fn open() -> Option<Mutex<File>> {
    let path = path()?;
    let parent = path.parent()?;
    std::fs::create_dir_all(parent).ok()?;

    // 超限就把当前这份推成上一代。rename 失败（目标被占用）不算致命，
    // 大不了这次继续往同一个文件追加，下次启动再试。
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > MAX_BYTES {
        let old = parent.join(OLD_FILE_NAME);
        let _ = std::fs::remove_file(&old);
        let _ = std::fs::rename(&path, &old);
    }

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;
    Some(Mutex::new(file))
}

/// 初始化并写入一行会话头。
///
/// 不调用也能用（首次 [`write`] 会自动初始化），但显式调一次能让每次运行
/// 在日志里有明确的分隔，排查时一眼看出「这是哪一次启动」。
pub fn init() {
    let elevated = {
        #[cfg(windows)]
        {
            crate::platform::windows::security::is_elevated()
        }
        #[cfg(not(windows))]
        {
            false
        }
    };
    write(format_args!(
        "===== QuickCleaner v{} 启动 | 提权={} | pid={} =====",
        env!("CARGO_PKG_VERSION"),
        if elevated { "是" } else { "否" },
        std::process::id()
    ));
}

/// 追加一行。带本地时间戳，自动补换行。
///
/// 锁中毒（某个线程写日志时 panic 了）不该让后续写入全部失效，
/// 因此直接取 `into_inner` 继续用。
pub fn write(args: std::fmt::Arguments) {
    let Some(sink) = SINK.get_or_init(open) else {
        return;
    };
    let mut file = match sink.lock() {
        Ok(f) => f,
        Err(poisoned) => poisoned.into_inner(),
    };
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let _ = writeln!(file, "[{ts}] {args}");
    let _ = file.flush();
}

/// 写一行日志，用法同 `println!`。
///
/// ```ignore
/// log!("阶段一扫描完成 {:?}，{} 项", elapsed, items.len());
/// ```
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        $crate::core::log::write(format_args!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 日志不可用时所有写入都必须是空操作，绝不能 panic。
    /// 这里没法真的让 `SINK` 变成 `None`，退而验证正常路径不 panic。
    #[test]
    fn writing_never_panics() {
        write(format_args!("测试写入 {} {:?}", 1, "两"));
        crate::log!("宏形式 {}", 2);
    }

    /// 路径必须落在 QuickCleaner 配置目录下，和 settings.json 同级。
    #[test]
    fn path_sits_next_to_settings() {
        let (Some(log), Some(settings)) = (path(), crate::core::settings::Settings::path()) else {
            return; // 拿不到主目录的环境（CI 容器）直接跳过
        };
        assert_eq!(log.parent(), settings.parent());
        assert_eq!(log.file_name().unwrap(), FILE_NAME);
    }
}
