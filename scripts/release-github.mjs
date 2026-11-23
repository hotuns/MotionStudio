import { spawnSync } from 'node:child_process';
import { repository } from './release-artifacts.mjs';

const base = '/repos/' + repository;

export function createGitHubClient(root, env = process.env) {
  let token = env.GH_TOKEN || env.GITHUB_TOKEN;
  if (!token) {
    const result = spawnSync('git', ['-c', 'credential.interactive=never', 'credential', 'fill'], {
      cwd: root, input: 'protocol=https\nhost=github.com\n\n', encoding: 'utf8', windowsHide: true, timeout: 20000,
      env: { ...env, GIT_TERMINAL_PROMPT: '0', GCM_INTERACTIVE: 'never', GCM_GUI_PROMPT: '0' },
    });
    if (result.status === 0) token = result.stdout.split(/\r?\n/).find(line => line.startsWith('password='))?.slice(9);
  }
  if (!token) throw new Error('Configure Git credentials for github.com or set GH_TOKEN with repository Contents write access');
  return async (path, { method = 'GET', data, bytes, contentType, optional = false } = {}) => {
    const url = new URL(path, 'https://api.github.com');
    if (url.protocol !== 'https:' || !['api.github.com', 'uploads.github.com'].includes(url.hostname)
      || (url.pathname !== base && !url.pathname.startsWith(base + '/'))) {
      throw new Error('Unexpected release API destination');
    }
    const response = await fetch(url, {
      method, redirect: 'error', signal: AbortSignal.timeout(bytes ? 180000 : 60000),
      headers: {
        Accept: 'application/vnd.github+json', 'User-Agent': 'MotionStudio-local-release',
        'X-GitHub-Api-Version': '2022-11-28', Authorization: 'Bearer ' + token,
        ...(data ? { 'Content-Type': 'application/json' } : {}),
        ...(bytes ? { 'Content-Type': contentType } : {}),
      },
      body: bytes ?? (data ? JSON.stringify(data) : undefined),
    });
    if (optional && response.status === 404) return null;
    if (!response.ok) throw new Error('GitHub API ' + response.status + ' ' + method + ' ' + url.pathname);
    return response.status === 204 ? null : response.json();
  };
}

export async function inspectRemote(api, tag) {
  const [head, tagRef, release] = await Promise.all([
    api(base + '/git/ref/heads/main'),
    api(base + '/git/ref/tags/' + tag, { optional: true }),
    api(base + '/releases/tags/' + tag, { optional: true }),
  ]);
  return { head, tagRef, release };
}

export function assertReleaseSource(remote, commit) {
  if (remote.head.object.sha !== commit) throw new Error('Commit and push the release source to origin/main first');
  if (remote.tagRef && (remote.tagRef.object.type !== 'commit' || remote.tagRef.object.sha !== commit)) {
    throw new Error('Existing release tag points to another commit');
  }
}

function verifyRemoteAsset(asset, files) {
  const file = files.get(asset.name);
  if (!file || asset.state !== 'uploaded' || asset.size !== file.bytes.length || asset.digest !== 'sha256:' + file.digest) {
    throw new Error('GitHub asset differs from the verified local package: ' + asset.name);
  }
}

export async function publishRelease({ api, config, files, commit, mirrorUrl, log = console.log }) {
  const remote = await inspectRemote(api, config.tag);
  assertReleaseSource(remote, commit);
  let release = remote.release;
  if (release && release.draft && release.target_commitish !== commit) {
    throw new Error('Existing release draft targets another source');
  }
  // Conflicting assets are never overwritten, including after an interrupted upload.
  for (const asset of release?.assets ?? []) verifyRemoteAsset(asset, files);
  if (!remote.tagRef) {
    if (release) throw new Error('Existing release has no matching source tag');
    await api(base + '/git/refs', { method: 'POST', data: { ref: 'refs/tags/' + config.tag, sha: commit } });
  }
  const body = config.notes + '\n\n- [Windows x64 安装包](' + config.download(config.setup) + ')\n'
    + (mirrorUrl ? '- [Windows x64 安装包（Sealos）](' + mirrorUrl + ')\n' : '')
    + '- [SHA-256 校验值](' + config.download('SHA256SUMS.txt') + ')\n\n'
    + 'Source: https://github.com/' + repository + '/commit/' + commit;
  if (!release) {
    release = await api(base + '/releases', { method: 'POST', data: {
      tag_name: config.tag, target_commitish: commit, name: 'Motion Studio ' + config.tag,
      body, draft: true, prerelease: false,
    } });
  }
  if (release.draft) {
    for (const [name, file] of files) {
      if (release.assets.some(asset => asset.name === name)) continue;
      const upload = new URL(release.upload_url.split('{')[0]);
      upload.searchParams.set('name', name);
      const asset = await api(upload, { method: 'POST', bytes: file.bytes,
        contentType: name.endsWith('.json') ? 'application/json' : name.endsWith('.exe') ? 'application/octet-stream' : 'text/plain',
      });
      verifyRemoteAsset(asset, files);
      log('Uploaded and verified: ' + name);
    }
    const assets = await api(base + '/releases/' + release.id + '/assets');
    if (assets.length !== files.size) throw new Error('Draft release assets are incomplete');
    for (const asset of assets) verifyRemoteAsset(asset, files);
    assertReleaseSource(await inspectRemote(api, config.tag), commit);
    release = await api(base + '/releases/' + release.id, { method: 'PATCH', data: {
      name: 'Motion Studio ' + config.tag, body, draft: false, prerelease: false, make_latest: 'true',
    } });
  }
  const latest = await api(base + '/releases/latest');
  if (release.draft || release.prerelease || latest.id !== release.id || release.assets.length !== files.size) {
    throw new Error('Release is not complete and published as latest');
  }
  for (const asset of release.assets) verifyRemoteAsset(asset, files);
  log('Published: ' + release.html_url);
  return release;
}
