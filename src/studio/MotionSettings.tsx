import {Range, Toggle} from './Controls';
import type {Device, Gyro, Profile} from '../types';
import styles from './MotionControls.module.css';
import {MotionBehavior, AccelerationSettings, StabilitySettings, MotionChoice, type GameCalibrationProps} from './AdvancedMotion';
import {CustomFilters} from './CustomFilters';
import type {GyroBindingField, GyroBindingRecording} from '../pad-capture';
import {GyroBindingInput} from './GyroBindingInput';
import {SettingField, SettingSection, settingStyles} from './SettingLayout';

type Update=(update:(profile:Profile)=>void)=>void;
const smoothingPresets=[['跟手',0],['均衡',16],['更稳',32]] as const;

export function MotionSettings({profile,profileId,update,device,recording,canRecord,onRecord,
  onCancelRecording,onOpenDevice,onPreviewOutput,status,onMotionAction,deviceNoise}: {
  profile:Profile;
  profileId:string;
  update:Update;
  device?:Device|null;
  deviceNoise?:number;
  recording?:GyroBindingRecording;
  canRecord:boolean;
  onRecord:(field:GyroBindingField)=>void;
  onCancelRecording:()=>void;
  onOpenDevice:()=>void;
  onPreviewOutput:()=>void;
}&Pick<GameCalibrationProps,'status'|'onMotionAction'>) {
  const gyro=profile.gyro;
  const setGyro=<K extends keyof Gyro>(key:K,value:Gyro[K])=>update(draft=>{draft.gyro[key]=value;});
  const activation=gyro.activation==='always'?'always':gyro.activation_mode;
  const bindingLocked=!!recording||!!status?.binding_capture||!!status?.motion?.measuring;
  const split=(device?.touchpads??0)>=2;
  const preset=gyro.motion.smoothing==='ema'?smoothingPresets.find(([,ms])=>ms===gyro.smoothing_ms):undefined;

  return <div className={settingStyles.page}>
    <SettingSection id="motion-main" title="体感瞄准" defaultOpen summary={gyro.enabled?<>{gyro.sensitivity}×</>:'关闭'}>
      <Toggle label="启用体感" value={gyro.enabled} onChange={value=>setGyro('enabled',value)}/>
      {gyro.enabled&&<>
        <SettingField label="开启和暂停" path={['gyro.activation','gyro.activation_mode','gyro.pause_input']} disabled={bindingLocked}>
          <div className={settingStyles.row}><span>开启方式</span>
            <div className={styles.activationModes} role="group" aria-label="开启方式">
              {([['always','始终开启'],['hold','按住开启'],['toggle','按一下切换开关']] as const).map(([mode,label])=>
                <button key={mode} type="button" className="secondary-btn" aria-pressed={mode===activation} disabled={bindingLocked}
                  onClick={()=>{
                    if(mode===activation)return;
                    update(draft=>{
                      draft.gyro.activation_mode=mode==='always'?'hold':mode;
                      if(mode==='always')draft.gyro.activation='always';
                      else if(draft.gyro.activation==='always')draft.gyro.activation=draft.gyro.pause_input==='left_trigger'?'left_shoulder':'left_trigger';
                    });
                  }}>{label}</button>)}
            </div>
          </div>
        {activation!=='always'&&<GyroBindingInput field="activation" value={gyro.activation} blocked={gyro.pause_input}
          split={split} available={canRecord} locked={bindingLocked} recording={recording?.gyroField==='activation'?recording:undefined}
          onChange={value=>setGyro('activation',value)} onRecord={()=>onRecord('activation')} onCancel={onCancelRecording}/>}
        <GyroBindingInput field="pause_input" value={gyro.pause_input} blocked={gyro.activation}
          split={split} available={canRecord} locked={bindingLocked} recording={recording?.gyroField==='pause_input'?recording:undefined}
          onChange={value=>setGyro('pause_input',value||null)} onRecord={()=>onRecord('pause_input')} onCancel={onCancelRecording}/>
        </SettingField>
        {(activation!=='always'||gyro.pause_input)&&<Range label="扳机与摇杆阈值" value={gyro.activation_threshold}
          min={0} max={0.95} step={0.05} onChange={value=>setGyro('activation_threshold',value)}/>}
        <Range label="体感灵敏度" value={gyro.sensitivity} min={0.1} max={10} step={0.1} unit="×"
          onChange={value=>setGyro('sensitivity',value)} help="转得太慢就调高，容易转过头就调低。"/>
        <Range label="垂直灵敏度比例" value={gyro.vertical_ratio} min={0.1} max={2} step={0.05} unit="×"
          onChange={value=>setGyro('vertical_ratio',value)}/>
        <div className={settingStyles.row}><span>防抖预设</span><div className={styles.activationModes} role="group" aria-label="防抖预设">
          {smoothingPresets.map(([label,ms])=><button key={ms} type="button" className="secondary-btn" aria-pressed={preset?.[1]===ms}
            onClick={()=>update(draft=>{draft.gyro.motion.smoothing='ema';draft.gyro.smoothing_ms=ms;})}>{label}</button>)}
        </div></div>
      </>}
      <div className="studio-actions"><button type="button" className="secondary-btn" onClick={onPreviewOutput}>查看输出预览</button></div>
    </SettingSection>
    {gyro.enabled&&<><StabilitySettings profile={profile} update={update} onOpenDevice={onOpenDevice} deviceNoise={deviceNoise}/>
    <AccelerationSettings profile={profile} update={update}/>
    <MotionBehavior profile={profile} update={update}/>
    <SettingSection id="motion-filters" title="采样与扩展滤波" summary={<>{gyro.motion.custom_filters?.length??0} 个滤波器</>} lazy>
      <MotionChoice label="采样时钟" value={gyro.timestamp_source}
        options={[["host","SDL 事件时间戳（推荐）"],["sensor","传感器时间戳"]]}
        onChange={value=>setGyro('timestamp_source',value as Gyro['timestamp_source'])}/>
      <SettingField label="自定义滤波" path="gyro.motion.custom_filters">
        <CustomFilters key={profileId} profile={profile} update={update} embedded
          running={status?.profile_id===profileId&&['running','observing'].includes(status.state)}
          statuses={status?.profile_id===profileId?status.motion?.filters:undefined}
          onReset={()=>onMotionAction('reset_filters')}/>
      </SettingField>
    </SettingSection>
    </>}
  </div>;
}

