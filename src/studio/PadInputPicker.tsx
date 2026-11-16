import {createContext, useContext, useEffect, useId, useRef, type ReactNode} from 'react';
import {t} from '../i18n';
import type {GamepadBindingRecording} from '../pad-capture';
import {OptionPicker, type PickerGroup} from './OptionPicker';
import {padInputLabel} from './binding-choices';
import styles from './PadInputPicker.module.css';

export type PadPickerRequest = {controlId: string; allowedInputs: string[]; onGamepadInput: (input: string) => void};
export const PadInputCaptureContext = createContext<{
  available: boolean;
  locked: boolean;
  split: boolean;
  recording?: GamepadBindingRecording & {controlId?: string};
  start: (request: PadPickerRequest) => void;
  cancel: (controlId: string) => void;
} | null>(null);

export function PadInputRow({children, recording, disabled, starting = false, onRecord,
  recordLabel = '监听手柄输入', cancelLabel = '取消录入'}: {
  children: ReactNode;
  recording: boolean;
  disabled: boolean;
  starting?: boolean;
  onRecord: () => void;
  recordLabel?: string;
  cancelLabel?: string;
}) {
  return <div className={styles.row} data-pad-input-row>
    {children}
    <button type="button" className="secondary-btn" data-cancel-capture aria-pressed={recording}
      aria-label={recording ? cancelLabel : recordLabel} disabled={disabled} onClick={onRecord}>
      {starting ? '正在准备…' : recording ? '取消录入' : '监听手柄输入'}
    </button>
  </div>;
}

export function PadInputValue({label, value}: {label: string; value: string}) {
  return <input className={styles.value} aria-label={label} value={value} readOnly
    placeholder={t('选择手柄输入')} title={value} translate="no"/>;
}

export function PadInputPicker({label, value, groups, onChange, disabled = false, placeholder, className}: {
  label: string;
  value: string | null;
  groups: PickerGroup[];
  onChange: (value: string) => void;
  disabled?: boolean;
  placeholder?: string;
  className?: string;
}) {
  const capture = useContext(PadInputCaptureContext);
  const controlId = useId();
  const latestChange = useRef(onChange);
  latestChange.current = onChange;
  const cancel = capture?.cancel;
  useEffect(() => () => cancel?.(controlId), [cancel, controlId]);
  const recording = capture?.recording?.controlId === controlId ? capture.recording : undefined;
  const locked = disabled || !!capture?.locked;
  return <div className={[styles.field, className].filter(Boolean).join(' ')}>
    <PadInputRow recording={!!recording} disabled={!recording && (locked || !capture?.available)}
      recordLabel={t('监听手柄输入：{0}').replace('{0}', t(label))}
      cancelLabel={t('取消监听：{0}').replace('{0}', t(label))}
      onRecord={() => {
        if (recording) cancel?.(controlId);
        else capture?.start({controlId,
          allowedInputs:groups.flatMap(group => group.options.filter(option => !option.disabled && option.value).map(option => option.value)),
          onGamepadInput:input => latestChange.current(input),
        });
      }}>
      <OptionPicker label={label} value={value} groups={groups} placeholder={placeholder} disabled={locked} onChange={onChange}/>
    </PadInputRow>
    {recording && <p className={styles.hint} role="status">
      {recording.waitingForNeutral ? '请先松开当前输入并让摇杆回中。' :
        recording.preview?.length ? <><strong translate="no">{recording.preview.map(input => t(padInputLabel(input, capture?.split))).join(' + ')}</strong> · 松开后完成录入</> :
        '请按下或接触要录入的手柄输入。'}
      <br/>录入期间暂停游戏输出，Esc 可取消。
    </p>}
  </div>;
}
