import importlib.util
from pathlib import Path
import unittest
import contextlib
import io
import tempfile
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('guard', Path(__file__).with_name('check-ui-components.py'))
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


class BoundaryTests(unittest.TestCase):
    def test_qualified_grouped_and_aliased_business_imports(self):
        for source in ('use crate::ui::Root;', 'use crate::{core as domain};', 'use crate::platform as native;', 'use crate as app; app::core::model::Check;', 'use crate::ui::{Root as Owner};'):
            self.assertTrue(guard.violations(source, 'components'), source)

    def test_comments_literals_lifetimes_and_raw_strings(self):
        source = '''// Root
        /* crate::core /* nested */ Root */
        let x = "Root crate::core";
        let y = r##"Root /* on_key_down() */"##;
        let z = '中'; fn f<'a>(x: &'a str) {}'''
        self.assertFalse(guard.violations(source, 'components'))
        self.assertFalse(guard.violations(source, 'views'))

    def test_backdrop_and_drag_but_not_decoration_or_normal_click(self):
        self.assertFalse(guard.violations('div().inset_0().child(icon); div().on_click(action);', 'views'))
        self.assertTrue(guard.violations('div().absolute().inset_0().on_mouse_down(action);', 'views'))
        self.assertTrue(guard.violations('drag_capture(owner, move_cb, up_cb);', 'views'))
        self.assertTrue(guard.violations('div().on_key_down(listener);', 'views'))

    def test_hex_colors_only_outside_page_layers(self):
        source = 'div().bg(rgb(0x7547ab)); // 0xffffff in a comment'
        for layer in ('views', 'adapters', 'shell'):
            self.assertEqual(guard.violations(source, layer), {'ui-no-hex-colors': 1}, layer)
        self.assertFalse(guard.violations(source, 'components'))
        self.assertFalse(guard.violations('div().bg(rgb(ACCENT)); let n = 0x10;', 'views'))

    def test_theme_is_held_to_the_component_rule(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'src/ui').mkdir(parents=True)
            (root / 'src/ui/theme.rs').write_text('use crate::core::categories::Safety;', encoding='utf-8')
            found = guard.scan(root)
            self.assertEqual(found['components-no-business'], {'src/ui/theme.rs': 1})

    def test_tray_content_rejects_window_and_business_control(self):
        for source in (
            'cx.open_window(options, render);', 'window.remove_window();',
            'use gpui::{WindowHandle as Handle};',
            'use crate::{platform as native};', 'use crate as app;',
            'cx.defer(callback);', 'cx.spawn(task);', 'cx.quit();',
            'self.model.update(cx, callback);', 'std::process::Command::new(cmd);',
            'window.on_window_should_close(cx, callback);',
        ):
            self.assertIn('tray-content-no-lifecycle', guard.violations(source, 'tray-content'), source)

    def test_tray_content_accepts_read_render_and_session_events(self):
        source = '''let state = self.model.read(cx); session.emit(Action::Open, cx);
        session.dismiss(cx); cx.observe(&model, callback);
        // cx.open_window();
        let text = r#"cx.spawn(task)"#;
        #[cfg(test)]
        mod tests { fn fixture() { cx.open_window(options, render); } }
        '''
        self.assertFalse(guard.violations(source, 'tray-content'))

    def test_tray_content_scan_covers_split_content_modules(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            content = root / 'src/ui/views/tray_popover'
            content.mkdir(parents=True)
            (content / 'hardware.rs').write_text('cx.spawn(task);', encoding='utf-8')
            self.assertEqual(guard.scan(root)['tray-content-no-lifecycle'],
                             {'src/ui/views/tray_popover/hardware.rs': 1})

    def test_baselines_can_shrink_but_cannot_grow_or_transfer(self):
        old = {'rule': {'a.rs': 2}}
        self.assertFalse(guard.growth({'rule': {'a.rs': 1}}, old))
        self.assertTrue(guard.growth({'rule': {'a.rs': 3}}, old))
        self.assertTrue(guard.growth({'rule': {'a.rs': 1, 'b.rs': 1}}, old))

    def test_completed_migration_cannot_reintroduce_exceptions(self):
        with tempfile.TemporaryDirectory() as directory:
            baseline = Path(directory) / 'baseline.json'
            baseline.write_text('{"components-no-business": {"a.rs": 1}}', encoding='utf-8')
            with patch.object(guard, 'BASELINE', baseline), patch('sys.argv', ['guard']), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(guard.main(), 1)


if __name__ == '__main__':
    unittest.main()
