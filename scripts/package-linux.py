"""Stage the Linux packages' licences, then audit the packages Tauri built from them.

  package-linux.py stage <licences dir>
  package-linux.py audit <arch> <version> <release dir> <licences dir> <output dir>

The audit runs in the build container (it needs dpkg-deb, rpm, cpio and readelf).
Diagnostics name files and checks, never file contents.
"""
import fnmatch
import gzip
import hashlib
import importlib.util
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

SCRIPTS = Path(__file__).resolve().parent


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / file)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


windows = load('package_windows', 'package-windows.py')
gate = load('publication_gate', 'publication_gate.py')

# Packages carry no private docs and no files the app writes at runtime.
# (`gate.PRIVATE` includes `gate.RUNTIME_FILES`.)
PRIVATE = tuple(p for p in gate.PRIVATE if not p.startswith('*.'))
ARCHES = {
    'x86_64': {'deb': 'amd64', 'machine': 'Advanced Micro Devices X86-64',
               'interpreter': '/lib64/ld-linux-x86-64.so.2', 'appimage': True},
    'aarch64': {'deb': 'arm64', 'machine': 'AArch64',
                'interpreter': '/lib/ld-linux-aarch64.so.1', 'appimage': False},
}
# Ubuntu 22.04's glibc: the packages must install on it and on anything newer.
GLIBC = (2, 35)
# Fedora Asahi Remix uses 16 KiB pages, so every segment must be aligned to at least that.
PAGE = 0x4000
NEEDED = {
    'libwebkit2gtk-4.1.so.0', 'libgtk-3.so.0', 'libgdk-3.so.0', 'libjavascriptcoregtk-4.1.so.0',
    'libsoup-3.0.so.0', 'libcairo.so.2', 'libgdk_pixbuf-2.0.so.0', 'libgio-2.0.so.0',
    'libgobject-2.0.so.0', 'libglib-2.0.so.0', 'libgcc_s.so.1', 'libm.so.6', 'libc.so.6',
    'ld-linux-x86-64.so.2', 'ld-linux-aarch64.so.1',
    # tao's D-Bus client; WebKitGTK depends on the same library.
    'libdbus-1.so.3',
}
DEPENDS = 'ca-certificates, libwebkit2gtk-4.1-0, libgtk-3-0'
REQUIRES = {'ca-certificates', 'libwebkit2gtk-4.1.so.0()(64bit)', 'libgtk-3.so.0()(64bit)'}
DESKTOP = {
    'Type': 'Application', 'Name': 'Melora', 'Exec': 'melora', 'Icon': 'melora',
    'StartupWMClass': 'melora', 'Terminal': 'false', 'Categories': 'AudioVideo;Audio;Music;',
}
ICONS = {
    'usr/share/icons/hicolor/32x32/apps/melora.png',
    'usr/share/icons/hicolor/64x64/apps/melora.png',
    'usr/share/icons/hicolor/128x128/apps/melora.png',
    'usr/share/icons/hicolor/512x512/apps/melora.png',
}
LICENCES = 'usr/share/doc/melora/licences'
# Tauri writes the package type into each package's copy of the binary; nothing else may differ.
MARKER = re.compile(rb'__TAURI_BUNDLE_TYPE_VAR_[A-Z]{3}')
MARKERS = {'deb': b'__TAURI_BUNDLE_TYPE_VAR_DEB', 'rpm': b'__TAURI_BUNDLE_TYPE_VAR_RPM',
           'appimage': b'__TAURI_BUNDLE_TYPE_VAR_APP'}


class AuditError(Exception):
    pass


def check(condition, message):
    if not condition:
        raise AuditError(message)


def stage(output):
    contents = {'THIRD_PARTY_NOTICES.md': Path('THIRD_PARTY_NOTICES.md')}
    contents.update((f'fonts/{name}', Path('public/fonts/licences') / name) for name in windows.FONT_LICENCES)
    data = {}
    for name, file in contents.items():
        if file.is_symlink() or not file.is_file():
            raise AuditError('Runtime input must be a regular file')
        data[name] = file.read_bytes()
    data.update(('dependencies/' + relative, value) for relative, value in windows.notice_files().items())
    if output.exists():
        shutil.rmtree(output)
    for name, value in data.items():
        (output / name).parent.mkdir(parents=True, exist_ok=True)
        (output / name).write_bytes(value)
    print(f'Staged {len(data)} licence files.')


def files(root):
    found = {}
    for path in sorted(root.rglob('*')):
        if path.is_symlink() or path.is_file():
            found[path.relative_to(root).as_posix()] = path
    return found


def tool(*args, stdin=None):
    result = subprocess.run(args, stdin=stdin, capture_output=True)
    if result.returncode:
        raise AuditError(f'{args[0]} failed')
    return result.stdout.decode()


def rpm_payload(data):
    """The cpio archive inside an rpm: Ubuntu 22.04's rpm2cpio can't read the rpm crate's packages."""
    offset = 96  # the lead
    for padded in (True, False):  # the signature header (padded to 8 bytes), then the main header
        check(data[offset:offset + 4] == b'\x8e\xad\xe8\x01', 'rpm header is malformed')
        count, size = int.from_bytes(data[offset + 8:offset + 12], 'big'), int.from_bytes(data[offset + 12:offset + 16], 'big')
        offset += 16 + 16 * count + size
        if padded:
            offset += -offset % 8
    check(data[offset:offset + 2] == b'\x1f\x8b', 'rpm payload is not gzip')
    return gzip.decompress(data[offset:])


def extract(kind, package, into):
    if kind == 'deb':
        tool('dpkg-deb', '-x', str(package), str(into))
        return into
    if kind == 'rpm':
        subprocess.run(['cpio', '-idm', '--quiet', '--no-absolute-filenames'], input=rpm_payload(package.read_bytes()),
                       cwd=into, check=True)
        return into
    subprocess.run([str(package.resolve()), '--appimage-extract'], cwd=into, capture_output=True, check=True)
    return into / 'squashfs-root'


def audit_binary(arch, binary):
    spec = ARCHES[arch]
    header = tool('readelf', '-W', '-h', str(binary))
    check('ELF64' in header and spec['machine'] in header, 'melora is not a 64-bit binary for this architecture')
    segments = tool('readelf', '-W', '-l', str(binary))
    check(f'[Requesting program interpreter: {spec["interpreter"]}]' in segments, 'melora has the wrong interpreter')
    alignments = [int(line.split()[-1], 16) for line in segments.splitlines() if line.strip().startswith('LOAD')]
    check(alignments and min(alignments) >= (PAGE if arch == 'aarch64' else 0x1000), 'melora segments are not page aligned')
    dynamic = tool('readelf', '-W', '-d', str(binary))
    needed = set(re.findall(r'\(NEEDED\)\s+Shared library: \[([^\]]+)\]', dynamic))
    check(needed <= NEEDED, f'melora links unreviewed libraries: {sorted(needed - NEEDED)}')
    versions = tool('readelf', '-W', '-V', str(binary))
    glibc = max(tuple(int(part) for part in v.split('.')) for v in re.findall(r'Name: GLIBC_([0-9.]+)', versions))
    check(glibc <= GLIBC, f'melora needs glibc {".".join(map(str, glibc))}')
    data = binary.read_bytes().decode('latin1')
    check(not gate.RULES['credential'].search(data), 'melora contains a credential pattern')
    return sorted(needed), '.'.join(map(str, glibc))


def first_difference(a, b, step=1 << 16):
    """Where two different byte strings of the same length first differ, a block at a time."""
    at = 0
    while a[at:at + step] == b[at:at + step]:
        at += step
    return next(i for i in range(at, at + step) if a[i] != b[i])


def same_but_marker(packaged, build, kind):
    """A package's binary is the build except, at most, one bundle type marker set to that package's type."""
    if len(packaged) != len(build):
        return False
    if packaged == build:
        return True
    first = first_difference(packaged, build)
    size = len(MARKERS[kind])
    start = packaged.find(MARKERS[kind], max(0, first - size + 1), first + size)
    end = start + size
    # Every difference lies inside that one marker: the bytes before it are equal by definition.
    return (0 <= start <= first and packaged[end:] == build[end:]
            and MARKER.fullmatch(build[start:end]) is not None)


def sections(data):
    """An ELF64 little-endian file's sections, by name."""
    check(data[:6] == b'\x7fELF\x02\x01', 'not a 64-bit little-endian ELF file')
    offset, size, count, names = (int.from_bytes(data[a:b], 'little') for a, b in ((0x28, 0x30), (0x3A, 0x3C), (0x3C, 0x3E), (0x3E, 0x40)))
    headers = [data[offset + i * size:offset + (i + 1) * size] for i in range(count)]
    def body(header):
        start, length = int.from_bytes(header[0x18:0x20], 'little'), int.from_bytes(header[0x20:0x28], 'little')
        return data[start:start + length]
    table = body(headers[names])
    named = {}
    for header in headers:
        start = int.from_bytes(header[:4], 'little')
        named[table[start:table.index(b'\0', start)].decode()] = body(header)
    return named


# linuxdeploy rewrites the AppImage's binary (its RPATH points at the bundled libraries), so
# there the program's own code and data must match instead of the whole file.
PROGRAM = ('.text', '.rodata', '.data.rel.ro', '.data')


def same_program(packaged, build, kind):
    mine, theirs = sections(packaged), sections(build)
    if not all(name in mine and name in theirs for name in PROGRAM):
        return False
    return all(same_but_marker(mine[name], theirs[name], kind) for name in PROGRAM)


def audit_tree(root, binary, licences, kind):
    found = files(root)
    for name in found:
        parts = Path(name.lower()).parts
        check(not any(fnmatch.fnmatch(part, pattern) for part in parts for pattern in PRIVATE),
              f'private or runtime file packaged: {name}')
    same = same_but_marker if kind != 'appimage' else same_program
    check(found.get('usr/bin/melora') and same(found['usr/bin/melora'].read_bytes(), binary, kind),
          'usr/bin/melora differs from the build')
    staged = files(licences)
    packaged = {name[len(LICENCES) + 1:]: path for name, path in found.items() if name.startswith(LICENCES + '/')}
    check(set(packaged) == set(staged), 'packaged licences differ from the staged inventory')
    check(all(packaged[name].read_bytes() == staged[name].read_bytes() for name in staged), 'packaged licence bytes differ')
    desktop = found.get('usr/share/applications/Melora.desktop')
    check(desktop is not None, 'Melora.desktop is missing')
    lines = desktop.read_text().splitlines()
    check(lines[:1] == ['[Desktop Entry]'], 'Melora.desktop has no [Desktop Entry] header')
    entries = dict(line.split('=', 1) for line in lines[1:] if line.strip())
    check(entries.get('Comment') and {k: v for k, v in entries.items() if k != 'Comment'} == DESKTOP,
          f'Melora.desktop is wrong: {sorted(entries)}')
    if kind != 'appimage':
        expected = {'usr/bin/melora', 'usr/share/applications/Melora.desktop'} | ICONS
        expected |= {f'{LICENCES}/{name}' for name in staged}
        check(set(found) == expected, f'unexpected package files: {sorted(set(found) ^ expected)}')
        for name, path in found.items():
            if not name.startswith(LICENCES + '/') and name != 'usr/bin/melora':
                check(not gate.RULES['credential'].search(path.read_bytes().decode('latin1')), f'credential pattern in {name}')
    return len(found)


def audit(arch, version, release, licences, output):
    spec = ARCHES[arch]
    binary = release / 'melora'
    needed, glibc = audit_binary(arch, binary)
    bundle = release / 'bundle'
    packages = {
        'deb': bundle / 'deb' / f'Melora_{version}_{spec["deb"]}.deb',
        'rpm': bundle / 'rpm' / f'Melora-{version}-1.{arch}.rpm',
    }
    if spec['appimage']:
        packages['appimage'] = bundle / 'appimage' / f'Melora_{version}_{spec["deb"]}.AppImage'
    report = []
    for kind, package in packages.items():
        check(package.is_file() and not package.is_symlink(), f'{package.name} was not built')
        if kind == 'deb':
            fields = tool('dpkg-deb', '-f', str(package), 'Package', 'Version', 'Architecture', 'Depends')
            check(fields.splitlines() == ['Package: melora', f'Version: {version}', f'Architecture: {spec["deb"]}',
                                          f'Depends: {DEPENDS}'], f'{package.name} control fields are wrong')
        if kind == 'rpm':
            fields = tool('rpm', '-qp', '--qf', '%{NAME} %{VERSION} %{RELEASE} %{ARCH}', str(package))
            check(fields == f'melora {version} 1 {arch}', f'{package.name} header is wrong')
            requires = {line for line in tool('rpm', '-qp', '--requires', str(package)).splitlines()
                        if not line.startswith('rpmlib(')}
            check(requires == REQUIRES, f'{package.name} requires {sorted(requires)}')
        with tempfile.TemporaryDirectory() as directory:
            count = audit_tree(extract(kind, package, Path(directory)), binary.read_bytes(), licences, kind)
        report.append((package, count))

    output.mkdir(parents=True, exist_ok=True)
    for old in output.iterdir():
        stale = old.name.startswith('Melora') and (f'_{spec["deb"]}.' in old.name or f'.{arch}.rpm' in old.name)
        if stale:
            old.unlink()
    for package, count in report:
        shutil.copyfile(package, output / package.name)
        digest = hashlib.sha256(package.read_bytes()).hexdigest()
        (output / f'{package.name}.sha256').write_text(f'{digest}  {package.name}\n')
        print(f'{package.name}: {package.stat().st_size} bytes, {count} files, SHA-256 {digest}')
    print(f'melora ({arch}): glibc {glibc}, links {", ".join(needed)}')


def main():
    try:
        if sys.argv[1:2] == ['stage'] and len(sys.argv) == 3:
            stage(Path(sys.argv[2]))
        elif sys.argv[1:2] == ['audit'] and len(sys.argv) == 7 and sys.argv[2] in ARCHES:
            audit(sys.argv[2], sys.argv[3], Path(sys.argv[4]), Path(sys.argv[5]), Path(sys.argv[6]))
        else:
            raise AuditError(__doc__.strip().splitlines()[2])
    except (AuditError, ValueError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(f'Linux packaging failed: {error}') from None


if __name__ == '__main__':
    main()
