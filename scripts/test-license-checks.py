# SPDX-License-Identifier: Apache-2.0
"""Regression cases for missing, altered and executable redistribution notices."""
import importlib.util
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location('native_notices', ROOT / 'scripts/check-native-package-notices.py')
checks = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checks)
license_spec = importlib.util.spec_from_file_location('license_checks', ROOT / 'scripts/check-licenses.py')
license_checks = importlib.util.module_from_spec(license_spec)
license_spec.loader.exec_module(license_checks)
notice_spec = importlib.util.spec_from_file_location('notice_generator', ROOT / 'scripts/prepare-license-notices.py')
notice_generator = importlib.util.module_from_spec(notice_spec)
notice_spec.loader.exec_module(notice_generator)


class NoticeRegressionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='dock-notice-regression-')
        self.root = Path(self.temp.name)
        self.base = self.root / 'usr/share/licenses/codex-dock'
        for name, source in checks.expected_notices().items():
            target = self.base / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)

    def tearDown(self):
        self.temp.cleanup()

    def test_source_uninstall_with_spaces_preserves_adjacent_and_profile_data(self):
        prefix = self.root / 'account data'
        adjacent = self.root / 'account'
        adjacent.write_bytes(b'not application data')
        profile = self.root / '.codex-profiles/test/auth.json'
        profile.parent.mkdir(parents=True)
        profile.write_bytes(b'disposable authentication sentinel')
        profile.chmod(0o600)
        for name in ('bin/codex-dock', 'bin/codex-dock-cli',
                     'share/applications/codex-dock.desktop',
                     'share/licenses/codex-dock/LICENSE'):
            path = prefix / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'disposable application file')
        autostart = self.root / 'autostart/codex-dock.desktop'
        subprocess.run(['make', 'uninstall', f'PREFIX={prefix}', f'AUTOSTART={autostart}'],
                       cwd=ROOT, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.assertEqual(adjacent.read_bytes(), b'not application data')
        self.assertEqual(profile.read_bytes(), b'disposable authentication sentinel')
        self.assertEqual(profile.stat().st_mode & 0o777, 0o600)
        self.assertFalse((prefix / 'bin/codex-dock').exists())
        self.assertFalse((prefix / 'share/licenses/codex-dock').exists())

    def test_nested_notices_are_collected_but_source_modules_are_not(self):
        root = self.root / 'crate'
        (root / 'src/vendor').mkdir(parents=True)
        (root / 'LICENSE').write_text('top-level terms')
        (root / 'src/vendor/LICENSE').write_text('nested terms')
        (root / 'src/copying.rs').write_text('source module, not a legal notice')
        self.assertEqual([str(p.relative_to(root)) for p in notice_generator.notice_files(root)],
                         ['LICENSE', 'src/vendor/LICENSE'])

    def test_upstream_crlf_notice_bytes_are_preserved(self):
        source = self.root / 'upstream-license'
        target = self.root / 'retained-license'
        original = b'Synthetic copyright notice\r\nPermission terms\r\n'
        source.write_bytes(original)
        notice_generator.write_notice(target, notice_generator.read_notice(source))
        self.assertEqual(target.read_bytes(), original)

    def test_complete_payload_passes(self):
        checks.check_tree(self.root)

    def test_missing_attribution_is_rejected(self):
        (self.base / 'NOTICE').unlink()
        with self.assertRaisesRegex(ValueError, 'Missing/altered notice'):
            checks.check_tree(self.root)

    def test_modified_dependency_notice_is_rejected(self):
        with (self.base / 'THIRD-PARTY-NOTICES.txt').open('ab') as target:
            target.write(b'\naltered\n')
        with self.assertRaisesRegex(ValueError, 'Missing/altered notice'):
            checks.check_tree(self.root)

    def test_executable_license_data_is_rejected(self):
        (self.base / 'LICENSE').chmod(0o755)
        with self.assertRaisesRegex(ValueError, 'Executable license data'):
            checks.check_tree(self.root)

    def test_open_obligations_block_release(self):
        with self.assertRaisesRegex(ValueError, 'Public redistribution remains blocked'):
            license_checks.require_release_clearance({'unresolved': ['synthetic-source-obligation']})

    def test_resolved_obligations_pass_the_clearance_check(self):
        license_checks.require_release_clearance({'unresolved': []})



if __name__ == '__main__':
    unittest.main()
