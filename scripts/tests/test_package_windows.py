import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
import zipfile

SPEC = importlib.util.spec_from_file_location('package_windows', Path(__file__).resolve().parents[1] / 'package-windows.py')
packaging = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(packaging)


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.previous = Path.cwd()
        os.chdir(self.temp.name)
        self.addCleanup(os.chdir, self.previous)
        self.root = Path('artifacts/dependency-notices')
        self.root.mkdir(parents=True)
        (self.root / 'dependency-licences.json').write_text('[]\n')
        (self.root / 'LICENSE.txt').write_text('Synthetic test notice\n')
        self.manifest = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in self.root.iterdir()}
        Path('artifacts/dependency-notices-manifest.json').write_text(json.dumps(self.manifest))
        fonts = Path('public/fonts/licences')
        fonts.mkdir(parents=True)
        for name in packaging.FONT_LICENCES:
            (fonts / name).write_text('Synthetic font notice\n')
        Path('THIRD_PARTY_NOTICES.md').write_text('Synthetic runtime notices\n')
        executable = bytearray(128)
        executable[:2] = b'MZ'
        executable[60:64] = (64).to_bytes(4, 'little')
        executable[64:70] = b'PE\0\0\x64\x86'
        self.exe = Path('synthetic.exe')
        self.exe.write_bytes(executable)

    def test_exact_inventory_and_bytes_are_packaged(self):
        packaging.package(self.exe, '0.1.0')
        archive_path = Path('artifacts/Melora-0.1.0-windows-x64.zip')
        with zipfile.ZipFile(archive_path) as archive:
            self.assertIsNone(archive.testzip())
            self.assertEqual(archive.read('Melora.exe'), self.exe.read_bytes())
            self.assertEqual(archive.read('data/'), b'')
            self.assertEqual(len(archive.namelist()), 9)
        digest = hashlib.sha256(archive_path.read_bytes()).hexdigest()
        self.assertTrue(archive_path.with_suffix('.zip.sha256').read_text().startswith(digest))

    def test_unexpected_and_modified_notices_fail_before_replacing_release(self):
        release = Path('artifacts/Melora-0.1.0-windows-x64.zip')
        release.write_bytes(b'previous synthetic release')
        extra = self.root / 'unexpected.txt'
        extra.write_text('Synthetic excluded input')
        with self.assertRaises(ValueError):
            packaging.package(self.exe, '0.1.0')
        extra.unlink()
        (self.root / 'LICENSE.txt').write_text('Modified test notice')
        with self.assertRaises(ValueError):
            packaging.package(self.exe, '0.1.0')
        self.assertEqual(release.read_bytes(), b'previous synthetic release')

    def test_missing_notice_and_wrong_architecture_fail(self):
        (self.root / 'LICENSE.txt').unlink()
        with self.assertRaises(ValueError):
            packaging.package(self.exe, '0.1.0')
        self.exe.write_bytes(b'MZ' + bytes(126))
        with self.assertRaises(ValueError):
            packaging.package(self.exe, '0.1.0')

    def test_inventory_path_traversal_is_rejected(self):
        self.manifest['../outside.txt'] = '0' * 64
        Path('artifacts/dependency-notices-manifest.json').write_text(json.dumps(self.manifest))
        with self.assertRaises(ValueError):
            packaging.notice_files()

    @unittest.skipUnless(hasattr(os, 'symlink') and os.name != 'nt', 'Requires local symlink support')
    def test_notice_symlink_is_rejected(self):
        notice = self.root / 'LICENSE.txt'
        notice.unlink()
        Path('outside.txt').write_text('Synthetic outside file')
        notice.symlink_to(Path('../../outside.txt'))
        with self.assertRaises(ValueError):
            packaging.notice_files()
