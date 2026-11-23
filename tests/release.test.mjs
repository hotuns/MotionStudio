import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, generateKeyPairSync, sign } from 'node:crypto';
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { readReleaseConfig, readReleaseAssets, verifyInstaller, verifyBuildSource, sha256 } from '../scripts/release-artifacts.mjs';
import { createGitHubClient, publishRelease } from '../scripts/release-github.mjs';
import { signingEnvironment } from '../scripts/release-local.mjs';
import { stageSealosRelease, publishSealosManifest, sealosDownload } from '../scripts/release-sealos.mjs';

const commit = 'a'.repeat(40);

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'motion-release-test-'));
  t.after(() => {
    assert.equal(dirname(realpathSync(root)), realpathSync(tmpdir()));
    rmSync(root, { recursive: true, force: true });
  });
  const write = (path, value) => {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), value);
  };
  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  const id = Buffer.alloc(8, 1);
  const pub = Buffer.concat([Buffer.from('Ed'), id, publicKey.export({ format: 'der', type: 'spki' }).subarray(-32)]);
  const encodedKey = Buffer.from('untrusted comment: test key\n' + pub.toString('base64') + '\n').toString('base64');
  write('package.json', JSON.stringify({ version: '1.2.3' }));
  write('package-lock.json', JSON.stringify({ version: '1.2.3', packages: { '': { version: '1.2.3' } } }));
  write('src-tauri/tauri.conf.json', JSON.stringify({ version: '1.2.3', plugins: { updater: { pubkey: encodedKey } } }));
  const crates = [['src-tauri', 'motion-studio'], ['crates/engine', 'motion-core'], ['crates/host', 'motion-engine']];
  for (const [path, name] of crates) write(path + '/Cargo.toml', '[package]\nname = "' + name + '"\nversion = "1.2.3"\n');
  write('Cargo.lock', crates.map(([, name]) => '[[package]]\nname = "' + name + '"\nversion = "1.2.3"\n').join('\n'));
  write('docs/releases/v1.2.3.md', 'Test release\r\n\r\nNotes.\r\n');
  const config = readReleaseConfig(root);
  const bytes = Buffer.from('MZsigned installer test fixture');
  const rawSignature = sign(null, createHash('blake2b512').update(bytes).digest(), privateKey);
  const comment = 'timestamp:1\tversion:1.2.3';
  const signature = Buffer.from('untrusted comment: test signature\n'
    + Buffer.concat([Buffer.from('ED'), id, rawSignature]).toString('base64') + '\ntrusted comment: ' + comment + '\n'
    + sign(null, Buffer.concat([rawSignature, Buffer.from(comment)]), privateKey).toString('base64') + '\n').toString('base64');
  const artifact = (name, value) => write('artifacts/release-v1.2.3/' + name, value);
  artifact(config.setup, bytes);
  artifact(config.setup + '.sig', signature);
  artifact('SHA256SUMS.txt', sha256(bytes) + '  ' + config.setup + '\n');
  artifact('latest.json', JSON.stringify({ version: config.version, notes: config.notes, pub_date: '2026-11-16T00:00:00Z',
    platforms: { 'windows-x86_64-nsis': { url: config.download(config.setup), signature } },
  }));
  const files = readReleaseAssets(config);
  writeFileSync(config.sourceFile, JSON.stringify({ version: config.version, commit,
    sha256: Object.fromEntries([...files].map(([name, file]) => [name, file.digest])),
  }));
  return { root, config, files, bytes, signature, write, artifact };
}

function mockGitHub(config, { tagCommit = null, interruptUpload = false } = {}) {
  const calls = [];
  let tag = tagCommit ? { object: { type: 'commit', sha: tagCommit } } : null;
  let release = null;
  let interrupted = false;
  const api = async (path, options = {}) => {
    const url = new URL(path, 'https://api.github.com');
    const method = options.method ?? 'GET';
    calls.push({ path: url.pathname, method, name: url.searchParams.get('name') });
    if (method === 'GET') {
      if (url.pathname.endsWith('/git/ref/heads/main')) return { object: { sha: commit } };
      if (url.pathname.endsWith('/git/ref/tags/' + config.tag)) return structuredClone(tag);
      if (url.pathname.endsWith('/releases/tags/' + config.tag)) return structuredClone(release);
      if (url.pathname.endsWith('/assets')) return structuredClone(release.assets);
      if (url.pathname.endsWith('/releases/latest')) return structuredClone(release?.draft ? null : release);
    }
    if (method === 'POST' && url.pathname.endsWith('/git/refs')) {
      tag = { object: { type: 'commit', sha: options.data.sha } };
      return structuredClone(tag);
    }
    if (method === 'POST' && url.pathname.endsWith('/releases')) {
      release = { id: 1, ...options.data, assets: [], html_url: 'https://github.com/hotuns/MotionStudio/releases/tag/' + config.tag,
        upload_url: 'https://uploads.github.com/repos/hotuns/MotionStudio/releases/1/assets{?name,label}' };
      return structuredClone(release);
    }
    if (method === 'POST' && url.pathname.endsWith('/assets')) {
      if (interruptUpload && !interrupted && release.assets.length === 1) {
        interrupted = true;
        throw new Error('Simulated upload disconnect');
      }
      const asset = { id: release.assets.length + 1, name: url.searchParams.get('name'), state: 'uploaded',
        size: options.bytes.length, digest: 'sha256:' + sha256(options.bytes) };
      release.assets.push(asset);
      return structuredClone(asset);
    }
    if (method === 'PATCH') {
      Object.assign(release, options.data);
      return structuredClone(release);
    }
    throw new Error('Unexpected mock API request');
  };
  return { api, calls, get release() { return release; } };
}

test('signed artifacts normalize release notes and remain tied to the exact source commit', t => {
  const f = fixture(t);
  assert.equal(f.config.notes, 'Test release\n\nNotes.');
  verifyBuildSource(f.config, f.files, commit);
  assert.throws(() => verifyBuildSource(f.config, f.files, 'b'.repeat(40)), /original clean source/);
  f.files.get(f.config.setup).digest = 'modified';
  assert.throws(() => verifyBuildSource(f.config, f.files, commit), /original clean source/);
});

test('mirror keeps signed bytes, verifies public downloads, resumes safely and promotes the manifest last', async t => {
  const {config, files} = fixture(t);
  const objects = new Map();
  const fail = status => Object.assign(new Error(String(status)), { $metadata: { httpStatusCode: status } });
  const client = { send: async command => {
    const {Key, Body, IfMatch, IfNoneMatch} = command.input;
    const old = objects.get(Key);
    if (Body) {
      if (IfNoneMatch === '*' && old || IfMatch && IfMatch !== old?.ETag) throw fail(412);
      objects.set(Key, {bytes: Body, ETag: '"' + sha256(Body) + '"'});
      return {};
    }
    if (!old) throw fail(404);
    return {ETag: old.ETag, Body: {transformToString: async () => old.bytes.toString()}};
  }};
  const request = async url => {
    const path = new URL(url).pathname.split('/').slice(2).join('/');
    const item = objects.get(path);
    return new Response(item?.bytes, {status: item ? 200 : 404});
  };
  const manifest = await stageSealosRelease(client, config, files, request);
  assert.equal(objects.has('releases/latest.json'), false);
  const platform = JSON.parse(manifest.bytes).platforms['windows-x86_64-nsis'];
  assert.equal(platform.url, sealosDownload(config, config.setup));
  assert.equal(platform.signature, files.get(config.setup + '.sig').bytes.toString().trim());
  assert.deepEqual(objects.get('releases/' + config.tag + '/' + config.setup).bytes, files.get(config.setup).bytes);
  await stageSealosRelease(client, config, files, request);
  await publishSealosManifest(client, config, manifest, request);
  await publishSealosManifest(client, config, manifest, request);
  assert.deepEqual(objects.get('releases/latest.json').bytes, manifest.bytes);
  objects.get('releases/latest.json').bytes = Buffer.from('{"version":"1.12.0"}');
  await assert.rejects(publishSealosManifest(client, config, manifest, request), /newer release/);
  objects.get('releases/' + config.tag + '/' + config.setup).bytes = Buffer.from('corrupt');
  await assert.rejects(stageSealosRelease(client, config, files, request), /differs/);
});

test('installer bytes, signature comments, and signed version cannot be substituted', t => {
  const f = fixture(t);
  assert.throws(() => verifyInstaller(Buffer.from('MZdifferent'), f.signature, f.config), /signature verification/);
  assert.throws(() => verifyInstaller(f.bytes, f.signature, { ...f.config, version: '1.2.4' }), /signed version/);
  const changed = Buffer.from(Buffer.from(f.signature, 'base64').toString().replace('timestamp:1', 'timestamp:2')).toString('base64');
  assert.throws(() => verifyInstaller(f.bytes, changed, f.config), /signature verification/);
});

test('the updater URL, checksum, and Cargo lockfile versions are checked', t => {
  const f = fixture(t);
  const manifest = JSON.parse(readFileSync(join(f.config.directory, 'latest.json'), 'utf8'));
  manifest.platforms['windows-x86_64-nsis'].url = 'https://example.com/other.exe';
  f.artifact('latest.json', JSON.stringify(manifest));
  assert.throws(() => readReleaseAssets(f.config), /manifest differs/);
  manifest.platforms['windows-x86_64-nsis'].url = f.config.download(f.config.setup);
  f.artifact('latest.json', JSON.stringify(manifest));
  f.artifact('SHA256SUMS.txt', 'wrong');
  assert.throws(() => readReleaseAssets(f.config), /checksum mismatch/);
  f.write('Cargo.lock', readFileSync(join(f.root, 'Cargo.lock'), 'utf8').replace('version = "1.2.3"', 'version = "1.2.2"'));
  assert.throws(() => readReleaseConfig(f.root), /Cargo.lock/);
});

test('local key discovery validates the public key without reading the private key', t => {
  const f = fixture(t);
  const key = '.tauri/motion-studio-release/updater.key';
  f.write(key, 'test private material');
  f.write(key + '.pub', f.config.publicKey);
  const env = signingEnvironment(f.config, {}, f.root);
  assert.equal(env.TAURI_SIGNING_PRIVATE_KEY, join(f.root, key));
  assert.equal(env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD, '');
  f.write(key + '.pub', 'another public key');
  assert.throws(() => signingEnvironment(f.config, {}, f.root), /public key differs/);
});

test('publishing uploads only the four verified assets before making the release latest', async t => {
  const f = fixture(t);
  const mock = mockGitHub(f.config);
  await publishRelease({ ...f, api: mock.api, commit, log: () => {} });
  assert.equal(mock.release.draft, false);
  assert.equal(mock.release.make_latest, 'true');
  assert.deepEqual(mock.release.assets.map(asset => asset.name), f.config.names);
  const writes = mock.calls.filter(call => call.method !== 'GET');
  assert.equal(writes.at(-1).method, 'PATCH');
  const count = writes.length;
  await publishRelease({ ...f, api: mock.api, commit, log: () => {} });
  assert.equal(mock.calls.filter(call => call.method !== 'GET').length, count);
});

test('an interrupted upload resumes without replacing existing assets', async t => {
  const f = fixture(t);
  const mock = mockGitHub(f.config, { interruptUpload: true });
  await assert.rejects(publishRelease({ ...f, api: mock.api, commit, log: () => {} }), /disconnect/);
  assert.equal(mock.release.draft, true);
  await publishRelease({ ...f, api: mock.api, commit, log: () => {} });
  assert.equal(mock.release.assets.length, 4);
  assert.equal(mock.calls.filter(call => call.method === 'POST' && call.name === f.config.setup).length, 1);
});

test('conflicting tags and remotely changed files stop publication without overwrites', async t => {
  const f = fixture(t);
  const conflict = mockGitHub(f.config, { tagCommit: 'b'.repeat(40) });
  await assert.rejects(publishRelease({ ...f, api: conflict.api, commit, log: () => {} }), /another commit/);
  assert.ok(conflict.calls.every(call => call.method === 'GET'));
  const mock = mockGitHub(f.config, { interruptUpload: true });
  await assert.rejects(publishRelease({ ...f, api: mock.api, commit, log: () => {} }));
  mock.release.assets[0].digest = 'sha256:changed';
  const writes = mock.calls.filter(call => call.method !== 'GET').length;
  await assert.rejects(publishRelease({ ...f, api: mock.api, commit, log: () => {} }), /differs/);
  assert.equal(mock.calls.filter(call => call.method !== 'GET').length, writes);
});

test('GitHub credentials cannot be sent to another origin or repository', async () => {
  const api = createGitHubClient('.', { GH_TOKEN: 'test-token-never-sent' });
  await assert.rejects(api('https://example.com/repos/hotuns/MotionStudio/releases'), /destination/);
  await assert.rejects(api('https://uploads.github.com/repos/other/repository/releases'), /destination/);
});
