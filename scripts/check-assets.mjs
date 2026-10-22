import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';

const allowedTopLevel = new Set(['index.html', 'assets', 'fonts']);
for (const name of await readdir('dist')) {
  if (!allowedTopLevel.has(name))
    throw new Error(`Unexpected packaged asset: ${name}`);
}
async function inspect(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) await inspect(file);
    else if (
      /agents|claude\.md|handoff|do_next|instructions|private-archive|spotify_integration|changelog|mistakes|build_spec|start-melora|foobar_ui|\.env|\.map$|\.ps1$/i.test(
        entry.name,
      )
    ) {
      throw new Error(`Private or development asset in package: ${file}`);
    }
  }
}
await inspect('dist');
const config = JSON.parse(await readFile('src-tauri/tauri.conf.json', 'utf8'));
if (
  config.bundle.resources.length !== 0 ||
  config.build.frontendDist !== '../dist'
) {
  throw new Error(
    'Unexpected native resources or frontend location; review packaging scope.',
  );
}
console.log(
  'Packaged asset audit passed: frontend assets and bundled font licences only.',
);
