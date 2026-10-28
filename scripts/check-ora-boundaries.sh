#!/usr/bin/env bash
# Keep the ora crates' boundaries: ora-core stays free of Tauri, and the committed TypeScript
# bindings match what ora-types generates.
set -euo pipefail
cd "$(dirname "$0")/.."

fail() {
  echo "ora boundary check failed: $1" >&2
  exit 1
}

tree=$(cargo tree --locked -p ora-core --target all -e normal,build --prefix none --format '{p}') ||
  fail 'could not list the ora-core dependency tree.'
if grep -Eq '^tauri([-_][A-Za-z0-9_-]+)? v' <<<"$tree"; then
  fail 'ora-core must not depend on tauri.'
fi

# ts-rs never deletes a binding it no longer generates, so start from none: a stale file for a
# removed or renamed type then shows up as a deletion below.
find src/native/bindings -name '*.ts' -delete
cargo test --locked -q -p ora-types >/dev/null ||
  fail 'ora-types tests failed.'
# Compare against the index, so freshly regenerated bindings pass once they are staged.
if ! git diff --quiet -- src/native/bindings ||
  [ -n "$(git ls-files --others --exclude-standard -- src/native/bindings)" ]; then
  git status --short --untracked-files=all -- src/native/bindings >&2
  fail 'generated bindings in src/native/bindings are out of date. Stage the regenerated files.'
fi

echo 'ora boundary check passed: ora-core is Tauri-free, bindings are current.'
