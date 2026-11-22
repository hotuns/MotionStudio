import {isSensorOutput,sensorOutputLabel} from '../sensor-passthrough';
import {memo} from 'react';
import {Range, Toggle} from './Controls';
import {applyJsmResponse,isJsmResponse,nativeSmoothing,usesJsmAim} from '../motion-presets';
import type {ImuFilter, FilterStatus, Gyro, Profile} from '../types';
import {useLanguage} from '../i18n';
import styles from './MotionControls.module.css';
import {MotionBehavior, AccelerationSettings, StabilitySettings, MotionChoice} from './AdvancedMotion';
import {CustomFilters} from './CustomFilters';
import type {GyroBindingField, GyroBindingRecording} from '../pad-capture';
import {GyroBindingInput} from './GyroBindingInput';
import {SettingField, SettingLabel, SettingSection, settingStyles} from './SettingLayout';

type Update=(update:(profile:Profile)=>void)=>void;
const smoothingPresets=[['线性',0],['均衡',16],['更稳',32]] as const;

export const MotionSettings = memo(function MotionSettings({profile,profileId,update,split,missingImu,recording,canRecord,onRecord,
  bindingLocked,onCancelRecording,onOpenDevice,onPreviewOutput,filtersRunning,filterStatuses,onMotionAction,deviceNoise}: {
  profile:Profile;
  profileId:string;
  update:Update;
  split:boolean;
  missingImu:boolean;
  deviceNoise?:number;
  recording?:GyroBindingRecording;
  canRecord:boolean;
  bindingLocked:boolean;
  onRecord:(field:GyroBindingField)=>void;
  onCancelRecording:()=>void;
  onOpenDevice:()=>void;
  onPreviewOutput:()=>void;
  filtersRunning:boolean;
  filterStatuses?:FilterStatus[];
  onMotionAction:(action:string)=>void;
}) {
  useLanguage();
  const gyro=profile.gyro;
  const output=gyro.output;
  const passthrough=isSensorOutput(output);
  const jsm=usesJsmAim(gyro);
  const sensorFilter=gyro.ds4_imu_filter;
  const setGyro=<K extends keyof Gyro>(key:K,value:Gyro[K])=>update(draft=>{draft.gyro[key]=value;});
  const setSensorFilter=(change:ImuFilter)=>update(draft=>{
    draft.gyro.ds4_imu_filter={...draft.gyro.ds4_imu_filter,...change};
  });
  const activation=gyro.activation==='always'?'always':gyro.activation_mode;
  const smooth=nativeSmoothing(gyro);
  const preset=smoothingPresets.find(([,ms])=>ms===0?isJsmResponse(gyro):jsm
    ?smooth.window===ms&&smooth.threshold===60&&!smooth.decay&&!smooth.oneEuro
    :gyro.motion.smoothing==='ema'&&gyro.smoothing_ms===ms);

  return <div className={settingStyles.page}>
    <SettingSection id="motion-main" title={passthrough?sensorOutputLabel(output):'体感瞄准'} defaultOpen
      summary={gyro.enabled?(passthrough?'已启用':<>{gyro.sensitivity}×</>):'关闭'}>
      <Toggle label="启用体感" value={gyro.enabled} onChange={value=>setGyro('enabled',value)}
        help={passthrough?'按所选开启方式，将设备校准后的传感器数据发送给虚拟手柄。':undefined}/>
      {gyro.enabled&&<>
        <SettingField label="开启和暂停" path={['gyro.activation','gyro.activation_mode','gyro.pause_input']} disabled={bindingLocked}>
          <div className={settingStyles.row}><SettingLabel label="开启方式"/>
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
        {!passthrough&&<><Range label="体感灵敏度" value={gyro.sensitivity} min={0.1} max={10} step={0.1} unit="×"
          onChange={value=>setGyro('sensitivity',value)} help="转得太慢就调高，容易转过头就调低。"/>
        <Range label="垂直灵敏度比例" value={gyro.vertical_ratio} min={0.1} max={2} step={0.05} unit="×"
          onChange={value=>setGyro('vertical_ratio',value)}/>
        <div className={settingStyles.row}><span>防抖预设</span><div className={styles.activationModes} role="group" aria-label="防抖预设">
          {smoothingPresets.map(([label,ms])=><button key={ms} type="button" className="secondary-btn" aria-pressed={preset?.[1]===ms}
            onClick={()=>update(draft=>{
              if(ms===0)applyJsmResponse(draft.gyro);
              else if(jsm)draft.gyro.jsm_response={...draft.gyro.jsm_response,smooth_time_ms:ms,smooth_threshold_dps:60,decay_smoothing:false,one_euro:false};
              else {draft.gyro.motion.smoothing='ema';draft.gyro.smoothing_ms=ms;}
            })}>{label}</button>)}
        </div></div>
        <p className={styles.hint}>线性预设使用固定灵敏度，并关闭平滑、低速抑制、吸附和刹车。</p>
        {jsm&&gyro.motion.edge.mode!=='off'&&
          <p className={settingStyles.notice}>此配置的边缘转向设置仅在旧版角速度瞄准中生效。</p>}
        </>}
      </>}
      {passthrough?<>
        <p className={styles.hint}>使用同时支持陀螺仪和加速度计的实体手柄。设备静止校准和安装方向可在「设备与输入」调整。</p>
        {missingImu&&<p className={settingStyles.notice}>传感器直通需要同时支持陀螺仪和加速度计的手柄接口</p>}
        <SampleClock value={gyro.timestamp_source} onChange={value=>setGyro('timestamp_source',value)}/>
        <div className="studio-actions"><button type="button" className="secondary-btn" onClick={onOpenDevice}>打开设备校准</button></div>
      </>:<div className="studio-actions"><button type="button" className="secondary-btn" onClick={onPreviewOutput}>查看输出预览</button></div>}
    </SettingSection>
    {gyro.enabled&&<>
      {!passthrough&&<>
        <StabilitySettings profile={profile} update={update} onOpenDevice={onOpenDevice} deviceNoise={deviceNoise}/>
        <AccelerationSettings profile={profile} update={update}/>
        <MotionBehavior profile={profile} update={update}/>
      </>}
      <SettingSection key={passthrough?'ds4':'aim'} id={passthrough?'motion-ds4-filters':'motion-filters'}
        title={passthrough?'直通滤波':'采样与扩展滤波'} defaultOpen={passthrough} lazy={!passthrough}
        summary={!passthrough&&<>{gyro.motion.custom_filters?.length??0} 个滤波器</>}>
        {passthrough?<>
          <p className={styles.hint}>发送给虚拟手柄前可处理三轴角速度。加速度计数据沿用设备校准。</p>
          <Range label="直通平滑时间" value={sensorFilter?.smoothing_ms??0} min={0} max={120} step={1} unit=" ms"
            onChange={smoothing_ms=>setSensorFilter({smoothing_ms})}
            help="平滑值越大，角速度越平稳，快速转动的响应也会变慢。0 关闭平滑。"/>
          <Range label="直通噪声抑制倍率" value={sensorFilter?.noise_gate??0} min={0} max={5} step={0.1} unit="×"
            onChange={noise_gate=>setSensorFilter({noise_gate})}
            help="按设备静止校准得到的噪声底抑制小幅波动。0 关闭噪声抑制。"/>
          <Toggle label="直通应用原始角速度脚本" value={sensorFilter?.use_custom_filters??false}
            onChange={use_custom_filters=>setSensorFilter({use_custom_filters})}
            help="在直通时应用下方已启用的 Rhai 脚本。"/>
        </>:<SampleClock value={gyro.timestamp_source} onChange={value=>setGyro('timestamp_source',value)}/>}
        {jsm&&<Toggle label="自动校准陀螺仪偏移" value={gyro.jsm_response?.auto_calibrate_gyro??false}
          onChange={v=>update(p=>{p.gyro.jsm_response={...p.gyro.jsm_response,auto_calibrate_gyro:v};})}
          help="手柄静止时自动估计陀螺仪的残余偏移。默认关闭。"/>}
        <SettingField label="自定义滤波" path="gyro.motion.custom_filters"><SettingLabel label="自定义滤波"/>
          <CustomFilters key={profileId} profile={profile} update={update} embedded rawGyroOnly={passthrough}
            running={(!passthrough||!!sensorFilter?.use_custom_filters)&&filtersRunning}
            statuses={filterStatuses}
            onReset={()=>onMotionAction('reset_filters')}/>
        </SettingField>
      </SettingSection>
    </>}
  </div>;
});

function SampleClock({value,onChange}:{value:Gyro['timestamp_source'];onChange:(value:Gyro['timestamp_source'])=>void}) {
  return <MotionChoice label="采样时钟" value={value}
    options={[["host","SDL 事件时间戳（推荐）"],["sensor","传感器时间戳"]]}
    onChange={next=>onChange(next as Gyro['timestamp_source'])}/>;
}

