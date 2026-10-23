import json
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]


class VersionTests(unittest.TestCase):
    """Melora's version is set in several files; a bump must change them together."""

    def test_app_version_matches_everywhere(self):
        version = json.loads((ROOT / 'package.json').read_text())['version']
        self.assertRegex(version, r'^\d+\.\d+\.\d+$')
        lock = json.loads((ROOT / 'package-lock.json').read_text())
        self.assertEqual(lock['version'], version)
        self.assertEqual(lock['packages']['']['version'], version)
        cargo = (ROOT / 'src-tauri/Cargo.toml').read_text()
        self.assertIn(f'name = "melora"\nversion = "{version}"', cargo)
        cargo_lock = (ROOT / 'Cargo.lock').read_text()
        self.assertIn(f'name = "melora"\nversion = "{version}"', cargo_lock)
        client = (ROOT / 'src/native/client.ts').read_text()
        self.assertEqual(re.findall(r"version: '([^']+)'", client), [version])


if __name__ == '__main__':
    unittest.main()
