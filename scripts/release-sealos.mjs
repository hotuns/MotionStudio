import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { S3Client, HeadBucketCommand, HeadObjectCommand, GetObjectCommand, PutObjectCommand } from '@aws-sdk/client-s3';
import { sha256 } from './release-artifacts.mjs';

// The updater and the publisher consume the same public, path-style release URL.
const base = new URL(readFileSync(new URL('../src-tauri/sealos-release-url.txt', import.meta.url), 'utf8').trim());
const [bucket, ...prefix] = base.pathname.slice(1).split('/');
const key = path => [...prefix, path].join('/');
const publicUrl = path => base.href + '/' + path;

export function createSealosClient(credentials) {
  return new S3Client({
    endpoint: base.origin, region: 'us-east-1', forcePathStyle: true, credentials,
    requestChecksumCalculation: 'WHEN_REQUIRED', responseChecksumValidation: 'WHEN_REQUIRED',
  });
}

export const checkSealosAccess = client => client.send(new HeadBucketCommand({ Bucket: bucket }));
export const sealosDownload = (config, name) => publicUrl(config.tag + '/' + name);

async function existing(client, command) {
  try {
    return await client.send(command);
  } catch (error) {
    if (error.$metadata?.httpStatusCode === 404) return null;
    throw error;
  }
}

async function verifyPublic(path, file, request) {
  const response = await request(publicUrl(path), {
    redirect: 'error', signal: AbortSignal.timeout(180000), cache: 'no-store',
  });
  if (!response.ok) throw new Error('Sealos public GET ' + response.status + ': ' + path);
  const digest = createHash('sha256');
  let size = 0;
  for await (const chunk of response.body) {
    size += chunk.length;
    if (size > file.bytes.length) throw new Error('Sealos public object is larger than the release file: ' + path);
    digest.update(chunk);
  }
  if (size !== file.bytes.length || digest.digest('hex') !== file.digest) {
    throw new Error('Sealos public object differs from the signed release: ' + path);
  }
}

function put(client, path, file, condition, cacheControl) {
  const type = path.endsWith('.json') ? 'application/json' : path.endsWith('.exe') ? 'application/octet-stream' : 'text/plain';
  return client.send(new PutObjectCommand({
    Bucket: bucket, Key: key(path), Body: file.bytes, ContentType: type, CacheControl: cacheControl, ...condition,
  }));
}

export async function stageSealosRelease(client, config, files, request = fetch) {
  const manifest = JSON.parse(files.get('latest.json').bytes.toString('utf8'));
  manifest.platforms['windows-x86_64-nsis'].url = sealosDownload(config, config.setup);
  const bytes = Buffer.from(JSON.stringify(manifest, null, 2) + '\n');
  const mirrorManifest = { bytes, digest: sha256(bytes) };
  for (const [name, original] of files) {
    const file = name === 'latest.json' ? mirrorManifest : original;
    const path = config.tag + '/' + name;
    const found = await existing(client, new HeadObjectCommand({ Bucket: bucket, Key: key(path) }));
    if (!found) {
      try {
        await put(client, path, file, { IfNoneMatch: '*' }, 'public, max-age=31536000, immutable');
      } catch (error) {
        if (error.$metadata?.httpStatusCode !== 412) throw error;
      }
    }
    // Also verifies a previous, interrupted upload without replacing its bytes.
    await verifyPublic(path, file, request);
    console.log('Verified Sealos download: ' + path);
  }
  return mirrorManifest;
}

function compareVersions(a, b) {
  const parts = value => {
    if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(value)) throw new Error('Invalid stable version in Sealos manifest');
    return value.split('.').map(BigInt);
  };
  const left = parts(a), right = parts(b);
  for (let i = 0; i < left.length; i++) {
    if (left[i] !== right[i]) return left[i] > right[i] ? 1 : -1;
  }
  return 0;
}

export async function publishSealosManifest(client, config, manifest, request = fetch) {
  const path = 'latest.json';
  const previous = await existing(client, new GetObjectCommand({ Bucket: bucket, Key: key(path) }));
  if (previous) {
    const old = await previous.Body.transformToString();
    const order = compareVersions(JSON.parse(old).version, config.version);
    if (order > 0) throw new Error('Sealos already serves a newer release');
    if (order === 0) {
      if (sha256(old) !== manifest.digest) throw new Error('Sealos already serves a different manifest for ' + config.tag);
      await verifyPublic(path, manifest, request);
      return;
    }
    if (!previous.ETag) throw new Error('Missing ETag for the current Sealos manifest');
  }
  // Advertise only verified files and preserve a concurrently published manifest.
  await put(client, path, manifest, previous ? { IfMatch: previous.ETag } : { IfNoneMatch: '*' }, 'no-cache, max-age=0, must-revalidate');
  await verifyPublic(path, manifest, request);
  console.log('Published Sealos update: ' + publicUrl(path));
}
