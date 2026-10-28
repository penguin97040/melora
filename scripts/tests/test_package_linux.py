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
        Path('LICENSE').write_text('Synthetic Melora licence\n')
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
        self.assertEqual(staged, ['LICENSE', 'THIRD_PARTY_NOTICES.md', 'dependencies/LICENSE.txt',
                                  'dependencies/dependency-licences.json',
                                  *sorted(f'fonts/{name}' for name in packaging.windows.FONT_LICENCES)])

    def test_stage_rejects_an_altered_notice(self):
        Path('artifacts/dependency-notices/LICENSE.txt').write_text('Altered\n')
        with self.assertRaises(ValueError):
            packaging.stage(self.licences)

    def test_exact_package_passes(self):
        root = self.package_tree()
        count = packaging.audit_tree(root, self.binary, self.licences, 'deb')
        self.assertEqual(count, 2 + len(packaging.ICONS) + 7)

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

    def appimage_tree(self):
        """A package tree as an AppImage: its own parts, two bundled libraries and their notices."""
        root = self.package_tree()
        for part in packaging.APPIMAGE_PARTS:
            (root / part).parent.mkdir(parents=True, exist_ok=True)
            (root / part).write_text('synthetic part')
        (root / 'usr/lib').mkdir(parents=True)
        (root / 'usr/lib/libgtk-3.so.0').write_bytes(b'\x7fELF gtk')
        (root / 'usr/lib/gio/modules').mkdir(parents=True)
        (root / 'usr/lib/gio/modules/libgiognutls.so').write_bytes(b'\x7fELF gio')
        (root / 'usr/lib/gio/modules/giomodule.cache').write_text('generated')
        for package in ('libgtk-3-0', 'glib-networking'):
            (root / 'usr/share/doc' / package).mkdir(parents=True)
            (root / 'usr/share/doc' / package / 'copyright').write_text('synthetic copyright')
        notices = Path('appimage-notices/appimage')
        for slug in {*packaging.APPIMAGE_PARTS.values(), *packaging.RUNTIME_NOTICES}:
            for target in (notices / slug, root / packaging.APPIMAGE_NOTICES / slug):
                target.mkdir(parents=True, exist_ok=True)
                (target / 'LICENSE').write_text(f'{slug} licence')
        return root

    @staticmethod
    def owners(names):
        known = {'libgtk-3.so.0': {'libgtk-3-0'}, 'libgiognutls.so': {'glib-networking'}}
        return {name: known.get(name, set()) for name in names}

    def test_the_appimage_has_a_notice_for_everything_it_bundles(self):
        root = self.appimage_tree()
        found = packaging.files(root)
        packaging.audit_appimage(found, Path('appimage-notices'), self.owners)
        # A plugin-copied module whose package's copyright is missing.
        (root / 'usr/share/doc/glib-networking/copyright').unlink()
        with self.assertRaisesRegex(packaging.AuditError, 'glib-networking'):
            packaging.audit_appimage(packaging.files(root), Path('appimage-notices'), self.owners)
        (root / 'usr/share/doc/glib-networking/copyright').write_text('synthetic copyright')
        # A library no package owns can't be accounted for.
        (root / 'usr/lib/libmystery.so').write_bytes(b'\x7fELF ?')
        with self.assertRaisesRegex(packaging.AuditError, 'no package owns'):
            packaging.audit_appimage(packaging.files(root), Path('appimage-notices'), self.owners)
        (root / 'usr/lib/libmystery.so').unlink()
        # The runtime's and AppRun's notices must be there, exactly as staged.
        (root / packaging.APPIMAGE_NOTICES / 'libfuse-3.15.0' / 'LICENSE').write_text('Altered')
        with self.assertRaisesRegex(packaging.AuditError, 'AppImage notices differ'):
            packaging.audit_appimage(packaging.files(root), Path('appimage-notices'), self.owners)

    def test_appimage_notices_come_pinned(self):
        notices = Path('third-party/notices')
        entries = []
        for slug in {*packaging.APPIMAGE_PARTS.values(), *packaging.RUNTIME_NOTICES}:
            (notices / slug).mkdir(parents=True)
            (notices / slug / 'LICENSE').write_text(f'{slug} licence')
            entries.append({'package': slug, 'file': 'LICENSE', 'source': 'https://example.invalid',
                            'sha256': hashlib.sha256(f'{slug} licence'.encode()).hexdigest()})
        (notices / 'sources.json').write_text(json.dumps(entries))
        self.assertEqual(len(packaging.upstream_notices(set(packaging.RUNTIME_NOTICES))), len(packaging.RUNTIME_NOTICES))
        (notices / 'musl-1.2.5' / 'LICENSE').write_text('Altered')
        with self.assertRaisesRegex(packaging.AuditError, 'pinned SHA-256'):
            packaging.upstream_notices(set(packaging.RUNTIME_NOTICES))

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
