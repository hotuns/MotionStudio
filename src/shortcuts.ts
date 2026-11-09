import {t} from './i18n';
import {keyboardKeyFromEvent, keyboardKeys} from './keyboard';
import type {Shortcut} from './types';

const byName = new Map(keyboardKeys.map(key => [key.name, key]));
export const shortcutKeys = keyboardKeys.map(key => key.name).filter(name => !['WIN', 'RWIN', 'NUMPADENTER'].includes(name));
export const defaultShortcut = (): Shortcut => ({key:'F9', ctrl:false, alt:false, shift:false});

export function shortcutLabel(shortcut: Shortcut | null = defaultShortcut()) {
  if (!shortcut) return t('未绑定');
  const {key, ctrl, alt, shift} = shortcut;
  return [...(ctrl ? ['Ctrl'] : []), ...(alt ? ['Alt'] : []), ...(shift ? ['Shift'] : []), t(byName.get(key)?.label ?? key)].join(' + ');
}

export const shortcutModifier = (key: string) => byName.get(key)?.modifier ?? 0;

export function shortcutFromEvent(event: KeyboardEvent): {shortcut?: Shortcut; error?: string} {
  if (event.metaKey || event.key === 'Meta') return {error:'请选择不含 Windows 键的组合键。'};
  if (event.key === 'F12' || event.code === 'F12') return {error:'F12 保留用于紧急停止，请选择其他按键。'};
  if (event.isComposing) return {error:'请先结束输入法输入，再录入快捷键。'};
  const name = /^[a-z]$/i.test(event.key) ? event.key.toUpperCase() : keyboardKeyFromEvent(event);
  const key = name === 'NUMPADENTER' ? 'ENTER' : name;
  if (!shortcutKeys.includes(key)) return {error:'无法录入此按键，请从按键列表选择。'};
  const modifier = shortcutModifier(key);
  const ctrl = event.ctrlKey && !(modifier & 0x11);
  const alt = event.altKey && !(modifier & 0x44);
  const shift = event.shiftKey && !(modifier & 0x22);
  if (ctrl && !alt && ['S', 'K', 'ENTER'].includes(key)) return {error:'此组合已用于保存、搜索或应用配置，请选择其他组合。'};
  if ((alt && !ctrl && ['F4', 'TAB'].includes(key)) || (ctrl && alt && key === 'DELETE')) return {error:'此组合已由 Windows 使用，请选择其他快捷键。'};
  return {shortcut:{key, ctrl, alt, shift}};
}
