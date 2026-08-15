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

    /// 从 POSIX/BCP-47 风格的语言标记推断语言：`zh` 开头算中文，其余一律英文。
    ///
    /// 认得 `zh`、`zh-CN`、`zh_TW.UTF-8`、`ZH-Hans` 这些写法。只有两种语言可选，
    /// 所以「不是中文就是英文」是准确的兜底，而不是偷懒。
    pub fn from_locale_tag(tag: &str) -> Self {
        let head = tag
            .split(['-', '_', '.', '@', ','])
            .next()
            .unwrap_or("")
            .trim();
        if head.eq_ignore_ascii_case("zh") {
            Language::Zh
        } else {
            Language::En
        }
    }
}

/// 一条随数据一起走的双语文案。
///
/// 扫描跑在后台线程上，那时还不知道用户之后会切到哪种语言；而语言开关
/// 必须立刻生效、不能触发重扫。所以扫描结果里的标签两种语言各存一份，
/// 渲染时才按当前语言取。
///
/// 大量标签（路径、`%TEMP%`、`Chrome` 这类品牌名）两种语言是同一个字符串，
/// 用 [`Text::same`] 构造，只存一份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Text {
    /// 中英一致，只存一份
    Same(String),
    /// 中英各存一份
    Pair { zh: String, en: String },
}

impl Text {
    pub fn new(zh: impl Into<String>, en: impl Into<String>) -> Self {
        Text::Pair {
            zh: zh.into(),
            en: en.into(),
        }
    }

    /// 两种语言下都是同一个字符串（路径、命令名、品牌名）。
    pub fn same(s: impl Into<String>) -> Self {
        Text::Same(s.into())
    }

    pub fn get(&self, lang: Language) -> &str {
        match self {
            Text::Same(s) => s,
            Text::Pair { zh, en } => match lang {
                Language::Zh => zh,
                Language::En => en,
            },
        }
    }

}

/// 把一个「按语言产文案」的函数求值成双语 [`Text`]。
///
/// 状态栏那批 `tr_status_*(lang, args…) -> String` 就是这个形状：
/// `bilingual(|l| tr_status_scan_done(l, &size))` 会把两种语言各算一遍存下来，
/// 用户之后切语言，已经写进状态栏的那句话也会跟着变。
pub fn bilingual(f: impl Fn(Language) -> String) -> Text {
    Text::Pair {
        zh: f(Language::Zh),
        en: f(Language::En),
    }
}

impl From<&str> for Text {
    fn from(s: &str) -> Self {
        Text::same(s)
    }
}

impl From<String> for Text {
    fn from(s: String) -> Self {
        Text::same(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_tags_map_to_a_language() {
        for zh in ["zh", "zh-CN", "zh_TW.UTF-8", "ZH-Hans", "zh_HK", "zh-Hant-MO"] {
            assert_eq!(Language::from_locale_tag(zh), Language::Zh, "{zh}");
        }
        for en in ["en", "en-US", "en_GB.UTF-8", "ja-JP", "de_DE", "ru", ""] {
            assert_eq!(Language::from_locale_tag(en), Language::En, "{en}");
        }
    }

    /// 别把 `zh` 当前缀匹配：`zho`、`zh-like` 这种得看清楚分隔符。
    #[test]
    fn locale_tag_matches_whole_subtag_only() {
        assert_eq!(Language::from_locale_tag("zhosomething"), Language::En);
        assert_eq!(Language::from_locale_tag("zhz-CN"), Language::En);
        // 但真正的中文标记不能被误伤
        assert_eq!(Language::from_locale_tag("zh.UTF-8"), Language::Zh);
    }

    #[test]
    fn same_text_reads_identically_in_both_languages() {
        let t = Text::same("%TEMP%");
        assert_eq!(t.get(Language::Zh), "%TEMP%");
        assert_eq!(t.get(Language::En), "%TEMP%");
    }

    #[test]
    fn pair_text_switches_with_language() {
        let t = Text::new("npm 缓存", "npm cache");
        assert_eq!(t.get(Language::Zh), "npm 缓存");
        assert_eq!(t.get(Language::En), "npm cache");
    }

}
