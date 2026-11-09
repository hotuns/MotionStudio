import {keyboardKeys} from '../keyboard';
import {inputs, labels, mouseOutputs, outputLabel, triggerStageInputs, wheelOutputs} from '../types';
import type {PickerGroup} from './OptionPicker';

const rings = ['left_stick_inner', 'left_stick_outer', 'right_stick_inner', 'right_stick_outer'];
const directions = [
  'left_stick_up', 'left_stick_down', 'left_stick_left', 'left_stick_right',
  'right_stick_up', 'right_stick_down', 'right_stick_left', 'right_stick_right',
];
const triggers = ['left_trigger', 'right_trigger', ...triggerStageInputs];

export const padInputLabel = (input: string, split = false) =>
  split && input === 'touchpad' ? '触摸板 1 按下' :
  split && input === 'misc2' ? '触摸板 2 按下' : labels[input] ?? input;

export function padChoices(allowed = inputs, blocked: string[] = [], split = false): PickerGroup[] {
  const touchpad = split ? ['touchpad', 'misc2', 'touch1', 'touch2'] : ['touchpad', 'touch1', 'touch2'];
  const grouped = new Set([...rings, ...directions, ...triggers, ...touchpad]);
  const aliases: Record<string, string> = {left_stick_outer:'左外圈 LS outer ring', right_stick_outer:'右外圈 RS outer ring'};
  const make = (label: string, values: string[]) => ({label, options:values.filter(input => allowed.includes(input)).map(value => ({
    value, label:padInputLabel(value, split), search:aliases[value], disabled:blocked.includes(value),
  }))});
  return [
    make('手柄按键', allowed.filter(input => !grouped.has(input))),
    make('触摸板', touchpad), make('摇杆内外圈', rings),
    make('扳机', triggers), make('摇杆方向', directions),
  ];
}

export function keyboardChoices(allowed: string[], prefix = '', blocked: string[] = []): PickerGroup[] {
  const groups = [['typing','主键区'], ['function','功能键'], ['navigation','导航与扩展键'],
    ['numpad','数字小键盘'], ['modifier','修饰键']];
  return groups.map(([group,label]) => ({label, options:keyboardKeys
    .filter(key => key.group === group && allowed.includes(key.name))
    .map(key => ({value:prefix + key.name, label:key.label, search:key.code, disabled:blocked.includes(prefix + key.name)})),
  }));
}

export function outputChoices(includeModifiers = true, blocked: string[] = []): PickerGroup[] {
  const mouse = (values: string[]) => values.map(value => ({value, label:outputLabel(value), disabled:blocked.includes(value)}));
  return [
    {label:'鼠标', options:mouse(mouseOutputs)}, {label:'鼠标滚轮', options:mouse(wheelOutputs)},
    ...keyboardChoices(keyboardKeys.filter(key => includeModifiers || !key.modifier).map(key => key.name), 'key:', blocked),
  ];
}
