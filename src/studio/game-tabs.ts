import type { StudioIconName } from './StudioIcon';

export const gameTabs = [
  { id: 'actions', label: '动作映射', shortcut: '动作映射', icon: 'keybinds' },
  { id: 'motion', label: '体感', shortcut: '体感调校', icon: 'gyro' },
  { id: 'sticks', label: '摇杆', shortcut: '摇杆设置', icon: 'games' },
  { id: 'triggers', label: '扳机', shortcut: '扳机设置', icon: 'controllerStatus' },
  { id: 'touch', label: '触控', shortcut: '触控设置', icon: 'touchpad' },
] as const satisfies readonly { id: string; label: string; shortcut: string; icon: StudioIconName }[];

export type GameTab = (typeof gameTabs)[number]['id'];
