import { createHash, createPublicKey, verify } from 'node:crypto';
import { copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

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
const artifacts = join(root, 'artifacts');
const output = join(artifacts, 'release-' + tag);
if (existsSync(output)) throw new Error('Release output already exists: ' + output);
const setupName = 'MotionStudio-' + version + '-x64-setup.exe';
const setupBuild = resolve(root, process.env.CARGO_TARGET_DIR || 'target', 'release/bundle/nsis', 'Motion Studio_' + version + '_x64-setup.exe');
if (!existsSync(setupBuild + '.sig')) {
  throw new Error('Build a signed NSIS package using src-tauri/tauri.release.conf.json first');
}
const downloadBase = 'https://github.com/hotuns/MotionStudio/releases/download/' + tag + '/';
const notesFile = 'docs/releases/' + tag + '.md';
const notes = existsSync(join(root, notesFile)) ? read(notesFile).trim() : '';

function releaseAsset(dir, file) {
  const signature = readFileSync(join(dir, file + '.sig'), 'utf8').trim();
  const signed = Buffer.from(signature, 'base64').toString('utf8').split(/\r?\n/);
  const key = Buffer.from(tauri.plugins.updater.pubkey, 'base64').toString('utf8').split(/\r?\n/);
  const publicPacket = Buffer.from(key[1] ?? '', 'base64');
  const signaturePacket = Buffer.from(signed[1] ?? '', 'base64');
  const commentSignature = Buffer.from(signed[3] ?? '', 'base64');
  if (publicPacket.length !== 42 || !['Ed', 'ED'].includes(publicPacket.subarray(0, 2).toString('ascii'))
    || signaturePacket.length !== 74 || signaturePacket.subarray(0, 2).toString('ascii') !== 'ED'
    || commentSignature.length !== 64 || !signed[2]?.startsWith('trusted comment: ')) {
    throw new Error('Invalid or unsupported update signature: ' + file);
  }
  const trustedComment = signed[2].slice('trusted comment: '.length);
  const signedVersions = trustedComment.split('\t').filter(field => field.startsWith('version:'));
  if (!publicPacket.subarray(2, 10).equals(signaturePacket.subarray(2, 10))
    || signedVersions.length !== 1 || signedVersions[0] !== 'version:' + version) {
    throw new Error('Wrong signing key or signed version: ' + file);
  }
  // Tauri's Minisign ED format: Ed25519 over a BLAKE2b-512 file digest,
  // plus Ed25519 over the raw signature and its trusted comment. Match the
  // updater's minisign-verify semantics without compiling the desktop tests.
  const publicKey = createPublicKey({
    key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), publicPacket.subarray(10)]),
    format: 'der', type: 'spki',
  });
  const fileSignature = signaturePacket.subarray(10);
  const digest = createHash('blake2b512').update(readFileSync(join(dir, file))).digest();
  if (!verify(null, digest, publicKey, fileSignature)
    || !verify(null, Buffer.concat([fileSignature, Buffer.from(trustedComment, 'utf8')]), publicKey, commentSignature)) {
    throw new Error('Installer or signed version failed signature verification: ' + file);
  }
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
