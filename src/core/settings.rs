//! 用户设置的读写
//!
//! 目前只有界面语言一项，但结构留好了：新增设置项时给 [`Settings`] 加字段、
//! 在 [`Settings::merge_json`] 里加一行解析即可，旧配置文件不会因此读不出来。
//!
//! # 存在哪
//!
//! Windows 上落在**真实前台用户**的 `%APPDATA%\QuickCleaner\settings.json`。
//! 用户目录由平台门面解析；原因和 `platform::windows::user_env` 里写的一样：
//! 本程序会通过 UAC 自提权，跨账户提权（OTS）时 `dirs::config_dir()` 返回的是
//! **管理员**的 AppData，于是提权前存的设置提权后读不到，看起来就像「设置没保存」。
//!
//! # 读不出来怎么办
//!
//! 一律退回默认值，绝不 panic、也不打断启动。配置文件被手改坏、被杀毒软件截断、
//! 或者是未来版本写的新格式——这些都不该让清理工具打不开。

use crate::core::i18n::Language;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 配置目录名与文件名
const DIR_NAME: &str = "QuickCleaner";
const FILE_NAME: &str = "settings.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 界面语言。首次启动时没有配置文件，由 `platform::detect_system_language()`
    /// 按系统显示语言决定，之后以用户的选择为准。
    pub language: Language,

    /// 磁盘透镜里手动勾选的路径，删除时先送回收站而不是直接抹掉。
    ///
    /// **默认关闭**，而且这个默认值是刻意的：回收站不释放磁盘空间——文件
    /// 还占着原来的簇，只是换了个位置登记。对一个「腾空间」的工具来说，
    /// 默认打开会让用户删完发现可用容量纹丝不动，反而更困惑。
    ///
    /// 只作用于用户手选的任意路径。分类清理走的是固定白名单表（缓存、
    /// 临时文件、构建产物），把它们塞进回收站没有意义，只会让用户再清一次。
    pub delete_to_recycle_bin: bool,

    /// macOS 专属：用户是否已勾选「不再提示完全磁盘访问权限引导」。
    pub macos_fda_dismissed: bool,

    /// 用户白名单：「永远别碰这些路径」的绝对路径清单。
    ///
    /// 生效方式见 `core::whitelist` 模块头注释——合并进 `safety::is_protected`，
    /// 所有删除通道（分类清理/手选/回收站）从删除层就不可绕过。这里只负责
    /// 持久化；运行时的全局查询表由 `core::whitelist::reload` 装载。
    pub whitelist: Vec<String>,

    /// 上一次真实跑 `brew cleanup` 的 Unix 时间（秒）。
    ///
    /// 见 `core::brew`：这是节流依据，间隔不到 `brew::THROTTLE_DAYS` 天
    /// 就不在列表里出现 brew 清理条目——dry-run 不是瞬时命令，每轮扫描
    /// 都跑没有道理。只在真实清理成功后更新。
    pub brew_cleanup_at: Option<i64>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: crate::platform::detect_system_language(),
            delete_to_recycle_bin: false,
            macos_fda_dismissed: false,
            whitelist: Vec::new(),
            brew_cleanup_at: None,
        }
    }
}

impl Settings {
    /// 读取用户设置。文件不存在或解析失败时返回默认值（语言跟随系统）。
    ///
    /// 顺带把白名单装载进全局查询表——`load` 是启动时唯一的设置读取点，
    /// 在这做就不需要调用方记得「读设置后还要刷新白名单」。
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        let settings = Self::merge_json(&text);
        // 装载失败（锁异常）或有条目被丢弃（展开不了的 `~`）：如实记日志。
        // 哑条目静默生效等于保护不存在，用户配了的路径没被保护却无人知晓。
        if !crate::core::whitelist::reload(&settings.whitelist) {
            crate::log!(
                "白名单装载不完整：配置里有展开不了的条目或内部表不可写（{} 条配置）",
                settings.whitelist.len()
            );
        }
        settings
    }

    /// 把设置写回磁盘。尽力而为：写不进去（只读目录、磁盘满、权限不足）
    /// 只是下次启动回到默认值，不值得打断用户手头的操作，因此不返回错误。
    ///
    /// 先写同目录下的临时文件再改名。直接 `fs::write` 是「先截断、后写入」，
    /// 中途断电或被杀进程会留下一个空的或半截的 settings.json；改名在同一个
    /// 卷上是原子的，最坏情况也只是留个临时文件，原配置完好无损。
    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            if std::fs::create_dir_all(dir).is_err() {
                return;
            }
        }
        let Ok(text) = serde_json::to_string_pretty(self) else {
            return;
        };

        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, &text).is_err() {
            return;
        }
        // Windows 上 rename 不能覆盖已存在的目标，得先挪开原文件。
        // 这一步失败（文件被占用）就退回直接覆盖——总比不保存强。
        if std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&path);
            if std::fs::rename(&tmp, &path).is_err() {
                let _ = std::fs::remove_file(&tmp);
                let _ = std::fs::write(&path, &text);
            }
        }
    }

    /// 解析配置文本；任何解析失败都退回默认值。
    ///
    /// 单独拆出来是为了可测——真正的读写要碰用户的 AppData，测不了。
    pub fn merge_json(text: &str) -> Self {
        serde_json::from_str(text).unwrap_or_default()
    }

    /// 配置文件的完整路径。
    pub fn path() -> Option<PathBuf> {
        Some(Self::dir()?.join(FILE_NAME))
    }

    fn dir() -> Option<PathBuf> {
        config_dir()
    }
}

/// 配置目录（`%APPDATA%\QuickCleaner\` / `~/Library/Application
/// Support/QuickCleaner/`）。`pub(crate)`：`core::history` 的审计日志
/// 放在与 settings.json 相同的目录，路径选择规则（Windows 锚定真实
/// 前台用户、不用 dirs::config_dir()，见本模块头注释）必须共用一份，
/// 不能各写一遍。
pub(crate) fn config_dir() -> Option<PathBuf> {
    crate::platform::user_data_dir().map(|d| d.join(DIR_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let s = Settings {
            language: Language::En,
            delete_to_recycle_bin: true,
            macos_fda_dismissed: true,
            whitelist: vec!["/tmp/keep-this".to_string()],
            brew_cleanup_at: Some(1_700_000_000),
        };
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(Settings::merge_json(&text), s);
    }

    /// 配置文件坏掉不能让程序起不来，也不能 panic。
    #[test]
    fn broken_file_falls_back_to_defaults() {
        for junk in ["", "   ", "{", "not json at all", "[1,2,3]", "\0\0\0"] {
            let s = Settings::merge_json(junk);
            assert_eq!(s, Settings::default(), "{junk:?} 应该退回默认值");
        }
    }

    /// 未来版本加了新字段，旧版本读到时要忽略而不是整份丢弃。
    #[test]
    fn unknown_fields_are_ignored() {
        let s = Settings::merge_json(r#"{"language":"En","future_option":42}"#);
        assert_eq!(s.language, Language::En);
    }

    /// 反过来：新版本读旧配置，缺的字段走默认值（靠 `#[serde(default)]`）。
    #[test]
    fn missing_fields_use_defaults() {
        let s = Settings::merge_json("{}");
        assert_eq!(s, Settings::default());
    }

    /// 只写了语言的旧配置文件，读出来时回收站开关要落到默认的「关」。
    #[test]
    fn recycle_bin_defaults_to_off_for_old_config_files() {
        let s = Settings::merge_json(r#"{"language":"Zh"}"#);
        assert_eq!(s.language, Language::Zh);
        assert!(!s.delete_to_recycle_bin, "回收站默认必须是关的");
    }

    #[test]
    fn recycle_bin_flag_round_trips() {
        let s = Settings::merge_json(r#"{"language":"En","delete_to_recycle_bin":true}"#);
        assert!(s.delete_to_recycle_bin);
        assert!(!s.macos_fda_dismissed);
    }

    #[test]
    fn macos_fda_dismissed_round_trips() {
        let s = Settings::merge_json(r#"{"language":"Zh","macos_fda_dismissed":true}"#);
        assert_eq!(s.language, Language::Zh);
        assert!(s.macos_fda_dismissed);
    }

    /// 认得的语言值要真的被采纳，不能被 `unwrap_or_default` 悄悄吃掉。
    #[test]
    fn known_language_values_are_honoured() {
        assert_eq!(
            Settings::merge_json(r#"{"language":"Zh"}"#).language,
            Language::Zh
        );
        assert_eq!(
            Settings::merge_json(r#"{"language":"En"}"#).language,
            Language::En
        );
        // 认不出来的语言名 → 整份退回默认，而不是留个半吊子状态
        assert_eq!(
            Settings::merge_json(r#"{"language":"Klingon"}"#),
            Settings::default()
        );
    }
}
