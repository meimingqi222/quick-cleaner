import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('guard', Path(__file__).with_name('check-ui-localization.py'))
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


class InlineCopyTests(unittest.TestCase):
    def test_every_inline_shape_is_counted(self):
        shapes = (
            'let t = if lang == Language::Zh { "名称" } else { "Name" };',
            'let (a, b) = match lang { Language::Zh => ("标题", "副标题"), Language::En => ("T", "S") };',
            '(View::Junk, Language::Zh) => "智能清理",',
            'self.status = bilingual(move |_| format!("正在移除 {n} 项"));',
            'div().child("定位")',
            'let sep = "、";',
            'format!("· {}：{}", a, b)',
            'let s = r#"原始串"#;',
        )
        for source in shapes:
            expected = 2 if '("标题", "副标题")' in source else 1
            self.assertEqual(guard.inline_count(source), expected, source)

    def test_diagnostics_comments_and_tests_are_not_ui_copy(self):
        source = '''// 注释里的中文
        /* 块注释 "中文" */
        crate::log!("批量移除失败「{name}」：{}", detail);
        eprintln!("调试");
        assert_eq!(x, 1, "断言信息");
        let v = opt.expect("不该为空");
        let ok = "plain ascii";
        #[cfg(test)]
        mod tests {
            #[test]
            fn t() { let s = "测试数据"; }
        }'''
        self.assertEqual(guard.inline_count(source), 0)

    def test_code_after_a_diagnostic_call_is_still_scanned(self):
        source = 'crate::log!("日志 {}", f(")")); div().child("界面文案");'
        self.assertEqual(guard.inline_count(source), 1)


if __name__ == '__main__':
    unittest.main()
