import {useState} from 'react';
import {t} from '../i18n';
import {chordParts, type GamepadBinding, type GamepadTarget, type Profile} from '../types';
import {hasAnalogCondition,isAnalogGamepadBinding,triggerPeers} from '../mapping-triggers';
import layout from '../virtual-gamepad-layout.json';
import {ActionTriggerFields,InputChordFields} from './BindingFields';
import {padInputLabel} from './binding-choices';
import {Dialog,Range} from './Controls';
import {OptionPicker} from './OptionPicker';

export const maxGamepadBindings = 64;

export function virtualGamepadControls(target: GamepadTarget) {
  return target === 'none' ? [] : [
    ...layout.buttons,
    ...layout.sticks,
    ...layout.triggers,
    ...(target === 'dualshock_4' ? layout.dualshock_4 : []),
  ];
}

const dualShockLabels: Record<string, string> = {
  south: '×', east: '○', west: '□', north: '△', back: 'Share', start: 'Options',
  guide: 'PS', left_shoulder: 'L1', right_shoulder: 'R1', left_trigger: 'L2',
  right_trigger: 'R2', left_stick_click: 'L3', right_stick_click: 'R3',
};

export function gamepadControlLabel(control: string, target?: GamepadTarget) {
  if (target === 'dualshock_4' && dualShockLabels[control]) return dualShockLabels[control];
  if (control === 'left_stick') return t('左摇杆');
  if (control === 'right_stick') return t('右摇杆');
  return t(padInputLabel(control));
}

export function isAnalogGamepadOutput(control: string) {
  return layout.sticks.includes(control) || layout.triggers.includes(control);
}

export function fillStandardGamepadBindings(target: GamepadTarget, current: GamepadBinding[]) {
  const controls = virtualGamepadControls(target);
  const active = current.filter(binding => controls.includes(binding.output));
  const inputs = new Set(active.flatMap(binding => chordParts(binding.input)));
  const outputs = new Set(active.map(binding => binding.output));
  const additions = controls
    .filter(control => !inputs.has(control) && !outputs.has(control))
    .slice(0, Math.max(0, maxGamepadBindings - current.length))
    .map(control => ({input: control, output: control}));
  return [...current, ...additions];
}

type Props = {
  value: GamepadBinding;
  index: number | null;
  bindings: GamepadBinding[];
  profile: Profile;
  target: GamepadTarget;
  split: boolean;
  locked: boolean;
  onClose: () => void;
  onSave: (binding: GamepadBinding) => void;
};

export function GamepadBindingEditor({value, index, bindings, profile, target, split, locked, onClose, onSave}: Props) {
  const [draft, setDraft] = useState(value);
  const available = virtualGamepadControls(target);
  const isStick = layout.sticks.includes(draft.output);
  const analog = isAnalogGamepadBinding(draft);
  const reserved = new Set(bindings.flatMap((binding, i) =>
    i !== index && isAnalogGamepadOutput(binding.output) ? [binding.output] : []));
  const validMs = (ms:number|undefined|null,min:number,max:number) => ms==null || Number.isInteger(ms)&&ms>=min&&ms<=max;
  const threshold = draft.threshold ?? 0.5;
  const valid = available.includes(draft.output) && !reserved.has(draft.output)
    && (isStick ? layout.sticks.includes(draft.input) : chordParts(draft.input).length > 0 && chordParts(draft.input).length <= 4)
    && Number.isFinite(threshold) && threshold >= 0 && threshold < 1
    && validMs(draft.trigger_ms,10,5000) && validMs(draft.turbo_ms,20,2000);
  const change = (patch:Partial<GamepadBinding>) => setDraft(current => {
    const next = {...current,...patch};
    return isAnalogGamepadBinding(next) ? {...next,trigger:'down',turbo_ms:null,allow_simultaneous:false} : next;
  });
  const chooseOutput = (output: string) => change({
    output, input: layout.sticks.includes(output) === layout.sticks.includes(draft.input) ? draft.input : '',
  });
  return <Dialog
    title={index === null ? '添加手柄映射' : '编辑手柄映射'}
    onClose={onClose}
    onSubmit={() => { if (valid && !locked) onSave(draft); }}
    actions={<>
      <button type="button" className="secondary-btn" onClick={onClose}>取消</button>
      <button type="submit" className="primary-btn" disabled={!valid || locked}>保存映射</button>
    </>}>
    <fieldset className="studio-content" disabled={locked}>
      <label className="studio-field">虚拟手柄输出
        <select aria-label="虚拟手柄输出" value={draft.output} onChange={event=>chooseOutput(event.target.value)}>
          <option value="" disabled>选择虚拟手柄输出</option>
          {available.map(output=><option key={output} value={output} disabled={reserved.has(output)}>{gamepadControlLabel(output,target)}</option>)}
        </select>
      </label>
      {isStick
        ? <div className="studio-field">实体摇杆
            <OptionPicker label="实体摇杆" placeholder="选择实体摇杆" value={draft.input}
              groups={[{options: layout.sticks.map(input => ({value: input, label: gamepadControlLabel(input)}))}]}
              onChange={input => change({input})}/>
          </div>
        : <InputChordFields value={draft.input} split={split} showHint={false}
            onChange={input => change({input})}/>}
      {analog ? <p className="studio-muted">模拟量映射连续传递摇杆位置或扳机行程。轻拉、全拉与摇杆方向可作为数字输入单独设置触发方式。</p> : <>
        {hasAnalogCondition(draft.input)&&<Range label="输入触发阈值" value={threshold} min={0} max={0.99} step={0.01}
          onChange={threshold=>change({threshold})}/>}
        <ActionTriggerFields action={draft} onChange={change} peers={triggerPeers(profile,'gamepad',index)}/>
      </>}
    </fieldset>
  </Dialog>;
}
