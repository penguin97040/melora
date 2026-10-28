#!/usr/bin/env python3
"""Local fail-closed publication audit. Diagnostics contain locations, never values."""
import argparse
import fnmatch
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

# Files the app writes at runtime (ora-core config.rs and order.rs, src-tauri preferences.rs).
# They hold a Client ID, account IDs and settings: never published, never packaged.
RUNTIME_FILES = ('spotify.json', 'playlist-order.json', 'preferences.json')
# The bundled fonts' licences (OFL texts that name their authors).
FONT_LICENCES = ('Chakra-Petch.txt', 'Share-Tech-Mono.txt', 'Ubuntu-Mono.txt')
PRIVATE = ('agents.md', 'changelog.md', 'mistakes.md', 'codex_start_here.md',
           'instructions.md', 'spotify_client_build_spec.md', 'start-melora-*.ps1',
           'foobar_ui.png', 'claude.md', 'handoff.md', 'do_next.md', 'user_instructions.md',
           'spotify_integration.md', '.private-archive', *RUNTIME_FILES, '.env', '.env.*', '*.pem', '*.key', '*.p12', '*.pfx',
           '*.mobileprovision', '*.p8', '*.crt', '*.cer', '*.der', '*.keystore', '*.jks', '*.db', '*.sqlite*', '*.exe', '*.zip')
# The only files that may contain NUL bytes; anything else with one can't be read as text.
BINARY_SUFFIXES = {'.woff2', '.png', '.ico', '.icns'}
NOTICE_SOURCES = 'third-party/notices/sources.json'
# Push sends objects as they are, so replacement refs (`git replace`) must not change what is
# scanned.
GIT_ENV = {**os.environ, 'GIT_NO_REPLACE_OBJECTS': '1'}
OUTPUTS = {'node_modules', 'target', 'dist', 'artifacts', 'data', '.xwin-cache', 'credentials', 'account-data', 'secrets'}
RULES = {
    'credential': re.compile(r'(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|sk-(?:proj-|ant-)?[A-Za-z0-9_-]{20,}|(?:AKIA|ASIA)[A-Z0-9]{16}|-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----|eyJ[A-Za-z0-9_-]{12,}\.[A-Za-z0-9_-]{12,}\.[A-Za-z0-9_-]{12,})'),
    'sensitive assignment': re.compile(r'''(?i)(?:client[_ -]?secret|access[_ -]?token|refresh[_ -]?token|api[_ -]?key|secret|token|credential|private[_ -]?key|authori[sz]ation[_ -]?code|password|code[_ -]?verifier|spotify[_ -]?client[_ -]?id)\s*["']?\s*[:=]\s*["']?([A-Za-z0-9_+/.=-]{12,})'''),
    # A Spotify Client ID is 32 lowercase hex characters, saved by the app as "clientId".
    'spotify client id': re.compile(r'''(?i)client[_ -]?id\s*["']?\s*[:=]\s*["']?([0-9a-f]{32})(?![0-9a-f])'''),
    'attribution': re.compile(r'(?i)(?:co-' r'authored-by:.*(?:bot|claude|openai|codex|anthropic)|(?:generated|written|authored|created) (?:by|with) (?:AI|ChatGPT|Claude|Codex)|(?:AI|Claude|Codex|ChatGPT) co-author)'),
    'personal email': re.compile(r'[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}'),
    'personal path': re.compile(r'(?:[A-Za-z]:[\\/](?:Users|Documents and Settings)[\\/][^\s/\\]+|' r'/' r'Users/[^\s/]+|' r'/' r'home/(?!runner\b|agent\b)[^\s/]+)'),
}
# Sequential test fixtures already in published history; no real Client ID looks like these.
SYNTHETIC_CLIENT_IDS = {'0123456789abcdef' * 2, 'fedcba9876543210' * 2}
PLACEHOLDERS = {'YOUR_SPOTIFY_CLIENT_ID', 'YOUR_CLIENT_ID', 'YOUR_API_KEY', 'PLACEHOLDER', 'your_client_id_here'}

class AuditError(Exception):
    pass


def git(*args):
    result = subprocess.run(['git', *args], stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=GIT_ENV)
    if result.returncode:
        raise AuditError('Git check incomplete')
    return result.stdout


class Blobs:
    """Reads objects through one `git cat-file --batch` process."""

    def __init__(self):
        self.process = subprocess.Popen(['git', 'cat-file', '--batch'], stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=GIT_ENV)

    def read(self, oid, kind=b'blob'):
        self.process.stdin.write(oid.encode() + b'\n')
        self.process.stdin.flush()
        header = self.process.stdout.readline().split()
        if len(header) != 3 or header[1] != kind:
            raise AuditError('Git check incomplete')
        data = self.process.stdout.read(int(header[2]))
        if len(data) != int(header[2]) or self.process.stdout.read(1) != b'\n':
            raise AuditError('Git check incomplete')
        return data

    def close(self):
        self.process.stdin.close()
        self.process.wait()


def notice_hashes(sources):
    """The upstream licence texts `sources.json` pins: their paths and SHA-256s."""
    try:
        entries = json.loads(sources)
        return {(f'third-party/notices/{e["package"]}/{e["file"]}', e['sha256']) for e in entries}
    except (TypeError, KeyError, ValueError):
        return set()


def prohibited(path):
    parts = Path(path.lower()).parts
    return any(p in OUTPUTS for p in parts) or any(
        fnmatch.fnmatch(p, pattern) for p in parts for pattern in PRIVATE
        if not (p == '.env.example' and pattern == '.env.*'))


def findings(data, location, *, licence=False, email=None):
    try:
        value = data.decode('utf-8')
    except UnicodeError:
        return [('uninspectable binary', location)]
    result = []
    for rule, pattern in RULES.items():
        for match in pattern.finditer(value):
            if rule == 'sensitive assignment' and match.group(1) in PLACEHOLDERS:
                continue
            if rule == 'spotify client id' and match.group(1).lower() in SYNTHETIC_CLIENT_IDS:
                continue
            if rule == 'personal email' and (licence or match.group() == email):
                continue
            result.append((rule, location))
            break
    return result


def audit_blob(path, data, notices=frozenset()):
    path_results = findings(path.encode(), 'redacted path ' + hashlib.sha256(path.encode()).hexdigest()[:12])
    # Tauri's standard retina icon name resembles an email address. Only its
    # filename gets this exception; its contents still receive every rule.
    if path == 'src-tauri/icons/128x128' + '@' + '2x.png':
        path_results = [result for result in path_results if result[0] != 'personal email']
    location = path_results[0][1] if path_results else path
    if prohibited(path):
        return path_results + [('prohibited private/output path', location)]
    # Only known font/icon formats may be binary. Scan ASCII secret signatures too.
    if b'\0' in data:
        if Path(path).suffix not in BINARY_SUFFIXES:
            # Text with NULs in it (UTF-16, say) would slip past every rule.
            return path_results + [('uninspectable binary', location)]
        return path_results + findings(data.decode('latin1').encode(), location)
    # Licence texts may name their authors' addresses and can't be edited; every other rule still
    # applies. Only the font licences and the upstream notices sources.json pins by SHA-256 (in
    # the same tree) count; anything else beside them, sources.json included, gets every rule.
    licence = path in {f'public/fonts/licences/{name}' for name in FONT_LICENCES} or (
        (path, hashlib.sha256(data).hexdigest()) in notices)
    return path_results + findings(data, location, licence=licence)


def policy():
    # The approval is repository-wide, so linked worktrees read it from the common Git directory.
    common = Path(git('rev-parse', '--path-format=absolute', '--git-common-dir').decode().strip())
    cfg = common / 'melora-publication.json'
    try:
        data = json.loads(cfg.read_text())
        if not isinstance(data, dict) or set(data) != {'remote', 'name', 'email'} or not all(isinstance(v, str) and v.strip() for v in data.values()):
            raise ValueError()
        if not re.fullmatch(r'\d+\+[A-Za-z0-9-]+@users\.noreply\.github\.com', data['email']):
            raise ValueError()
        return data
    except (OSError, ValueError):
        raise AuditError('Missing or invalid local destination/identity approval') from None


def audit(args):
    approved = policy()
    if git('config', 'user.name').decode().strip() != approved['name'] or git('config', 'user.email').decode().strip() != approved['email']:
        raise AuditError('Configured identity differs from local approval')
    destinations = git('remote', 'get-url', '--push', '--all', 'origin').decode().splitlines()
    if destinations != [approved['remote']]:
        raise AuditError('Push destinations differ from local approval')
    destination = destinations[0]
    if destination != approved['remote'] or (args.hook and (args.remote != 'origin' or args.url != destination)):
        raise AuditError('Push destination differs from local approval')
    if git('rev-parse', '--is-shallow-repository').strip() != b'false':
        raise AuditError('Shallow history cannot be fully inspected')
    if git('for-each-ref', 'refs/replace/'):
        raise AuditError('Replacement refs must be removed before publishing')
    refs = []
    if args.hook:
        for line in sys.stdin:
            fields = line.split()
            if len(fields) != 4 or not all(re.fullmatch(r'(?:[0-9a-f]{40}|[0-9a-f]{64})', fields[i]) for i in (1, 3)):
                raise AuditError('Invalid push ref input')
            if set(fields[1]) != {'0'}:
                refs.append(fields[1])
    else:
        head = subprocess.run(['git', 'rev-parse', '--verify', 'HEAD'], capture_output=True, env=GIT_ENV)
        if head.returncode == 0:
            refs.append(head.stdout.decode().strip())
    # Audit complete ancestry of outgoing refs, including every historical tree.
    seen = set()
    results = []
    blobs = Blobs()
    # A file's findings depend on its path, its contents and the notices pinned beside it, so
    # each combination is scanned once however many commits share it.
    checked = {}

    def scan(path, oid, notices_oid, notices):
        key = (path, oid, notices_oid)
        if key not in checked:
            checked[key] = audit_blob(path, blobs.read(oid), notices)
        return checked[key]

    def tree_entries(entries):
        for entry in entries:
            if entry:
                meta, path = entry.split(b'\t', 1)
                yield meta.split(), path.decode('utf-8')

    try:
        for ref in refs:
            if git('cat-file', '-t', ref).strip() != b'commit':
                raise AuditError('Non-commit push object is not supported')
            for commit in git('rev-list', ref).decode().splitlines():
                if commit in seen:
                    continue
                seen.add(commit)
                raw = git('show', '-s', '--format=%an%x00%ae%x00%cn%x00%ce%x00%B', commit)
                an, ae, cn, ce, _message = raw.split(b'\0', 4)
                if any((name.decode() != approved['name'] or mail.decode() != approved['email']) for name, mail in ((an, ae), (cn, ce))):
                    results.append(('unapproved commit identity', commit[:12]))
                results.extend(findings(blobs.read(commit, b'commit'), 'commit ' + commit[:12], email=approved['email']))
                entries = []
                for (mode, kind, oid), path in tree_entries(git('ls-tree', '-rz', '--full-tree', commit).split(b'\0')):
                    if kind != b'blob' or mode == b'120000':
                        raise AuditError('Submodules/symlinks require an explicit audit')
                    entries.append((path, oid.decode()))
                sources = dict(entries).get(NOTICE_SOURCES)
                notices = notice_hashes(blobs.read(sources)) if sources else set()
                for path, oid in entries:
                    results.extend(scan(path, oid, sources, notices))
        # Also check current index (even when working copy has been sanitised).
        index = []
        for (mode, oid, stage), path in tree_entries(git('ls-files', '--stage', '-z').split(b'\0')):
            if stage != b'0' or mode in {b'120000', b'160000'}:
                raise AuditError('Index has unresolved or unsupported entries')
            index.append((path, oid.decode()))
        sources = dict(index).get(NOTICE_SOURCES)
        notices = notice_hashes(blobs.read(sources)) if sources else set()
        for path, oid in index:
            results.extend(scan(path, oid, sources, notices))
    finally:
        blobs.close()
    working = Path(NOTICE_SOURCES)
    notices = notice_hashes(working.read_bytes()) if working.is_file() else set()
    for raw in git('ls-files', '--cached', '--others', '--exclude-standard', '-z').split(b'\0'):
        if raw:
            path = raw.decode()
            if Path(path).is_symlink():
                raise AuditError('Untracked symlinks require an explicit audit')
            if Path(path).exists() and not Path(path).is_file():
                raise AuditError('Publishable working entry is not a regular file')
            if Path(path).is_file():
                results.extend(audit_blob(path, Path(path).read_bytes(), notices))
    if results:
        for rule, location in sorted(set(results)):
            print('BLOCKED: ' + rule + ' at ' + json.dumps(location, ensure_ascii=True), file=sys.stderr)
        return 1
    print(f'Publication audit passed: {len(seen)} historical commits; index and publishable working files inspected.')
    return 0


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--hook', action='store_true')
    parser.add_argument('remote', nargs='?')
    parser.add_argument('url', nargs='?')
    try:
        return audit(parser.parse_args())
    except (AuditError, OSError, ValueError, UnicodeError):
        print('BLOCKED: publication checks incomplete; inspect local approval, Git state and tool availability.', file=sys.stderr)
        return 1

if __name__ == '__main__':
    sys.exit(main())
