"""Package only reviewed runtime files, never workspace contents."""
import hashlib
from pathlib import Path
import re
import sys
import zipfile

exe = Path(sys.argv[1])
version = sys.argv[2]
if not re.fullmatch(r'\d+\.\d+\.\d+', version):
    raise SystemExit('Invalid release version')
if not exe.is_file() or exe.read_bytes()[:2] != b'MZ':
    raise SystemExit('Missing Windows release executable')
output = Path('artifacts') / f'Melora-{version}-windows-x64.zip'
output.parent.mkdir(exist_ok=True)
notice = '''Melora portable preview

Extract the complete folder to a writable location and run Melora.exe.
Windows x64 and the system Microsoft Edge WebView2 Runtime are required.
No installer or Chromium runtime is included. This build is unsigned.
Preferences are stored in data/preferences.json beside Melora.exe.
Change display settings, close, relaunch and confirm they are restored.
If data cannot be written, move the extracted folder to a writable location.
Synthetic preview only: no Spotify authentication or audio is implemented.
Future Spotify credentials will use Windows Credential Manager; moving this
folder to another computer will require signing in again.
This is a compilation-tested build; Windows launch still needs manual testing.
'''
with zipfile.ZipFile(output, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
    archive.write(exe, 'Melora.exe')
    # The linked WebView2 loader is static; OS WebView2 and OS libraries remain prerequisites.
    archive.writestr('data/', '')
    archive.writestr('README.txt', notice)
    for file in sorted(Path('public/fonts/licences').glob('*.txt')):
        archive.write(file, f'licences/fonts/{file.name}')
    archive.write('THIRD_PARTY_NOTICES.md', 'licences/THIRD_PARTY_NOTICES.md')
    for file in sorted(Path('artifacts/dependency-notices').rglob('*')):
        if file.is_file():
            archive.write(file, 'licences/dependencies/' + file.relative_to('artifacts/dependency-notices').as_posix())
with zipfile.ZipFile(output) as archive:
    allowed = {'Melora.exe', 'data/', 'README.txt', 'licences/THIRD_PARTY_NOTICES.md'}
    allowed.update(f'licences/fonts/{p.name}' for p in Path('public/fonts/licences').glob('*.txt'))
    allowed.update('licences/dependencies/' + p.relative_to('artifacts/dependency-notices').as_posix() for p in Path('artifacts/dependency-notices').rglob('*') if p.is_file())
    if set(archive.namelist()) != allowed:
        raise SystemExit('Unexpected ZIP contents')
digest = hashlib.sha256(output.read_bytes()).hexdigest()
output.with_suffix('.zip.sha256').write_text(f'{digest}  {output.name}\n')
print(f'Portable ZIP: {output.resolve()}')
print(f'SHA-256: {digest}')
