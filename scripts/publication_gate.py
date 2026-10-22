#!/usr/bin/env python3
"""Local fail-closed publication audit. Diagnostics contain locations, never values."""
import argparse
import fnmatch
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

PRIVATE = ('agents.md', 'changelog.md', 'mistakes.md', 'codex_start_here.md',
           'instructions.md', 'spotify_client_build_spec.md', 'start-melora-*.ps1',
           'foobar_ui.png', 'claude.md', 'handoff.md', 'do_next.md', 'user_instructions.md',
           'spotify_integration.md', '.private-archive', '.env', '.env.*', '*.pem', '*.key', '*.p12', '*.pfx',
           '*.mobileprovision', '*.p8', '*.crt', '*.cer', '*.der', '*.keystore', '*.jks', '*.db', '*.sqlite*', '*.exe', '*.zip')
OUTPUTS = {'node_modules', 'target', 'dist', 'artifacts', 'data', '.xwin-cache', 'credentials', 'account-data', 'secrets'}
RULES = {
    'credential': re.compile(r'(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|sk-(?:proj-|ant-)?[A-Za-z0-9_-]{20,}|(?:AKIA|ASIA)[A-Z0-9]{16}|-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----|eyJ[A-Za-z0-9_-]{12,}\.[A-Za-z0-9_-]{12,}\.[A-Za-z0-9_-]{12,})'),
    'sensitive assignment': re.compile(r'''(?i)(?:client[_ -]?secret|access[_ -]?token|refresh[_ -]?token|api[_ -]?key|secret|token|credential|private[_ -]?key|authori[sz]ation[_ -]?code|password|code[_ -]?verifier|spotify[_ -]?client[_ -]?id)\s*["']?\s*[:=]\s*["']?([A-Za-z0-9_+/.=-]{12,})'''),
    'attribution': re.compile(r'(?i)(?:co-' r'authored-by:.*(?:bot|claude|openai|codex|anthropic)|(?:generated|written|authored|created) (?:by|with) (?:AI|ChatGPT|Claude|Codex)|(?:AI|Claude|Codex|ChatGPT) co-author)'),
    'personal email': re.compile(r'[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}'),
    'personal path': re.compile(r'(?:[A-Za-z]:[\\/](?:Users|Documents and Settings)[\\/][^\s/\\]+|' r'/' r'Users/[^\s/]+|' r'/' r'home/(?!runner\b|agent\b)[^\s/]+)'),
}
PLACEHOLDERS = {'YOUR_SPOTIFY_CLIENT_ID', 'YOUR_CLIENT_ID', 'YOUR_API_KEY', 'PLACEHOLDER', 'your_client_id_here'}

class AuditError(Exception):
    pass


def git(*args):
    result = subprocess.run(['git', *args], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode:
        raise AuditError('Git check incomplete')
    return result.stdout


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
            if rule == 'personal email' and (licence or match.group() == email):
                continue
            result.append((rule, location))
            break
    return result


def audit_blob(path, data):
    path_results = findings(path.encode(), 'redacted path ' + hashlib.sha256(path.encode()).hexdigest()[:12])
    # Tauri's standard retina icon name resembles an email address. Only its
    # filename gets this exception; its contents still receive every rule.
    if path == 'src-tauri/icons/128x128' + '@' + '2x.png':
        path_results = [result for result in path_results if result[0] != 'personal email']
    location = path_results[0][1] if path_results else path
    if prohibited(path):
        return path_results + [('prohibited private/output path', location)]
    # Only known font/icon formats may be binary. Scan ASCII secret signatures too.
    if b'\0' in data and Path(path).suffix in {'.woff2', '.png', '.ico', '.icns'}:
        return path_results + findings(data.decode('latin1').encode(), location)
    return path_results + findings(data, location, licence=path.startswith('public/fonts/licences/'))


def policy():
    cfg = Path(git('rev-parse', '--git-path', 'melora-publication.json').decode().strip())
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
    refs = []
    if args.hook:
        for line in sys.stdin:
            fields = line.split()
            if len(fields) != 4 or not all(re.fullmatch(r'(?:[0-9a-f]{40}|[0-9a-f]{64})', fields[i]) for i in (1, 3)):
                raise AuditError('Invalid push ref input')
            if set(fields[1]) != {'0'}:
                refs.append(fields[1])
    else:
        head = subprocess.run(['git', 'rev-parse', '--verify', 'HEAD'], capture_output=True)
        if head.returncode == 0:
            refs.append(head.stdout.decode().strip())
    # Audit complete ancestry of outgoing refs, including every historical tree.
    seen = set()
    results = []
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
            results.extend(findings(git('cat-file', 'commit', commit), 'commit ' + commit[:12], email=approved['email']))
            for entry in git('ls-tree', '-rz', '--full-tree', commit).split(b'\0'):
                if not entry:
                    continue
                meta, path = entry.split(b'\t', 1)
                mode, kind, oid = meta.split()
                path = path.decode('utf-8')
                if kind != b'blob' or mode == b'120000':
                    raise AuditError('Submodules/symlinks require an explicit audit')
                results.extend(audit_blob(path, git('cat-file', 'blob', oid.decode())))
    # Also check current index (even when working copy has been sanitised).
    for entry in git('ls-files', '--stage', '-z').split(b'\0'):
        if not entry:
            continue
        meta, path = entry.split(b'\t', 1)
        mode, oid, stage = meta.split()
        if stage != b'0' or mode in {b'120000', b'160000'}:
            raise AuditError('Index has unresolved or unsupported entries')
        results.extend(audit_blob(path.decode(), git('cat-file', 'blob', oid.decode())))
    for raw in git('ls-files', '--cached', '--others', '--exclude-standard', '-z').split(b'\0'):
        if raw:
            path = raw.decode()
            if Path(path).is_symlink():
                raise AuditError('Untracked symlinks require an explicit audit')
            if Path(path).exists() and not Path(path).is_file():
                raise AuditError('Publishable working entry is not a regular file')
            if Path(path).is_file():
                results.extend(audit_blob(path, Path(path).read_bytes()))
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
