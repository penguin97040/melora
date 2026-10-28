// Builds Melora's Linux packages in a pinned Ubuntu 22.04 container: .deb, .rpm and an
// AppImage for x86_64, and .deb and .rpm for aarch64 (cross-compiled). Every download is
// checked against the SHA-256 below before use, and the build runs without network access:
// the host fetches the crates (`cargo fetch --locked`) and npm packages beforehand.
//
//   node scripts/build-linux.mjs [x86_64] [aarch64]
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream, existsSync } from 'node:fs';
import {
  copyFile,
  mkdir,
  readdir,
  readFile,
  rm,
  writeFile,
} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

const NODE = '22.22.1';
const RUST = '1.99.1';
const INPUTS = {
  [`node-v${NODE}-linux-x64.tar.xz`]: {
    url: `https://nodejs.org/dist/v${NODE}/node-v${NODE}-linux-x64.tar.xz`,
    sha256: '9a6bc82f9b491279147219f6a18add1e18424dce90d41d2a5fcd69d4924ba3aa',
  },
  [`rust-${RUST}-x86_64-unknown-linux-gnu.tar.xz`]: {
    url: `https://static.rust-lang.org/dist/rust-${RUST}-x86_64-unknown-linux-gnu.tar.xz`,
    sha256: '231fcae47ee950621374725b5fc3b5441d2fbda2955ac45dd7dc816d8876cfe7',
  },
  [`rust-std-${RUST}-aarch64-unknown-linux-gnu.tar.xz`]: {
    url: `https://static.rust-lang.org/dist/rust-std-${RUST}-aarch64-unknown-linux-gnu.tar.xz`,
    sha256: 'e25668477083d6a9ddb87b9beb9c00742225f38233ccb4bfba3afa7a07427f52',
    arch: 'aarch64',
  },
};
// The AppImage bundler's tools, under the names it looks for in its cache (it would
// otherwise download them unchecked, one from a moving "continuous" release). The
// type2 runtime is the part that runs on the user's machine.
const APPIMAGE_TOOLS = {
  'AppRun-x86_64': {
    url: 'https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64',
    sha256: 'f30140a43a0a59e46db21bdefdf749b9e9f2c6946e92afabbacf98b8ae73fb4f',
  },
  'linuxdeploy-07333c6-x86_64.AppImage': {
    url: 'https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy-07333c6/linuxdeploy-x86_64.AppImage',
    sha256: '36a2d7e274d12e1050d0e9ecfe11d339ed54720b2bec464c286d53f8b07f5c62',
  },
  'linuxdeploy-plugin-appimage.AppImage': {
    url: 'https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/1-alpha-20250213-1/linuxdeploy-plugin-appimage-x86_64.AppImage',
    sha256: '992d502a248e14ab185448ddf6f6e7d25558cb84d4623c354c3af350c25fccb3',
  },
  'runtime-x86_64': {
    url: 'https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64',
    sha256: '2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d',
  },
};
const TARGETS = {
  x86_64: {
    triple: 'x86_64-unknown-linux-gnu',
    bundles: 'deb,rpm,appimage',
    env: {},
  },
  aarch64: {
    triple: 'aarch64-unknown-linux-gnu',
    bundles: 'deb,rpm',
    env: {
      CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER: 'aarch64-linux-gnu-gcc',
      CC_aarch64_unknown_linux_gnu: 'aarch64-linux-gnu-gcc',
      PKG_CONFIG_ALLOW_CROSS: '1',
      PKG_CONFIG_SYSROOT_DIR: '/',
      PKG_CONFIG_PATH: '/usr/lib/aarch64-linux-gnu/pkgconfig',
    },
  },
};

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: 'inherit', ...options });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
  return result;
}

function sha256(file) {
  return new Promise((resolve, reject) => {
    const hash = createHash('sha256');
    createReadStream(file)
      .on('data', (chunk) => hash.update(chunk))
      .on('error', reject)
      .on('end', () => resolve(hash.digest('hex')));
  });
}

// Downloads once into the cache and refuses anything whose checksum differs.
async function fetchChecked(directory, name, { url, sha256: expected }) {
  const file = path.join(directory, name);
  if (!existsSync(file) || (await sha256(file)) !== expected) {
    await rm(file, { force: true });
    run('curl', ['-fsSL', '--proto', '=https', '-o', `${file}.part`, url]);
    if ((await sha256(`${file}.part`)) !== expected) {
      await rm(`${file}.part`, { force: true });
      throw new Error(`Checksum mismatch for ${name}`);
    }
    await copyFile(`${file}.part`, file);
    await rm(`${file}.part`);
  }
  return file;
}

const requested = process.argv.slice(2);
const arches = requested.length ? requested : Object.keys(TARGETS);
for (const arch of arches)
  if (!TARGETS[arch]) throw new Error(`Unknown architecture ${arch}`);

const root = process.cwd();
const cache =
  process.env.MELORA_LINUX_CACHE ||
  path.join(os.homedir(), '.cache', 'melora-linux-build');
const targetDirectory = path.resolve(
  process.env.MELORA_LINUX_TARGET_DIR || 'target/linux',
);
const cargoHome = process.env.CARGO_HOME || path.join(os.homedir(), '.cargo');
const { version } = JSON.parse(await readFile('package.json', 'utf8'));
const downloads = path.join(cache, 'downloads');
await mkdir(downloads, { recursive: true });
await mkdir(targetDirectory, { recursive: true });
await mkdir(path.join(cargoHome, 'registry'), { recursive: true });

if (!existsSync('node_modules')) run('npm', ['ci']);
run('cargo', ['fetch', '--locked']);

for (const arch of arches) {
  const target = TARGETS[arch];
  const work = path.join('artifacts', 'linux-build', arch);
  await rm(work, { recursive: true, force: true });
  await mkdir(work, { recursive: true });

  // The image, keyed by its Dockerfile and inputs so a change rebuilds it.
  const inputs = Object.entries(INPUTS).filter(
    ([, input]) => !input.arch || input.arch === arch,
  );
  const dockerfile = await readFile('scripts/linux/Dockerfile');
  const key = createHash('sha256').update(dockerfile).update(arch);
  for (const [, input] of inputs) key.update(input.sha256);
  const image = `melora-linux-build:${arch}-${key.digest('hex').slice(0, 12)}`;
  const present = spawnSync('docker', ['image', 'inspect', image], {
    stdio: 'ignore',
  });
  if (present.status !== 0) {
    const context = path.join(cache, `context-${arch}`);
    await rm(context, { recursive: true, force: true });
    await mkdir(path.join(context, 'inputs'), { recursive: true });
    await writeFile(path.join(context, 'Dockerfile'), dockerfile);
    for (const [name, input] of inputs) {
      await copyFile(
        await fetchChecked(downloads, name, input),
        path.join(context, 'inputs', name),
      );
    }
    const proxies = [
      'http_proxy',
      'https_proxy',
      'HTTP_PROXY',
      'HTTPS_PROXY',
      'no_proxy',
      'NO_PROXY',
    ]
      .filter((name) => process.env[name])
      .flatMap((name) => ['--build-arg', name]);
    run('docker', [
      'build',
      ...proxies,
      '--build-arg',
      `ARCH=${arch}`,
      '--build-arg',
      `NODE=${NODE}`,
      '--build-arg',
      `RUST=${RUST}`,
      '-t',
      image,
      context,
    ]);
    await rm(context, { recursive: true, force: true });
  }

  // Licences for this target's runtime crates, staged for /usr/share/doc/melora.
  run('python3', ['scripts/collect-notices.py', target.triple]);
  run('python3', [
    'scripts/package-linux.py',
    'stage',
    path.join(work, 'licences'),
  ]);
  const licences = `/src/${work}/licences`;
  const files = { '/usr/share/doc/melora/licences': licences };
  // Melora's HTTPS client verifies Spotify's certificates against the system's CA store.
  const depends = ['ca-certificates'];
  // `appimageFiles`: what the AppImage carries besides Melora's licences (its notices).
  const writeConfig = (name, appimageFiles = {}) =>
    writeFile(
      path.join(work, name),
      JSON.stringify(
        {
          bundle: {
            active: true,
            icon: [
              'icons/32x32.png',
              'icons/64x64.png',
              'icons/128x128.png',
              'icons/icon.png',
            ],
            category: 'Music',
            shortDescription: 'A personal desktop music client',
            longDescription:
              'Melora is an unofficial desktop client for Spotify. On Linux it controls Spotify playing on another device.',
            linux: {
              deb: { files, depends, section: 'sound' },
              rpm: { files, depends, release: '1' },
              appimage: {
                files: { ...files, ...appimageFiles },
                bundleMediaFramework: false,
              },
            },
          },
        },
        null,
        2,
      ) + '\n',
    );
  await writeConfig('tauri.linux.json');
  if (target.bundles.includes('appimage')) {
    const tools = path.join(work, 'cache', 'tauri');
    await mkdir(tools, { recursive: true });
    for (const [name, tool] of Object.entries(APPIMAGE_TOOLS)) {
      await copyFile(
        await fetchChecked(downloads, name, tool),
        path.join(tools, name),
      );
      run('chmod', ['+x', path.join(tools, name)]);
    }
  }

  await rm(path.join(targetDirectory, `tmp-${arch}`), {
    recursive: true,
    force: true,
  });
  await mkdir(path.join(targetDirectory, `tmp-${arch}`), { recursive: true });
  const user = `${process.getuid()}:${process.getgid()}`;
  const environment = {
    HOME: '/tmp',
    // linuxdeploy unpacks itself here: on the host's disk (Docker's own can fill up) and not in
    // the checkout, which may be on a filesystem without symlinks.
    TMPDIR: `/target/tmp-${arch}`,
    CI: 'true',
    CARGO_HOME: '/cargo',
    CARGO_NET_OFFLINE: 'true',
    CARGO_TARGET_DIR: '/target',
    XDG_CACHE_HOME: `/src/${work}/cache`,
    LDAI_RUNTIME_FILE: `/src/${work}/cache/tauri/runtime-x86_64`,
    ...target.env,
  };
  const container = (...command) =>
    run('docker', [
      'run',
      '--rm',
      '--network',
      'none',
      '--user',
      user,
      ...Object.entries(environment).flatMap(([name, value]) => [
        '-e',
        `${name}=${value}`,
      ]),
      '-v',
      `${root}:/src`,
      '-v',
      `${path.join(cargoHome, 'registry')}:/cargo/registry`,
      '-v',
      `${targetDirectory}:/target`,
      '-w',
      '/src',
      image,
      ...command,
    ]);
  const build = (bundles, config) =>
    container(
      'node',
      'node_modules/@tauri-apps/cli/tauri.js',
      'build',
      '--ci',
      '--target',
      target.triple,
      '--bundles',
      bundles,
      '--config',
      `${work}/${config}`,
    );
  build(target.bundles, 'tauri.linux.json');
  const audit = [];
  if (target.bundles.includes('appimage')) {
    // The AppImage carries the build image's libraries, so it carries their notices too. Only
    // the first build shows which libraries linuxdeploy bundles; the copyright files it doesn't
    // copy itself are then added, and the AppImage is bundled again (the program isn't rebuilt).
    const notices = path.join(work, 'appimage-notices');
    container(
      'python3',
      'scripts/package-linux.py',
      'appimage-notices',
      `/target/${target.triple}/release/bundle/appimage/Melora_${version}_amd64.AppImage`,
      `/src/${notices}`,
    );
    const system = existsSync(path.join(notices, 'system'))
      ? await readdir(path.join(notices, 'system'))
      : [];
    await writeConfig('tauri.appimage.json', {
      ...Object.fromEntries(
        system.map((name) => [
          `/usr/share/doc/${name}/copyright`,
          `/src/${notices}/system/${name}/copyright`,
        ]),
      ),
      '/usr/share/doc/melora/appimage': `/src/${notices}/appimage`,
    });
    build('appimage', 'tauri.appimage.json');
    audit.push(`/src/${notices}`, environment.LDAI_RUNTIME_FILE);
  }
  container(
    'python3',
    'scripts/package-linux.py',
    'audit',
    arch,
    version,
    `/target/${target.triple}/release`,
    licences,
    'artifacts/linux',
    ...audit,
  );
}
