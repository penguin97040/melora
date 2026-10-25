"""Collect declared runtime licences and original notices without host metadata."""
import json
import hashlib
from pathlib import Path
import shutil
import subprocess
import sys

# The Rust target whose runtime crates ship (the Windows ZIP by default).
target = sys.argv[1] if len(sys.argv) > 1 else 'x86_64-pc-windows-msvc'

output = Path('artifacts/dependency-notices')
if output.exists():
    shutil.rmtree(output)
output.mkdir(parents=True)
report = []
for name in ['react', 'react-dom', 'scheduler', '@tauri-apps/api']:
    root = Path('node_modules') / name
    metadata = json.loads((root / 'package.json').read_text())
    slug = name.replace('@', '').replace('/', '-') + '-' + metadata['version']
    destination = output / slug
    destination.mkdir()
    notices = [p for p in root.iterdir() if p.is_file() and p.name.upper().startswith(('LICENSE', 'LICENCE', 'COPYING', 'NOTICE'))]
    if not notices:
        raise SystemExit(f'Missing runtime licence notice for {name}')
    for notice in notices:
        shutil.copyfile(notice, destination / notice.name)
    report.append({'name': name, 'version': metadata['version'], 'licence': metadata.get('license')})

result = subprocess.run(['cargo', 'metadata', '--manifest-path', 'Cargo.toml', '--locked', '--format-version', '1', '--filter-platform', target], check=True, capture_output=True, text=True)
metadata = json.loads(result.stdout)
nodes = {node['id']: node for node in metadata['resolve']['nodes']}
# A virtual workspace has no resolve root, so start from the shipped application package.
packages = {package['id']: package for package in metadata['packages']}
workspace = set(metadata['workspace_members'])
roots = [package_id for package_id in workspace if packages[package_id]['name'] == 'melora']
if len(roots) != 1:
    raise SystemExit('Cannot locate the melora package in the Cargo workspace')
root_id = roots[0]
visited = {root_id}
pending = [root_id]
while pending:
    current = pending.pop()
    for dep in nodes[current]['deps']:
        if any(kind['kind'] is None for kind in dep['dep_kinds']) and dep['pkg'] not in visited:
            visited.add(dep['pkg'])
            pending.append(dep['pkg'])
missing = []
for package in metadata['packages']:
    # Workspace members (melora and the ora crates) are this project's own code, not dependencies.
    if package['id'] not in visited or package['id'] in workspace:
        continue
    root = Path(package['manifest_path']).parent
    slug = package['name'] + '-' + package['version']
    destination = output / slug
    destination.mkdir(exist_ok=True)
    notices = [p for p in root.iterdir() if p.is_file() and p.name.upper().startswith(('LICENSE', 'LICENCE', 'COPYING', 'NOTICE', 'COPYRIGHT'))]
    if package.get('license_file'):
        explicit = root / package['license_file']
        if explicit.is_file() and explicit not in notices:
            notices.append(explicit)
    # Some crates keep notices in a dedicated directory.
    for folder in ('licenses', 'licences'):
        if (root / folder).is_dir():
            shutil.copytree(root / folder, destination / folder, dirs_exist_ok=True)
            notices.append(root / folder)
    for notice in notices:
        if notice.is_file():
            shutil.copyfile(notice, destination / notice.name)
    if not notices:
        sources = json.loads(Path('third-party/notices/sources.json').read_text())
        for source in sources:
            if source['package'] != slug:
                continue
            supplied = Path('third-party/notices') / slug / source['file']
            if hashlib.sha256(supplied.read_bytes()).hexdigest() != source['sha256']:
                raise SystemExit('Upstream licence checksum mismatch')
            shutil.copyfile(supplied, destination / supplied.name)
            notices.append(supplied)
        if not notices:
            missing.append(slug)
    report.append({'name': package['name'], 'version': package['version'], 'licence': package['license'], 'source': f"https://crates.io/crates/{package['name']}/{package['version']}"})
(output / 'dependency-licences.json').write_text(json.dumps(sorted(report, key=lambda p: p['name']), indent=2) + '\n')
if missing:
    (output / 'missing-original-notices.json').write_text(json.dumps(missing, indent=2) + '\n')
    raise SystemExit('Missing original runtime notices; review artifacts/dependency-notices/missing-original-notices.json')
print(f'Collected original notices and declared licences for {len(report)} runtime dependencies.')

# Packaging must use this exact inventory, never a fresh recursive allowlist.
files = {}
for file in sorted(output.rglob('*')):
    if file.is_symlink():
        raise SystemExit('Dependency notices must not contain symlinks')
    if file.is_file():
        files[file.relative_to(output).as_posix()] = hashlib.sha256(file.read_bytes()).hexdigest()
Path('artifacts/dependency-notices-manifest.json').write_text(json.dumps(files, indent=2) + '\n')
