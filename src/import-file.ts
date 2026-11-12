import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { t } from './i18n';

function decodeConfig(bytes: Uint8Array) {
  let encoding = 'UTF-8';
  let text: string;
  if (bytes[0] === 255 && bytes[1] === 254) {
    encoding = 'UTF-16 LE';
    text = new TextDecoder('utf-16le', { fatal: true }).decode(bytes);
  } else if (bytes[0] === 254 && bytes[1] === 255) {
    encoding = 'UTF-16 BE';
    text = new TextDecoder('utf-16be', { fatal: true }).decode(bytes);
  } else {
    try {
      text = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
    } catch {
      encoding = 'GB18030';
      text = new TextDecoder('gb18030', { fatal: true }).decode(bytes);
    }
  }
  if (text.includes('\0')) throw new Error('请选择文本配置文件。');
  return { text, encoding };
}

export async function pickImportFile(kind: 'toml' | 'jsm' | 'jsm-related') {
  const title = t(kind === 'toml' ? '选择 Motion Studio 配置' : '选择 JSM 配置文件');
  const extensions = kind === 'toml' ? ['toml'] : ['txt', 'jsm', 'cfg', 'conf'];
  if (kind === 'jsm') extensions.push('toml');
  const path = await open({ title, filters: [{ name: t('配置文件列表'), extensions }] });
  if (path === null) return null;
  const { name, bytes } = await invoke<{ name: string; bytes: number[] }>('read_import_file', { path });
  return { name, ...decodeConfig(new Uint8Array(bytes)) };
}
