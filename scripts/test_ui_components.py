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
