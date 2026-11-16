import { createHash, createPublicKey, verify } from 'node:crypto';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

export const repository = 'hotuns/MotionStudio';
export const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

export function readReleaseConfig(root) {
  const read = path => readFileSync(join(root, path), 'utf8');
  const { version } = JSON.parse(read('package.json'));
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) {
    throw new Error('Release versions must be stable SemVer (X.Y.Z)');
  }
  const tauri = JSON.parse(read('src-tauri/tauri.conf.json'));
  const npmLock = JSON.parse(read('package-lock.json'));
  if (tauri.version !== version || npmLock.version !== version || npmLock.packages[''].version !== version) {
    throw new Error('Tauri, npm package, and npm lockfile versions must agree');
  }
  const cargoPackages = read('Cargo.lock').split(/^\[\[package\]\]/m);
  for (const path of ['src-tauri', 'crates/engine', 'crates/host']) {
    const manifest = read(path + '/Cargo.toml');
    const name = manifest.match(/^name = "([^"]+)"/m)?.[1];
    const locked = cargoPackages.find(block => block.match(/^name = "([^"]+)"/m)?.[1] === name);
    if (manifest.match(/^version = "([^"]+)"/m)?.[1] !== version
      || locked?.match(/^version = "([^"]+)"/m)?.[1] !== version) {
      throw new Error('Version mismatch: ' + path + '/Cargo.toml or Cargo.lock');
    }
  }
  const tag = 'v' + version;
  const setup = 'MotionStudio-' + version + '-x64-setup.exe';
  const notesPath = join(root, 'docs/releases', tag + '.md');
  const notes = existsSync(notesPath) ? readFileSync(notesPath, 'utf8').replace(/\r\n/g, '\n').trim() : '';
  return {
    root, version, tag, setup, notes, publicKey: tauri.plugins.updater.pubkey,
    directory: join(root, 'artifacts', 'release-' + tag),
    sourceFile: join(root, 'artifacts', 'release-' + tag + '.source.json'),
    names: [setup, setup + '.sig', 'latest.json', 'SHA256SUMS.txt'],
    download: name => 'https://github.com/' + repository + '/releases/download/' + tag + '/' + name,
  };
}

export function verifyInstaller(bytes, signature, config) {
  const signed = Buffer.from(signature, 'base64').toString('utf8').split(/\r?\n/);
  const key = Buffer.from(config.publicKey, 'base64').toString('utf8').split(/\r?\n/);
  const publicPacket = Buffer.from(key[1] ?? '', 'base64');
  const packet = Buffer.from(signed[1] ?? '', 'base64');
  const commentSignature = Buffer.from(signed[3] ?? '', 'base64');
  if (publicPacket.length !== 42 || !['Ed', 'ED'].includes(publicPacket.subarray(0, 2).toString('ascii'))
    || packet.length !== 74 || packet.subarray(0, 2).toString('ascii') !== 'ED'
    || commentSignature.length !== 64 || !signed[2]?.startsWith('trusted comment: ')) {
    throw new Error('Invalid or unsupported update signature');
  }
  const comment = signed[2].slice('trusted comment: '.length);
  const versions = comment.split('\t').filter(field => field.startsWith('version:'));
  if (!publicPacket.subarray(2, 10).equals(packet.subarray(2, 10))
    || versions.length !== 1 || versions[0] !== 'version:' + config.version) {
    throw new Error('Wrong signing key or signed version');
  }
  // Same Minisign ED / BLAKE2b-512 / Ed25519 checks as the updater, without
  // compiling or running the desktop application to verify a release.
  const publicKey = createPublicKey({
    key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), publicPacket.subarray(10)]),
    format: 'der', type: 'spki',
  });
  const fileSignature = packet.subarray(10);
  if (!verify(null, createHash('blake2b512').update(bytes).digest(), publicKey, fileSignature)
    || !verify(null, Buffer.concat([fileSignature, Buffer.from(comment)]), publicKey, commentSignature)) {
    throw new Error('Installer or signed version failed signature verification');
  }
}

export function readReleaseAssets(config) {
  const names = readdirSync(config.directory);
  if (names.length !== config.names.length || names.some(name => !config.names.includes(name))) {
    throw new Error('Expected exactly the four installer/update release files');
  }
  const files = new Map(config.names.map(name => {
    const bytes = readFileSync(join(config.directory, name));
    if (!bytes.length) throw new Error('Empty release asset: ' + name);
    return [name, { bytes, digest: sha256(bytes) }];
  }));
  const manifest = JSON.parse(files.get('latest.json').bytes.toString('utf8'));
  const signature = files.get(config.setup + '.sig').bytes.toString('utf8').trim();
  const platform = manifest.platforms?.['windows-x86_64-nsis'];
  if (manifest.version !== config.version || manifest.notes !== config.notes
    || !Number.isFinite(Date.parse(manifest.pub_date)) || Object.keys(manifest.platforms ?? {}).length !== 1
    || platform?.url !== config.download(config.setup) || platform.signature !== signature) {
    throw new Error('Updater manifest differs from the release');
  }
  if (files.get('SHA256SUMS.txt').bytes.toString('utf8').trim() !== files.get(config.setup).digest + '  ' + config.setup) {
    throw new Error('Installer checksum mismatch');
  }
  if (files.get(config.setup).bytes.subarray(0, 2).toString() !== 'MZ') throw new Error('Installer is not a PE executable');
  verifyInstaller(files.get(config.setup).bytes, signature, config);
  return files;
}

export function verifyBuildSource(config, files, commit) {
  const source = JSON.parse(readFileSync(config.sourceFile, 'utf8'));
  if (source.commit !== commit || source.version !== config.version
    || files.size !== Object.keys(source.sha256 ?? {}).length
    || [...files].some(([name, file]) => source.sha256[name] !== file.digest)) {
    throw new Error('Release artifacts do not belong to this commit; rebuild from their original clean source');
  }
}
