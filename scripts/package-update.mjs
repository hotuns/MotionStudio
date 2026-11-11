import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { portableEntries, stagePortable } from './package-portable.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const read = path => readFileSync(join(root, path), 'utf8');
const { version } = JSON.parse(read('package.json'));
const tag = 'v' + version;
const tauri = JSON.parse(read('src-tauri/tauri.conf.json'));
if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) {
  throw new Error('Release versions must be stable SemVer (X.Y.Z)');
}
for (const path of ['src-tauri', 'crates/engine', 'crates/host']) {
  if (read(path + '/Cargo.toml').match(/^version = "([^"]+)"/m)?.[1] !== version) {
    throw new Error('Version mismatch: ' + path + '/Cargo.toml');
  }
}
const npmLock = JSON.parse(read('package-lock.json'));
if (tauri.version !== version || npmLock.version !== version || npmLock.packages[''].version !== version) {
  throw new Error('Tauri, npm package, and npm lockfile versions must agree');
}
if (process.argv.includes('--check-tag')) {
  if (process.env.GITHUB_REF_TYPE !== 'tag' || process.env.GITHUB_REF_NAME !== tag) {
    throw new Error('The release tag must be ' + tag);
  }
  console.log('Release tag and package versions agree: ' + tag);
  process.exit(0);
}
if (!process.env.TAURI_SIGNING_PRIVATE_KEY) throw new Error('Set TAURI_SIGNING_PRIVATE_KEY before packaging the update');

const artifacts = join(root, 'artifacts');
const output = join(artifacts, 'release-' + tag);
if (existsSync(output)) throw new Error('Release output already exists: ' + output);
const setupName = 'MotionStudio-' + version + '-x64-setup.exe';
const portableName = 'MotionStudio-' + version + '-windows-x64';
const setupBuild = resolve(root, process.env.CARGO_TARGET_DIR || 'target', 'release/bundle/nsis', 'Motion Studio_' + version + '_x64-setup.exe');
if (!existsSync(setupBuild + '.sig')) {
  throw new Error('Build a signed NSIS package using src-tauri/tauri.release.conf.json first');
}
const downloadBase = 'https://github.com/hotuns/MotionStudio/releases/download/' + tag + '/';
const notesFile = 'docs/releases/' + tag + '.md';
const notes = existsSync(join(root, notesFile)) ? read(notesFile).trim() : '';

function run(executable, args, errorMessage) {
  const result = spawnSync(executable, args, { cwd: root, encoding: 'utf8', windowsHide: true });
  if (result.status !== 0) throw new Error(errorMessage);
}

function releaseAsset(dir, file) {
  const signature = readFileSync(join(dir, file + '.sig'), 'utf8').trim();
  const signed = Buffer.from(signature, 'base64').toString('utf8').split(/\r?\n/);
  const key = Buffer.from(tauri.plugins.updater.pubkey, 'base64').toString('utf8').split(/\r?\n/);
  if (!Buffer.from(key[1] ?? '', 'base64').subarray(2, 10).equals(Buffer.from(signed[1] ?? '', 'base64').subarray(2, 10))
    || !signed[2]?.split('\t').includes('version:' + version)) {
    throw new Error('Wrong signing key or signed version: ' + file);
  }
  return { url: downloadBase + file, signature };
}

mkdirSync(artifacts, { recursive: true });
const stage = mkdtempSync(join(realpathSync(artifacts), '.update-release-'));
try {
  copyFileSync(setupBuild, join(stage, setupName));
  copyFileSync(setupBuild + '.sig', join(stage, setupName + '.sig'));
  const portableDir = join(stage, portableName);
  stagePortable(version, portableDir);
  const zipName = portableName + '.zip';
  const zip = join(stage, zipName);
  run('tar.exe', ['-a', '-cf', zip, '-C', portableDir, ...portableEntries], 'Could not archive the portable package');
  run(process.execPath, [
    join(root, 'node_modules/@tauri-apps/cli/tauri.js'), 'signer', 'sign', '--app-version', version, zip,
  ], 'Could not sign the portable package. Check the updater signing credentials.');

  const manifest = {
    version, notes, pub_date: new Date().toISOString(),
    platforms: {
      'windows-x86_64-nsis': releaseAsset(stage, setupName),
      'windows-x86_64-portable': releaseAsset(stage, zipName),
    },
  };
  const checksums = [setupName, zipName].map(file =>
    createHash('sha256').update(readFileSync(join(stage, file))).digest('hex') + '  ' + file);
  writeFileSync(join(stage, 'latest.json'), JSON.stringify(manifest, null, 2) + '\n');
  writeFileSync(join(stage, 'SHA256SUMS.txt'), checksums.join('\n') + '\n');
  renameSync(stage, output);
} finally {
  if (existsSync(stage) && dirname(realpathSync(stage)) === realpathSync(artifacts)) {
    rmSync(stage, { recursive: true });
  }
}
console.log('Signed release: ' + output);
