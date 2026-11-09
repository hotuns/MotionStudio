import {useState} from 'react';
import {t} from '../i18n';
import {inputLabel, outputLabel, type MotionBinding, type TurnBinding} from '../types';
import {Dialog, Range} from './Controls';
import {OptionPicker} from './OptionPicker';
import {outputChoices, padChoices} from './binding-choices';
import {CurveEditor} from './CurveEditor';
import styles from './MotionControls.module.css';

const imuOptions:[string,string,number][]=[
  ['imu_tilt_left','向左倾斜',45],['imu_tilt_right','向右倾斜',45],['imu_tilt_up','抬高手柄前端',45],['imu_tilt_down','压低手柄前端',45],
  ['imu_tilt_pitch','俯仰角（双向）',45],['imu_tilt_roll','横滚角（双向）',45],
  ['imu_gyro_pitch','俯仰转速（双向）',360],['imu_gyro_pitch_up','向上转动',360],['imu_gyro_pitch_down','向下转动',360],
  ['imu_gyro_yaw','水平转速（双向）',360],['imu_gyro_yaw_left','向左转动',360],['imu_gyro_yaw_right','向右转动',360],
  ['imu_gyro_roll','横滚转速（双向）',360],['imu_gyro_roll_left','向左横滚',360],['imu_gyro_roll_right','向右横滚',360],
  ['imu_accel_ud','上下加速度（双向）',1],['imu_accel_up','向上加速',1],['imu_accel_down','向下加速',1],
  ['imu_accel_lr','左右加速度（双向）',1],['imu_accel_left','向左加速',1],['imu_accel_right','向右加速',1],
  ['imu_accel_fb','前后加速度（双向）',1],['imu_accel_forward','向前加速',1],['imu_accel_backward','向后加速',1],
];
const padOutputs=[['left_stick_x','左摇杆水平'],['left_stick_y','左摇杆垂直'],['right_stick_x','右摇杆水平'],['right_stick_y','右摇杆垂直'],['left_trigger','左扳机'],['right_trigger','右扳机'],['btn_south','A / 南键'],['btn_east','B / 东键'],['btn_west','X / 西键'],['btn_north','Y / 北键'],['btn_start','Menu'],['btn_back','View'],['btn_ls','左摇杆按下'],['btn_rs','右摇杆按下'],['btn_lb','左肩键'],['btn_rb','右肩键'],['btn_guide','主菜单'],['dpad_up','方向键上'],['dpad_down','方向键下'],['dpad_left','方向键左'],['dpad_right','方向键右']];

const turnOutputs = [
  {value:'mouse',label:'鼠标'},
  {value:'right_stick',label:'虚拟右摇杆'},
  {value:'both',label:'鼠标和虚拟右摇杆'},
];

export type MotionAction = {kind:'motion'; value:MotionBinding} | {kind:'turn'; value:TurnBinding};

export function motionActionLabels(action: MotionAction) {
  if (action.kind === 'turn') {
    return {input:inputLabel(action.value.input),
      output:t(turnOutputs.find(option=>option.value===action.value.output)?.label ?? action.value.output),
      tag:t('定角转身')};
  }
  const {input, output} = action.value;
  const pin = output.startsWith('pad:') && padOutputs.find(([id])=>id===output.slice(4));
  return {input:t(imuOptions.find(([id])=>id===input)?.[1] ?? input),
    output:pin ? t(pin[1]) : outputLabel(output), tag:t('体感动作')};
}

export function newMotionAction(kind: MotionAction['kind']): MotionAction {
  return kind === 'motion'
    ? {kind, value:{name:t('倾斜动作'),input:'imu_tilt_left',output:'key:Q',range:45,threshold:0.4,mode:'hold',turbo_ms:100,curve:[[0,0],[1,1]]}}
    : {kind, value:{name:t('转身'),input:'right_stick_click',degrees:180,duration_ms:100,output:'mouse'}};
}

function MotionFields({action, onChange}: {action: MotionBinding; onChange: (patch: Partial<MotionBinding>)=>void}) {
  return <>
    <div className="studio-field">体感输入
      <OptionPicker label="体感输入" value={action.input}
        groups={[{options:imuOptions.map(([value,label])=>({value,label}))}]}
        onChange={input=>onChange({input,range:imuOptions.find(([id])=>id===input)![2]})}/>
    </div>
    <div className="studio-field">映射输出
      <OptionPicker label="映射输出" value={action.output}
        groups={[...outputChoices(),{label:'虚拟手柄',options:padOutputs.map(([id,label])=>({value:'pad:'+id,label}))}]}
        onChange={output=>onChange({output})}/>
    </div>
    <div className="studio-field">触发方式
      <OptionPicker label="触发方式" value={action.mode} groups={[{options:[
        {value:'hold',label:'保持'},{value:'toggle',label:'越过阈值时切换'},{value:'turbo',label:'按住连发'},
      ]}]} onChange={mode=>onChange({mode:mode as MotionBinding['mode']})}/>
    </div>
    <Range label="达到满量程的输入" value={action.range} min={0.01}
      max={action.input.startsWith('imu_tilt')?90:action.input.startsWith('imu_accel')?8:4000}
      step={action.input.startsWith('imu_accel')?0.05:1}
      unit={action.input.startsWith('imu_tilt')?'°':action.input.startsWith('imu_accel')?' g':' °/s'}
      onChange={range=>onChange({range})}/>
    <Range label="触发阈值（满量程比例）" value={action.threshold} min={0} max={0.99} step={0.01}
      onChange={threshold=>onChange({threshold})}/>
    {action.mode==='turbo' && <Range label="连发周期" value={action.turbo_ms} min={20} max={2000} step={10} unit=" ms"
      onChange={turbo_ms=>onChange({turbo_ms})}/>}
    <details><summary>动作响应曲线</summary>
      <CurveEditor points={action.curve} onChange={curve=>onChange({curve})} maxX={1} maxY={1}/>
    </details>
  </>;
}

function TurnFields({action, onChange}: {action: TurnBinding; onChange: (patch: Partial<TurnBinding>)=>void}) {
  return <>
    <div className="studio-field">转身按键
      <OptionPicker label="转身按键" value={action.input} groups={padChoices()} onChange={input=>onChange({input})}/>
    </div>
    <div className="studio-field">转身输出
      <OptionPicker label="转身输出" value={action.output} groups={[{options:turnOutputs}]}
        onChange={output=>onChange({output:output as TurnBinding['output']})}/>
    </div>
    <Range label="转身角度（正数向右）" value={action.degrees} min={-720} max={720} step={1} unit="°"
      onChange={degrees=>onChange({degrees})}/>
    <Range label="完成时间" value={action.duration_ms} min={0} max={5000} step={1} unit=" ms"
      onChange={duration_ms=>onChange({duration_ms})}/>
  </>;
}

export function MotionActionEditor({action, onClose, onSave, blocked}: {
  action: MotionAction;
  onClose: () => void;
  onSave: (action: MotionAction) => void;
  blocked: boolean;
}) {
  const [draft, setDraft] = useState<MotionAction>(()=>structuredClone(action));
  const within = (value: number, min: number, max: number) => value >= min && value <= max;
  const ready = !!draft.value.name.trim() && !!draft.value.input && (draft.kind==='motion'
    ? !!draft.value.output && within(draft.value.range,0.01,4000)
      && within(draft.value.threshold,0,0.99) && within(draft.value.turbo_ms,20,2000)
    : within(draft.value.degrees,-720,720) && within(draft.value.duration_ms,0,5000));

  return <Dialog title={draft.kind==='motion'?'设置体感动作':'设置定角转身'}
    description="动作在体感开启后生效，暂停体感时会停止输出。"
    onClose={onClose} onSubmit={()=>{if (ready && !blocked) onSave(draft);}}
    actions={<>
      <button type="button" className="secondary-btn" onClick={onClose}>取消</button>
      <button type="submit" formNoValidate className="primary-btn" disabled={!ready || blocked}>完成</button>
    </>}>
    <fieldset className="studio-content" disabled={blocked}>
      <label className="studio-field">动作名称
        <input autoFocus maxLength={100} value={draft.value.name}
          onChange={event=>{
            const name=event.target.value;
            setDraft(current=>current.kind==='motion'
              ? {kind:'motion',value:{...current.value,name}}
              : {kind:'turn',value:{...current.value,name}});
          }}/>
      </label>
      <div className={styles.panel}>
        {draft.kind==='motion'
          ? <MotionFields action={draft.value} onChange={patch=>setDraft({kind:'motion',value:{...draft.value,...patch}})}/>
          : <TurnFields action={draft.value} onChange={patch=>setDraft({kind:'turn',value:{...draft.value,...patch}})}/>}
      </div>
    </fieldset>
  </Dialog>;
}
