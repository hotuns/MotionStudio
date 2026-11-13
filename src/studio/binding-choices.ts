import {keyboardKeys} from '../keyboard';
import {inputs, mouseOutputs, outputLabel, wheelOutputs} from '../types';
import type {PickerGroup} from './OptionPicker';

import {controllerGroups, padInputLabel, motionPadOutputs} from './controller-catalog';
export {padInputLabel} from './controller-catalog';

export function padChoices(allowed = inputs, blocked: string[] = [], split = false): PickerGroup[] {
  const aliases: Record<string, string> = {left_stick_outer:'左外圈 LS outer ring', right_stick_outer:'右外圈 RS outer ring'};
  return controllerGroups(allowed, split).map(({label, values}) => ({label,
    options:values.map(value => ({value, label:padInputLabel(value, split),
      search:aliases[value], disabled:blocked.includes(value)})),
  }));
}

export function motionPadChoices(): PickerGroup[] {
  return padChoices(Object.values(motionPadOutputs)).map(group => ({
    label:'虚拟手柄 · '+group.label,
    options:group.options.map(option => ({...option,
      value:'pad:'+Object.keys(motionPadOutputs).find(pin => motionPadOutputs[pin] === option.value),
    })),
  }));
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
