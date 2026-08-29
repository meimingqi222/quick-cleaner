//! 清理目标占用状态的跨平台策略与统一门面。
//!
//! 本模块定义平台无关的占用状态、把平台检测结果合入扫描条目的策略，并将
//! 检测与删除前定点复检委托给 `platform` 门面。具体的进程、句柄和活数据库
//! 探测由各平台实现。

use crate::core::i18n::{bilingual, Text};
use crate::core::scanner::CategorySummary;
use std::collections::HashMap;
use std::path::PathBuf;

/// 一个目标的占用状态：三态，`unknown` 是“测不出”专用状态。
///
/// `app` 是平台推断出的归属应用；`open` 表示确实发现进程打开目标；
/// `unknown` 表示探测失败或结果不完整。测不出不等于空闲，因此 `unknown`
/// 与 `open` 都会阻止清理。三者可以同时成立，徽标优先展示解释力更强的应用名。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Busy {
    pub app: Option<String>,
    pub open: bool,
    /// 占用检测本身失败/超时/输出不完整，测不出真实状态。按占用处理。
    pub unknown: bool,
}

impl Busy {
    fn is_empty(&self) -> bool {
        self.app.is_none() && !self.open && !self.unknown
    }

    /// 徽标文案：`Some((文案, 是否应用级))`。
    ///
    /// 返回的布尔值只控制 UI 配色；清理入口对三种占用状态一视同仁。
    pub fn badge(&self) -> Option<(Text, bool)> {
        if let Some(app) = &self.app {
            return Some((
                bilingual(|l| match l {
                    crate::core::i18n::Language::Zh => format!("应用打开中 · {app}"),
                    crate::core::i18n::Language::En => format!("{app} running"),
                }),
                true,
            ));
        }
        if self.open {
            return Some((
                bilingual(|l| match l {
                    crate::core::i18n::Language::Zh => String::from("系统占用"),
                    crate::core::i18n::Language::En => String::from("In use"),
                }),
                false,
            ));
        }
        if self.unknown {
            return Some((
                bilingual(|l| match l {
                    crate::core::i18n::Language::Zh => String::from("占用状态未知"),
                    crate::core::i18n::Language::En => String::from("Busy status unknown"),
                }),
                false,
            ));
        }
        None
    }
}

/// 委托当前平台批量检测目标占用状态。
pub fn detect(targets: &[PathBuf]) -> HashMap<PathBuf, Busy> {
    crate::platform::detect_inuse(targets)
}

/// 把检测结果合入扫描条目，并给被占用的条目降级 `recommended`。
///
/// 返回受影响的条目数。直接修改推荐态，保证“当前选择是否等于推荐选择”
/// 的比较不需要额外维护一套占用过滤规则。
pub fn apply_busy(categories: &mut [CategorySummary], busy: &HashMap<PathBuf, Busy>) -> usize {
    let mut n = 0;
    for cat in categories {
        for item in &mut cat.items {
            if let Some(b) = busy.get(&item.path) {
                if !b.is_empty() {
                    item.busy = Some(b.clone());
                    item.recommended = false;
                    n += 1;
                }
            }
        }
    }
    n
}

/// 定点复检结果：给定目标“现在”是否仍然干净可删。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpotCheck {
    /// 没有发现进程打开目标。
    Clear,
    /// 发现目标在扫描快照之后被打开。
    Busy,
    /// 复检失败或结果不完整，按占用处理。
    Unknown,
}

/// 委托当前平台对一小批路径做删除前占用复检。
pub fn spot_check(paths: &[PathBuf]) -> HashMap<PathBuf, SpotCheck> {
    crate::platform::spot_check_inuse(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_prefers_app_over_open() {
        let b = Busy {
            app: Some("Edge".into()),
            open: true,
            unknown: false,
        };
        let (text, app_level) = b.badge().unwrap();
        assert!(text.get(crate::core::i18n::Language::Zh).contains("Edge"));
        assert!(app_level);
        assert!(
            !Busy {
                app: None,
                open: true,
                unknown: false
            }
            .badge()
            .unwrap()
            .1
        );
        assert!(Busy::default().badge().is_none());
    }

    #[test]
    fn badge_shows_unknown_when_detection_failed() {
        let (text, app_level) = Busy {
            app: None,
            open: false,
            unknown: true,
        }
        .badge()
        .unwrap();
        assert!(!text.get(crate::core::i18n::Language::Zh).is_empty());
        assert!(!app_level);
    }

    #[test]
    fn is_empty_accounts_for_unknown() {
        assert!(Busy::default().is_empty());
        assert!(!Busy {
            app: None,
            open: false,
            unknown: true
        }
        .is_empty());
    }

    #[test]
    fn apply_busy_downgrades_recommended() {
        use crate::core::categories::CategoryId;
        use crate::core::scanner::ScanItem;
        let item = |p: &str| ScanItem {
            path: PathBuf::from(p),
            label: Text::same("x"),
            size: 1,
            file_count: 0,
            category: CategoryId::UserTemp,
            last_modified: 0,
            recommended: true,
            busy: None,
            identity: None,
        };
        let mut cats = vec![CategorySummary {
            category: CategoryId::UserTemp,
            total_size: 2,
            items: vec![item("/a"), item("/b")],
            partial: false,
        }];
        let busy = HashMap::from([(
            PathBuf::from("/a"),
            Busy {
                app: Some("A".into()),
                open: false,
                unknown: false,
            },
        )]);
        assert_eq!(apply_busy(&mut cats, &busy), 1);
        assert!(cats[0].items[0].busy.is_some());
        assert!(!cats[0].items[0].recommended);
        assert!(cats[0].items[1].busy.is_none());
    }
}
