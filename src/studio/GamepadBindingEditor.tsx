import {useState} from 'react';
import {t} from '../i18n';
import {chordParts, inputs, type GamepadBinding, type GamepadTarget, type Profile} from '../types';
import {hasAnalogCondition,isAnalogGamepadBinding,triggerPeers} from '../mapping-triggers';
import type {GamepadBindingRecording} from '../pad-capture';
import layout from '../virtual-gamepad-layout.json';
import {ActionTriggerFields,InputCaptureButton,InputChordFields,InputQuickKeys,quickInputKeys} from './BindingFields';
import {padInputLabel} from './binding-choices';
import {Dialog,Range,Toggle} from './Controls';
import styles from './ActionEditor.module.css';

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
  recording?: GamepadBindingRecording;
  canRecord: boolean;
  onRecord: (wholeStick: boolean, onInput: (input: string) => void) => void;
  onCancelRecording: () => void;
  onClose: () => void;
  onSave: (binding: GamepadBinding) => void;
};

export function GamepadBindingEditor({value, index, bindings, profile, target, split, locked, recording, canRecord,
  onRecord, onCancelRecording, onClose, onSave}: Props) {
  const [draft, setDraft] = useState(value);
  const [inputCombo, setInputCombo] = useState(() => chordParts(value.input).length > 1);
  const available = virtualGamepadControls(target);
  const parts = chordParts(draft.input);
  const stickInput = layout.sticks.includes(draft.input);
  const stickOutput = layout.sticks.includes(draft.output);
  const wholeStick = stickInput || stickOutput;
  const fieldsLocked = locked || !!recording;
  const analog = isAnalogGamepadBinding(draft);
  const reserved = new Set(bindings.flatMap((binding, i) =>
    i !== index && isAnalogGamepadOutput(binding.output) ? [binding.output] : []));
  const validMs = (ms:number|undefined|null,min:number,max:number) => ms==null || Number.isInteger(ms)&&ms>=min&&ms<=max;
  const threshold = draft.threshold ?? 0.5;
  const validInput = stickOutput ? stickInput : parts.length > 0 && parts.length <= 4 && parts.every(part => inputs.includes(part));
  const selected = available.includes(draft.output) && validInput;
  const valid = selected && !reserved.has(draft.output)
    && Number.isFinite(threshold) && threshold >= 0 && threshold < 1
    && validMs(draft.trigger_ms,10,5000) && validMs(draft.turbo_ms,20,2000) && validMs(draft.chord_wait_ms,0,500);
  const change = (patch:Partial<GamepadBinding>) => setDraft(current => {
    const next = {...current,...patch};
    return isAnalogGamepadBinding(next) ? {...next,trigger:'down',turbo_ms:null,allow_simultaneous:false} : next;
  });
  const chooseInput = (input: string) => change({
    input, output: !input || layout.sticks.includes(input) === stickOutput ? draft.output : '',
  });
  const chooseOutput = (output: string) => change({
    output, input: layout.sticks.includes(output) === stickInput ? draft.input : '',
  });
  const record = (wholeStick: boolean) => onRecord(wholeStick, input => {
    if (chordParts(input).length > 1) setInputCombo(true);
    chooseInput(input);
  });
  const inputName = (input: string) => layout.sticks.includes(input) ? gamepadControlLabel(input) : t(padInputLabel(input, split));
  const preview = recording?.preview?.length ? recording.preview : parts;
  const outputGroups = [
    {label: '手柄按键', controls: [...layout.buttons, ...layout.dualshock_4].filter(control => available.includes(control))},
    {label: '摇杆', controls: layout.sticks},
    {label: '扳机', controls: layout.triggers},
  ];
  return <Dialog
    title={index === null ? '添加手柄映射' : '编辑手柄映射'}
    className={styles.editorDialog}
    onClose={onClose}
    onSubmit={() => { if (valid && !fieldsLocked) onSave(draft); }}
    actions={<>
      <button type="button" className="secondary-btn" data-cancel-capture onClick={onClose}>取消</button>
      <button type="submit" className="primary-btn" disabled={!valid || fieldsLocked}>保存映射</button>
    </>}>
    <div className={styles.columns}>
      <section className={styles.panel} aria-label="手柄输入设置" data-pad-zone="binding-input">
        <div className={styles.heading}><h3>手柄输入</h3></div>
        <InputCaptureButton value={preview.map(inputName).join(' + ')} recording={!!recording}
          disabled={!recording && (locked || !canRecord)}
          hint={recording?.waitingForNeutral ? '请先松开当前输入并让摇杆回中。' :
            (recording?.wholeStick ?? wholeStick) ? '拨动要录入的摇杆，回中后完成。' : undefined}
          onClick={recording ? onCancelRecording : () => record(wholeStick)}/>
        {recording && <p className={styles.hint} role="status">录入期间暂停游戏输出，Esc 可取消。</p>}
        <fieldset className={styles.manual} disabled={fieldsLocked}>
          {parts.length > 0 && <div className={styles.parts} aria-label="已选手柄输入">
            {parts.map(input => {
              const label = inputName(input);
              return <button type="button" key={input} aria-label={'移除输入 '+label}
                onClick={() => chooseInput(parts.filter(part => part !== input).join('+'))}>
                {label} <span aria-hidden="true">×</span>
              </button>;
            })}
          </div>}
          <Toggle label="组合输入" value={inputCombo} onChange={setInputCombo}/>
          <InputQuickKeys value={stickInput ? '' : draft.input} combo={inputCombo} split={split} onChange={chooseInput}/>
          <div className={styles.sectionHeading}>实体摇杆</div>
          <div className={styles.inputKeys + ' ' + styles.pairedKeys} role="group" aria-label="实体摇杆">
            {layout.sticks.map(input => <button type="button" key={input} aria-pressed={draft.input === input}
              onClick={() => chooseInput(input)}>{gamepadControlLabel(input)}</button>)}
          </div>
          {!wholeStick && <button type="button" className="secondary-btn" disabled={!canRecord}
            onClick={() => record(true)}>监听摇杆</button>}
          <details className={styles.advanced} open={!stickInput && parts.some(part => !quickInputKeys.some(([key]) => key === part))}>
            <summary>完整输入与组合设置</summary>
            <InputChordFields value={stickInput ? '' : draft.input} split={split} showHint={false} onChange={input => {
              if (chordParts(input).length > 1) setInputCombo(true);
              chooseInput(input);
            }}/>
          </details>
          {selected && !analog && <details className={styles.advanced}>
            <summary>触发方式</summary>
            {hasAnalogCondition(draft.input) && <Range label="输入触发阈值" value={threshold} min={0} max={0.99} step={0.01}
              onChange={threshold => change({threshold})}/>}
            <ActionTriggerFields action={draft} onChange={change} peers={triggerPeers(profile,'gamepad',index)}/>
          </details>}
        </fieldset>
      </section>
      <section className={styles.panel} aria-label="虚拟手柄输出" data-pad-zone="binding-output">
        <div className={styles.heading}><h3>虚拟手柄输出</h3></div>
        <fieldset className={styles.manual} disabled={fieldsLocked}>
          {outputGroups.map(({label, controls}) => <div key={label} role="group" aria-label={label}>
            <div className={styles.sectionHeading}>{label}</div>
            <div className={[styles.inputKeys, controls.length === 2 && styles.pairedKeys].filter(Boolean).join(' ')}>
              {controls.map(output => {
                const name = gamepadControlLabel(output, target);
                const occupied = reserved.has(output);
                return <button type="button" key={output} aria-label={'输出 '+name} aria-pressed={draft.output === output}
                  title={occupied ? '同一个虚拟摇杆或扳机只能绑定一条映射' : name}
                  disabled={occupied} onClick={() => chooseOutput(output)}>{name}</button>;
              })}
            </div>
          </div>)}
          {selected && analog && <p className={styles.hint}>模拟量映射连续传递摇杆位置或扳机行程。轻拉、全拉与摇杆方向可作为数字输入单独设置触发方式。</p>}
        </fieldset>
      </section>
    </div>
  </Dialog>;
}
