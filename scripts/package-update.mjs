import { createHash } from 'node:crypto';
import { copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readReleaseConfig, verifyInstaller } from './release-artifacts.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const config = readReleaseConfig(root);
const { version, tag, notes } = config;
if (process.argv.includes('--check-tag')) {
  const explicitTag = process.argv[process.argv.indexOf('--check-tag') + 1];
  if ((explicitTag ?? (process.env.GITHUB_REF_TYPE === 'tag' ? process.env.GITHUB_REF_NAME : '')) !== tag) {
    throw new Error('The release tag must be ' + tag);
  }
  console.log('Release tag and package versions agree: ' + tag);
  process.exit(0);
}
const artifacts = join(root, 'artifacts');
const output = join(artifacts, 'release-' + tag);
if (existsSync(output)) throw new Error('Release output already exists: ' + output);
const setupName = 'MotionStudio-' + version + '-x64-setup.exe';
const setupBuild = resolve(root, process.env.CARGO_TARGET_DIR || 'target', 'release/bundle/nsis', 'Motion Studio_' + version + '_x64-setup.exe');
if (!existsSync(setupBuild + '.sig')) {
  throw new Error('Build a signed NSIS package using src-tauri/tauri.release.conf.json first');
}
const downloadBase = 'https://github.com/hotuns/MotionStudio/releases/download/' + tag + '/';

function releaseAsset(dir, file) {
  const signature = readFileSync(join(dir, file + '.sig'), 'utf8').trim();
  verifyInstaller(readFileSync(join(dir, file)), signature, config);
  console.log('Verified installer and signed version: ' + file);
  return { url: downloadBase + file, signature };
}

mkdirSync(artifacts, { recursive: true });
const artifactsRoot = realpathSync(artifacts);
const stage = mkdtempSync(join(artifactsRoot, '.update-release-'));
function removeReleaseDirectory(path) {
  if (existsSync(path) && dirname(realpathSync(path)) === artifactsRoot) {
    rmSync(path, { recursive: true, maxRetries: 3 });
  }
}

try {
  copyFileSync(setupBuild, join(stage, setupName));
  copyFileSync(setupBuild + '.sig', join(stage, setupName + '.sig'));
  const manifest = {
    version, notes, pub_date: new Date().toISOString(),
    platforms: {
      'windows-x86_64-nsis': releaseAsset(stage, setupName),
    },
  };
  const checksum = createHash('sha256').update(readFileSync(join(stage, setupName))).digest('hex');
  writeFileSync(join(stage, 'latest.json'), JSON.stringify(manifest, null, 2) + '\n');
  writeFileSync(join(stage, 'SHA256SUMS.txt'), `${checksum}  ${setupName}\n`);
  try {
    renameSync(stage, output);
  } catch (error) {
    if (process.platform !== 'win32' || !['EPERM', 'EACCES', 'EBUSY'].includes(error.code)) throw error;
    // Windows can block renaming a directory while another process reads its files.
    // Reserve the destination first so the fallback cannot replace another release.
    mkdirSync(output);
    try {
      cpSync(stage, output, { recursive: true, force: false, errorOnExist: true });
    } catch (copyError) {
      removeReleaseDirectory(output);
      throw copyError;
    }
  }
} finally {
  removeReleaseDirectory(stage);
}
console.log('Signed release: ' + output);
