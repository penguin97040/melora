import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('package_linux', Path(__file__).resolve().parents[1] / 'package-linux.py')
packaging = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(packaging)

DESKTOP = '''[Desktop Entry]
Categories=AudioVideo;Audio;Music;
Comment=A personal desktop music client
Exec=melora
StartupWMClass=melora
Icon=melora
Name=Melora
Terminal=false
Type=Application
'''


class LinuxPackagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.previous = Path.cwd()
        os.chdir(self.temp.name)
        self.addCleanup(os.chdir, self.previous)
        notices = Path('artifacts/dependency-notices')
        notices.mkdir(parents=True)
        (notices / 'dependency-licences.json').write_text('[]\n')
        (notices / 'LICENSE.txt').write_text('Synthetic test notice\n')
        manifest = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in notices.iterdir()}
        Path('artifacts/dependency-notices-manifest.json').write_text(json.dumps(manifest))
        fonts = Path('public/fonts/licences')
        fonts.mkdir(parents=True)
        for name in packaging.windows.FONT_LICENCES:
            (fonts / name).write_text('Synthetic font licence\n')
        Path('THIRD_PARTY_NOTICES.md').write_text('Synthetic runtime notices\n')
        self.licences = Path('staged')
        packaging.stage(self.licences)
        self.binary = b'\x7fELF synthetic release __TAURI_BUNDLE_TYPE_VAR_DEB end'

    def package_tree(self):
        root = Path('package')
        (root / 'usr/bin').mkdir(parents=True, exist_ok=True)
        (root / 'usr/bin/melora').write_bytes(self.binary)
        (root / 'usr/share/applications').mkdir(parents=True, exist_ok=True)
        (root / 'usr/share/applications/Melora.desktop').write_text(DESKTOP)
        for icon in packaging.ICONS:
            (root / icon).parent.mkdir(parents=True, exist_ok=True)
            (root / icon).write_bytes(b'synthetic png')
        for path in self.licences.rglob('*'):
            if path.is_file():
                target = root / packaging.LICENCES / path.relative_to(self.licences)
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(path.read_bytes())
        return root

    def test_stage_writes_the_notice_inventory(self):
        staged = sorted(p.relative_to(self.licences).as_posix() for p in self.licences.rglob('*') if p.is_file())
        self.assertEqual(staged, ['THIRD_PARTY_NOTICES.md', 'dependencies/LICENSE.txt',
                                  'dependencies/dependency-licences.json',
                                  *sorted(f'fonts/{name}' for name in packaging.windows.FONT_LICENCES)])

    def test_stage_rejects_an_altered_notice(self):
        Path('artifacts/dependency-notices/LICENSE.txt').write_text('Altered\n')
        with self.assertRaises(ValueError):
            packaging.stage(self.licences)

    def test_exact_package_passes(self):
        root = self.package_tree()
        count = packaging.audit_tree(root, self.binary, self.licences, 'deb')
        self.assertEqual(count, 2 + len(packaging.ICONS) + 6)

    def test_private_and_account_files_are_refused(self):
        for name in ('HANDOFF.md', 'spotify.json', 'playlist-order.json'):
            with self.subTest(name=name):
                root = self.package_tree()
                (root / 'usr/share' / name).write_text('{}')
                with self.assertRaisesRegex(packaging.AuditError, 'private or runtime file'):
                    packaging.audit_tree(root, self.binary, self.licences, 'appimage')
                (root / 'usr/share' / name).unlink()

    def test_extra_files_fail_the_exact_inventory(self):
        root = self.package_tree()
        (root / 'usr/share/extra.txt').write_text('unexpected')
        with self.assertRaisesRegex(packaging.AuditError, 'unexpected package files'):
            packaging.audit_tree(root, self.binary, self.licences, 'deb')

    def test_a_changed_binary_or_licence_fails(self):
        root = self.package_tree()
        (root / 'usr/bin/melora').write_bytes(b'\x7fELF other')
        with self.assertRaisesRegex(packaging.AuditError, 'differs from the build'):
            packaging.audit_tree(root, self.binary, self.licences, 'deb')
        (root / 'usr/bin/melora').write_bytes(self.binary)
        (root / packaging.LICENCES / 'THIRD_PARTY_NOTICES.md').write_text('Altered\n')
        with self.assertRaisesRegex(packaging.AuditError, 'licence bytes differ'):
            packaging.audit_tree(root, self.binary, self.licences, 'deb')

    def test_only_the_package_type_marker_may_differ(self):
        build = self.binary.replace(b'VAR_DEB', b'VAR_APP')
        self.assertTrue(packaging.same_but_marker(self.binary, build, 'deb'))
        self.assertTrue(packaging.same_but_marker(build, build, 'appimage'))
        self.assertFalse(packaging.same_but_marker(self.binary, build, 'rpm'))
        self.assertFalse(packaging.same_but_marker(self.binary.replace(b'end', b'END'), build, 'deb'))
        # Large binaries, with the marker far from the start, and a second difference after it.
        filler = bytes(range(256)) * 4096
        big_build, big = filler + build + filler, filler + self.binary + filler
        self.assertTrue(packaging.same_but_marker(big, big_build, 'deb'))
        self.assertFalse(packaging.same_but_marker(big[:-1] + b'!', big_build, 'deb'))
        self.assertFalse(packaging.same_but_marker(b'!' + big[1:], big_build, 'deb'))

    def test_runtime_files_never_ship_from_any_list(self):
        for name in ('spotify.json', 'playlist-order.json', 'preferences.json'):
            self.assertIn(name, packaging.gate.RUNTIME_FILES)
            self.assertIn(name, packaging.windows.NEVER_SHIP)
            self.assertIn(name, packaging.PRIVATE)

    def test_a_wrong_desktop_entry_fails(self):
        root = self.package_tree()
        (root / 'usr/share/applications/Melora.desktop').write_text(DESKTOP.replace('Exec=melora', 'Exec=other'))
        with self.assertRaisesRegex(packaging.AuditError, 'Melora.desktop is wrong'):
            packaging.audit_tree(root, self.binary, self.licences, 'deb')


if __name__ == '__main__':
    unittest.main()
