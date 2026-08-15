//! 用户设置的读写
//!
//! 目前只有界面语言一项，但结构留好了：新增设置项时给 [`Settings`] 加字段、
//! 在 [`Settings::merge_json`] 里加一行解析即可，旧配置文件不会因此读不出来。
//!
//! # 存在哪
//!
//! Windows 上落在**真实前台用户**的 `%APPDATA%\QuickCleaner\settings.json`，
//! 而不是 `dirs::config_dir()`。原因和 `platform::windows::user_env` 里写的一样：
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: crate::platform::detect_system_language(),
        }
    }
}

impl Settings {
    /// 读取用户设置。文件不存在或解析失败时返回默认值（语言跟随系统）。
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        Self::merge_json(&text)
    }

    /// 把设置写回磁盘。尽力而为：写不进去（只读目录、磁盘满、权限不足）
    /// 只是下次启动回到默认值，不值得打断用户手头的操作，因此不返回错误。
    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            if std::fs::create_dir_all(dir).is_err() {
                return;
            }
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, text);
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
        #[cfg(windows)]
        {
            // 锚定真实前台用户，不能用 dirs::config_dir()——见模块头注释
            Some(crate::platform::windows::real_user_roaming_appdata().join(DIR_NAME))
        }
        #[cfg(not(windows))]
        {
            dirs::config_dir().map(|d| d.join(DIR_NAME))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let s = Settings {
            language: Language::En,
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

    /// 认得的语言值要真的被采纳，不能被 `unwrap_or_default` 悄悄吃掉。
    #[test]
    fn known_language_values_are_honoured() {
        assert_eq!(Settings::merge_json(r#"{"language":"Zh"}"#).language, Language::Zh);
        assert_eq!(Settings::merge_json(r#"{"language":"En"}"#).language, Language::En);
        // 认不出来的语言名 → 整份退回默认，而不是留个半吊子状态
        assert_eq!(Settings::merge_json(r#"{"language":"Klingon"}"#), Settings::default());
    }
}
