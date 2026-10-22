"""Package an exact, integrity-checked inventory of reviewed runtime files."""
import hashlib
import json
from pathlib import Path
import re
import sys
import tempfile
import zipfile

FONT_LICENCES = ('Chakra-Petch.txt', 'Share-Tech-Mono.txt', 'Ubuntu-Mono.txt')
NOTICE = '''Melora portable preview

Extract the complete folder to a writable location and run Melora.exe.
Windows x64 and the system Microsoft Edge WebView2 Runtime are required.
No installer or Chromium runtime is included. This build is unsigned.
Preferences are stored in data/preferences.json beside Melora.exe, and your
Spotify Client ID in data/spotify.json. Neither file holds a password or token.
If data cannot be written, move the extracted folder to a writable location.
Spotify sign-in keeps its refresh token only in Windows Credential Manager;
moving this folder to another computer requires signing in again. There is
no audio yet: the library and player still show labelled synthetic content.
Melora is unofficial and not affiliated with Spotify.
This is a compilation-tested build; Windows launch still needs manual testing.
'''
# Files the owner's copy writes at runtime. They must never be packaged.
NEVER_SHIP = {'spotify.json', 'preferences.json'}


def notice_files():
    root = Path('artifacts/dependency-notices')
    manifest = json.loads(Path('artifacts/dependency-notices-manifest.json').read_text())
    if not isinstance(manifest, dict) or 'dependency-licences.json' not in manifest:
        raise ValueError('Missing dependency notice inventory')
    files = {}
    for relative, digest in manifest.items():
        path = Path(relative)
        if path.is_absolute() or '..' in path.parts or '\\' in relative or not re.fullmatch(r'[0-9a-f]{64}', digest):
            raise ValueError('Invalid dependency notice inventory')
        files[relative] = root / path
    actual = set()
    if root.is_symlink():
        raise ValueError('Dependency notice directory must not be a symlink')
    for file in root.rglob('*'):
        if file.is_symlink():
            raise ValueError('Dependency notices must not contain symlinks')
        if file.is_file():
            actual.add(file.relative_to(root).as_posix())
    if actual != set(files):
        raise ValueError('Dependency notice inventory differs from collected files')
    contents = {}
    for relative, file in files.items():
        data = file.read_bytes()
        if hashlib.sha256(data).hexdigest() != manifest[relative]:
            raise ValueError('Dependency notice checksum mismatch')
        contents[relative] = data
    return contents


def package(exe, version):
    if not re.fullmatch(r'\d+\.\d+\.\d+', version):
        raise ValueError('Invalid release version')
    executable = exe.read_bytes()
    if exe.is_symlink() or executable[:2] != b'MZ' or len(executable) < 64:
        raise ValueError('Missing Windows release executable')
    offset = int.from_bytes(executable[60:64], 'little')
    if executable[offset:offset + 6] != b'PE\0\0\x64\x86':
        raise ValueError('Release executable must be Windows x64')
    files = {'licences/THIRD_PARTY_NOTICES.md': Path('THIRD_PARTY_NOTICES.md')}
    files.update((f'licences/fonts/{name}', Path('public/fonts/licences') / name) for name in FONT_LICENCES)
    dependencies = notice_files()
    # Capture bytes after validation so the archive and post-write audit agree.
    contents = {'Melora.exe': executable}
    contents.update(('licences/dependencies/' + relative, data) for relative, data in dependencies.items())
    for name, file in files.items():
        if file.is_symlink() or not file.is_file():
            raise ValueError('Runtime input must be a regular file')
        contents[name] = file.read_bytes()
    contents.update({'data/': b'', 'README.txt': NOTICE.encode()})
    if any(Path(name).name.lower() in NEVER_SHIP or (name.startswith('data/') and name != 'data/') for name in contents):
        raise ValueError('Account or settings data must not be packaged')
    output = Path('artifacts') / f'Melora-{version}-windows-x64.zip'
    with tempfile.TemporaryDirectory(prefix='melora-package-', dir=output.parent) as directory:
        temporary = Path(directory) / 'release.zip'
        with zipfile.ZipFile(temporary, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
            for name, data in sorted(contents.items()):
                archive.writestr(name, data)
        with zipfile.ZipFile(temporary) as archive:
            if archive.testzip() is not None or set(archive.namelist()) != set(contents):
                raise ValueError('ZIP integrity or inventory check failed')
            if any(archive.read(name) != data for name, data in contents.items()):
                raise ValueError('ZIP runtime bytes differ from reviewed inputs')
        temporary.replace(output)
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    output.with_suffix('.zip.sha256').write_text(f'{digest}  {output.name}\n')
    print(f'Portable ZIP: {output.resolve()}')
    print(f'SHA-256: {digest}')


if __name__ == '__main__':
    try:
        package(Path(sys.argv[1]), sys.argv[2])
    except (OSError, ValueError, TypeError, IndexError):
        raise SystemExit('Portable packaging failed: check executable, notices and inventory') from None
