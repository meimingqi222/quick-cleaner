//! 国际化（i18n）语言枚举与通用词条映射

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Language {
    #[default]
    Zh, // 简体中文
    En, // English
}

impl Language {
    pub const ALL: [Language; 2] = [Language::Zh, Language::En];

    pub fn code(&self) -> &'static str {
        match self {
            Language::Zh => "zh",
            Language::En => "en",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Language::Zh => "中文",
            Language::En => "English",
        }
    }

    pub fn short_name(&self) -> &'static str {
        match self {
            Language::Zh => "中",
            Language::En => "EN",
        }
    }

    pub fn toggle(&self) -> Self {
        match self {
            Language::Zh => Language::En,
            Language::En => Language::Zh,
        }
    }
}
