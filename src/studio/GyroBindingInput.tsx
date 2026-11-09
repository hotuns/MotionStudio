import {t} from '../i18n';
import type {GyroBindingField, GyroBindingRecording} from '../pad-capture';
import {OptionPicker} from './OptionPicker';
import {padChoices, padInputLabel} from './binding-choices';
import styles from './MotionControls.module.css';

export function GyroBindingInput({field, value, blocked, split, recording, locked, available, onChange, onRecord, onCancel}: {
  field: GyroBindingField;
  value: string | null;
  blocked: string | null;
  split: boolean;
  recording?: GyroBindingRecording;
  locked: boolean;
  available: boolean;
  onChange: (value: string) => void;
  onRecord: () => void;
  onCancel: () => void;
}) {
  const pause = field === 'pause_input';
  const label = pause ? '按住暂停' : '开启按键';
  const groups = padChoices(undefined, blocked ? [blocked] : [], split);
  if (pause) groups.unshift({options:[{value:'', label:'不设置'}]});
  return <div className={styles.bindingField}>
    <div className={styles.bindingRow}>
      <div className="studio-field">{label}
        <OptionPicker label={label} value={value ?? ''} groups={groups} disabled={locked} onChange={onChange}/>
      </div>
      <button type="button" className="secondary-btn" data-cancel-capture aria-pressed={!!recording}
        aria-label={recording ? '取消录入' : pause ? '录入暂停输入' : '录入开启输入'}
        disabled={!recording && (locked || !available)} onClick={recording ? onCancel : onRecord}>
        {recording ? '取消录入' : '监听手柄'}
      </button>
    </div>
    {recording && <p className={styles.hint} role="status">
      {recording.issue === 'duplicate' ? '开启和暂停不能使用同一个输入，请重新录入。' :
        recording.issue === 'multiple' ? '一次只录入一个体感输入，请重试。' :
        (recording.preview?.length ?? 0) > 1 ? '检测到多个输入，请全部松开。' :
        recording.waitingForNeutral ? '请先松开当前输入并让摇杆回中。' :
        recording.preview?.length ? <><strong translate="no">{t(padInputLabel(recording.preview[0], split))}</strong> · 松开后完成录入</> :
        '请按下或接触要录入的手柄输入。'}
      <br/>录入期间暂停游戏输出，Esc 可取消。
    </p>}
  </div>;
}
