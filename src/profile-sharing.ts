import type {GamepadTarget, Profile} from './types';

const catalogRoot = 'https://raw.githubusercontent.com/hotuns/MotionStudio/main/community-configs/';
const maxBytes = 512 * 1024;
export const maxShareCodeLength = 4 + Math.ceil((maxBytes + 1024) * 4 / 3);

export type CommunityProfile = {
  code: string;
  sha256: string;
  name: string;
  game: string;
  author: string;
  controller: string;
  output: GamepadTarget;
  description: string;
  notes: string;
  verified: boolean;
};

export type SharedSource =
  | {kind: 'code'; code: string}
  | {kind: 'community'; content: string; sha256: string};

export type SharedPreview = {profile: Profile; custom_filters: number};

let catalog: CommunityProfile[] | undefined;

const isText = (value: unknown, max: number): value is string =>
  typeof value === 'string' && !!value.trim() && value.length <= max;

function parseCatalog(data: unknown): CommunityProfile[] {
  const invalid = '社区配置目录格式无效';
  if (!data || typeof data !== 'object') throw Error(invalid);
  const {version, profiles} = data as Record<string, unknown>;
  if (version !== 1 || !Array.isArray(profiles) || profiles.length > 500) throw Error(invalid);
  const codes = new Set<string>();
  return profiles.map((value: unknown) => {
    if (!value || typeof value !== 'object') throw Error(invalid);
    const entry = value as CommunityProfile;
    if (typeof entry.sha256 !== 'string' || !/^[a-f0-9]{64}$/.test(entry.sha256) ||
      entry.code !== 'MS-' + entry.sha256.slice(0, 8).toUpperCase() || codes.has(entry.code) ||
      !isText(entry.name, 120) || !isText(entry.game, 100) || !isText(entry.author, 100) ||
      !isText(entry.controller, 160) || !isText(entry.description, 600) ||
      typeof entry.notes !== 'string' || entry.notes.length > 1200 || typeof entry.verified !== 'boolean' ||
      !['none', 'xbox_360', 'dualshock_4'].includes(entry.output)) throw Error(invalid);
    codes.add(entry.code);
    return entry;
  });
}

async function fetchText(path: string, signal: AbortSignal): Promise<string> {
  signal.throwIfAborted();
  const request = new AbortController();
  const abort = () => request.abort();
  const timer = window.setTimeout(abort, 12000);
  signal.addEventListener('abort', abort, {once: true});
  try {
    const response = await fetch(catalogRoot + path, {
      signal: request.signal, credentials: 'omit', redirect: 'error', cache: 'no-cache',
    }).catch(() => { throw Error('无法连接 GitHub，请稍后重试'); });
    if (!response.ok || !response.body) throw Error('无法读取社区配置，请稍后重试');
    if (Number(response.headers.get('content-length')) > maxBytes) throw Error('社区配置文件过大');
    const reader = response.body.getReader();
    const decoder = new TextDecoder('utf-8', {fatal: true, ignoreBOM: true});
    const text: string[] = [];
    let bytes = 0;
    try {
      while (true) {
        const {done, value} = await reader.read().catch(() => { throw Error('无法读取社区配置，请稍后重试'); });
        if (done) break;
        bytes += value.length;
        if (bytes > maxBytes) throw Error('社区配置文件过大');
        text.push(decoder.decode(value, {stream: true}));
      }
      text.push(decoder.decode());
    } finally {
      void reader.cancel().catch(() => {});
    }
    return text.join('');
  } finally {
    window.clearTimeout(timer);
    signal.removeEventListener('abort', abort);
  }
}

export async function loadCommunityCatalog(signal: AbortSignal, refresh = false): Promise<CommunityProfile[]> {
  if (catalog && !refresh) return catalog;
  const text = await fetchText('index.json', signal);
  let data: unknown;
  try { data = JSON.parse(text); } catch { throw Error('社区配置目录格式无效'); }
  catalog = parseCatalog(data);
  return catalog;
}

export async function resolveCommunityCode(code: string, signal: AbortSignal): Promise<CommunityProfile> {
  const hadCatalog = !!catalog;
  const entries = await loadCommunityCatalog(signal);
  const normalized = code.toUpperCase();
  const entry = entries.find(item => item.code === normalized)
    ?? (hadCatalog ? (await loadCommunityCatalog(signal, true)).find(item => item.code === normalized) : undefined);
  if (!entry) throw Error('找不到这个社区配置码，请核对后重试');
  return entry;
}

export async function loadCommunityProfile(entry: CommunityProfile, signal: AbortSignal): Promise<SharedSource> {
  // Paths are derived from the digest in the catalog. Entries cannot supply URLs.
  const content = await fetchText('profiles/' + entry.sha256 + '.toml', signal);
  return {kind: 'community', content, sha256: entry.sha256};
}
