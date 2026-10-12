# Agent Note: 本地化守卫只认一种写法，89 处内联中文漏网，部分英文界面显示中文

Status: implemented

## Problem

`scripts/check-ui-localization.py` 只匹配 `Language::Zh => "` / `Language::Zh => format!(`，
基线报 0，看起来 UI 文案已经全部走 `tr_*`。实际上 `src/ui` 里还有 89 处内联中文：
`if lang == Language::Zh {..}`、元组 `match lang`、`(View::X, Language::Zh) =>`、
`bilingual(|l| match l { Language::Zh => {..} })`，以及**根本没有英文分支**的硬编码。
后一类是用户可见的 bug：英文界面下开发环境批量移除的进度和摘要、相似照片的「定位 / 看图」、
磁盘面包屑的「‹ 收起」、整理页右键「已复制路径」、残留弹窗 launchd 证据行的全角冒号，
都显示中文。英文版批量移除摘要还漏掉了「跳过 N 项」分支。另有两处把中文塞进
`ScanError::Io` payload，违反 `ui/i18n` 模块「core/platform 错误 payload 语言中立」的约束。

## Decision

守卫改为同时统计「i18n 之外的 UI 代码里含中文（含全角标点）的字符串字面量」。
扫描前剔除注释、`#[cfg(test)] mod` 块、日志 / 断言 / panic 宏和 `.expect(..)` 的参数，
这些是开发者诊断信息而不是界面文案。基线仍为 0，所以这是一条全面禁令。
89 处全部迁到 `ui/i18n` 的 `tr_*`，文案逐字保留；只有中文的那些补了英文。
`ScanError::Io` 的两处 payload 改为语言中立的英文短语。

`lang == Language::Zh` 本身不禁：语言切换器的选中态、从 core 双语字段里取值（`kind_zh` /
`title_zh`）都是合法逻辑，没有内联文案。

## Alternatives considered

按 `Language::Zh` 出现次数计数：会误伤语言切换器和数据字段取值，又抓不到完全没有英文分支的
硬编码，而这类恰恰是真正漏翻的 bug。

给每种写法加一条正则（`if lang ==`、元组 `match`……）：写法是开放集合，下一种新写法照样漏网。
中文字面量是所有漏网写法的共同特征，按它判定才能封住。

## Consequences

新增界面文案只能放进 `ui/i18n`。日志仍可写中文；如果某条日志会被拼进状态栏或弹窗，
它就是界面文案，必须走 `tr_*`。

## Verification

- `scripts/test_ui_localization.py::test_every_inline_shape_is_counted`
- `scripts/test_ui_localization.py::test_diagnostics_comments_and_tests_are_not_ui_copy`
- `src/ui/i18n/views.rs::english_copy_has_no_cjk_and_keeps_every_count`

Proved: 守卫为 TDD 先红：旧规则在迁移前代码上报 `remaining baseline: 0`，新规则在同一份代码上
报 89 处，见 `docs/agent-notes-evidence/2026-10-12-ui-inline-cjk-red.log`。
`english_copy_has_no_cjk_and_keeps_every_count` 为制造型红跑：删掉
`tr_dev_batch_remove_done` 英文的跳过分支后 focused 失败，恢复后转绿，见
`docs/agent-notes-evidence/2026-10-12-ui-inline-cjk-test-red.log` 与
`docs/agent-notes-evidence/2026-10-12-ui-inline-cjk-test-green.log`。

Pitfalls checked: P1–P57 inspected; 只动文案取值与 UI 守卫，未碰卸载、删除、路径保护和搜索语法。
新增 P58 记录此坑。
