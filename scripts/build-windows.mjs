import { spawnSync } from 'node:child_process';
import { mkdir, readFile } from 'node:fs/promises';
import path from 'node:path';

function run(command, args, env = process.env) {
  const result = spawnSync(command, args, { stdio: 'inherit', env });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
const target = 'x86_64-pc-windows-msvc';
const targetDirectory = path.resolve(
  process.env.CARGO_TARGET_DIR || 'src-tauri/target',
);
const env = { ...process.env, CARGO_TARGET_DIR: targetDirectory };
const args = [
  'node_modules/@tauri-apps/cli/tauri.js',
  'build',
  '--no-bundle',
  '--target',
  target,
];
if (process.platform !== 'win32') args.push('--runner', 'cargo-xwin');
run(process.execPath, args, env);
run(
  process.platform === 'win32' ? 'python' : 'python3',
  ['scripts/collect-notices.py'],
  env,
);
await mkdir('artifacts', { recursive: true });
const { version } = JSON.parse(await readFile('package.json', 'utf8'));
const exe = path.join(targetDirectory, target, 'release', 'melora.exe');
run(process.platform === 'win32' ? 'python' : 'python3', [
  'scripts/package-windows.py',
  exe,
  version,
]);
