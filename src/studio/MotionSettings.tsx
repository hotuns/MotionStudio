import { useEffect, useState } from 'react';
import { Range, SettingsGroup, Toggle } from './Controls';
import { inputLabel, type Device, type Gyro, type Profile } from '../types';
import styles from './MotionControls.module.css';
import { MotionBehavior, AccelerationSettings, StabilitySettings, AdvancedTurning, GameAngleCalibration, type GameCalibrationProps } from './AdvancedMotion';
import {CustomFilters} from './CustomFilters';
import type {GyroBindingField, GyroBindingRecording} from '../pad-capture';
import {GyroBindingInput} from './GyroBindingInput';

const tabs = [
  ['behavior', '体感行为', '输出、开启方式与方向'],
  ['sensitivity', '灵敏度', '基础速度与快速转动倍率'],
  ['noise', '防抖与稳态', '平滑、静止抑制与滤波'],
  ['turning','高级转向','姿态控制与边缘转向'],
  ['calibration','游戏内转角校准','测量当前游戏的转向比例'],
] as const;

type Update = (update: (profile: Profile) => void) => void;
type MotionView = 'simple' | 'advanced';
const viewKey = 'motion-studio-motion-view';
const smoothingPresets = [['跟手', 0], ['均衡', 16], ['更稳', 32]] as const;

function initialView(): MotionView {
  try { return localStorage.getItem(viewKey) === 'advanced' ? 'advanced' : 'simple'; }
  catch { return 'simple'; }
}

export function MotionSettings({ profile, profileId, update, device, recording, canRecord, onRecord,
  onCancelRecording, onOpenDevice, onPreviewOutput, status, canMeasure, onMotionAction, onOpenCalibration, deviceNoise }: {
  profile: Profile;
  profileId:string;
  update: Update;
  device?: Device | null;
  deviceNoise?: number;
  recording?: GyroBindingRecording;
  canRecord: boolean;
  onRecord: (field: GyroBindingField) => void;
  onCancelRecording: () => void;
  onOpenDevice: () => void;
  onPreviewOutput: () => void;
}&GameCalibrationProps) {
  const [tab, setTab] = useState<(typeof tabs)[number][0]>('behavior');
  const [view, setView] = useState<MotionView>(initialView);
  const measuring = !!status?.motion?.measuring;
  useEffect(()=>{if (measuring) setTab('calibration');},[measuring]);
  const gyro = profile.gyro;
  const hasGamepad = profile.virtual_gamepad !== 'none';
  const setGyro = <K extends keyof Gyro>(key: K, value: Gyro[K]) => update(draft => { draft.gyro[key] = value; });
  const activation = gyro.activation === 'always' ? 'always' : gyro.activation_mode;
  const bindingLocked = !!recording || !!status?.binding_capture || measuring;
  const split = (device?.touchpads ?? 0) >= 2;
  const advanced = measuring || view === 'advanced';
  const preset = gyro.motion.smoothing === 'ema' ? smoothingPresets.find(([, ms]) => ms === gyro.smoothing_ms) : undefined;
  function changeView(next: MotionView) {
    setView(next);
    try { localStorage.setItem(viewKey, next); } catch { /* The view still changes for this session. */ }
  }

  const activationSettings = <>
    <div className="studio-field">开启方式
      <div role="group" aria-label="开启方式" className={styles.activationModes}>
        {([['always','始终开启'], ['hold','按住开启'], ['toggle','按一下切换开关']] as const).map(([mode, label]) =>
          <button key={mode} type="button" className="secondary-btn" aria-pressed={mode === activation} disabled={bindingLocked}
            onClick={() => {
              if (mode === activation) return;
              update(draft => {
                draft.gyro.activation_mode = mode === 'always' ? 'hold' : mode;
                if (mode === 'always') draft.gyro.activation = 'always';
                else if (draft.gyro.activation === 'always') {
                  draft.gyro.activation = draft.gyro.pause_input === 'left_trigger' ? 'left_shoulder' : 'left_trigger';
                }
              });
            }}>{label}</button>)}
      </div>
    </div>
    {activation !== 'always' && <GyroBindingInput field="activation" value={gyro.activation} blocked={gyro.pause_input}
      split={split} available={canRecord} locked={bindingLocked} recording={recording?.gyroField === 'activation' ? recording : undefined}
      onChange={value => setGyro('activation', value)} onRecord={() => onRecord('activation')} onCancel={onCancelRecording}/>}
  </>;

  return <div className={styles.page}>
    <div className={styles.modeHeader}>
      <div><h2>体感设置</h2><p className={styles.description}>
        {advanced ? '查看体感的全部调校参数。' : '用手腕转动手柄控制视角。先选择控制方式，再调到习惯的速度。'}
      </p></div>
      <div className={styles.activationModes} role="group" aria-label="体感设置视图">
        <button type="button" className="secondary-btn" aria-pressed={!advanced} disabled={bindingLocked}
          onClick={() => changeView('simple')}>简易模式</button>
        <button type="button" className="secondary-btn" aria-pressed={advanced} disabled={bindingLocked}
          onClick={() => changeView('advanced')}>进阶模式</button>
      </div>
    </div>
    {!advanced ? <section className={styles.panel + ' app-card'}>
      <div className={styles.simpleGrid}>
        <SettingsGroup title="1. 控制什么">
          <Toggle label="启用体感" value={gyro.enabled} onChange={value => setGyro('enabled', value)}/>
          {gyro.enabled && (gyro.motion.mode === 'pointer' ? <>
            <div className={styles.activationModes} role="group" aria-label="体感控制">
              <button type="button" className="secondary-btn" aria-pressed={gyro.output === 'mouse'}
                onClick={() => setGyro('output', 'mouse')}>鼠标瞄准</button>
              <button type="button" className="secondary-btn" aria-pressed={gyro.output === 'right_stick'} disabled={!hasGamepad}
                onClick={() => setGyro('output', 'right_stick')}>右摇杆瞄准</button>
            </div>
            <p className={styles.hint}>游戏用鼠标转视角就选鼠标，只接收手柄时用右摇杆。</p>
            {!hasGamepad && <p className={styles.hint}>在游戏配置页头选择虚拟手柄，即可使用右摇杆输出。</p>}
          </> : <p className={styles.hint}>
            当前使用进阶控制方式 · {gyro.motion.mode === 'steering' ? '姿态控制摇杆' : '体感旋转摇杆方向'}
          </p>)}
          {gyro.enabled && <details>
            <summary>方向不对？</summary>
            <Toggle label="反转垂直方向" value={gyro.invert_y} onChange={value => setGyro('invert_y', value)}/>
            <Toggle label="反转水平方向" value={gyro.motion.invert_x}
              onChange={value => update(draft => { draft.gyro.motion.invert_x = value; })}/>
          </details>}
        </SettingsGroup>
        {gyro.enabled && <>
          <SettingsGroup title="2. 什么时候生效">
            {activationSettings}
            <p className={styles.hint}>只想在开镜时用体感，就选按住开启并绑定瞄准键。</p>
            {gyro.pause_input && <p className={styles.hint}>按住暂停 · {inputLabel(gyro.pause_input)}</p>}
          </SettingsGroup>
          <SettingsGroup title="3. 转多快">
            <Range label="体感灵敏度" value={gyro.sensitivity} min={0.1} max={10} step={0.1} unit="×"
              onChange={value => { if (value >= 0.1 && value <= 10) setGyro('sensitivity', value); }}
              help="准星跟不上时调高，容易转过头时调低。"/>
          </SettingsGroup>
          <SettingsGroup title="4. 要多稳">
            <div className={styles.activationModes} role="group" aria-label="防抖预设">
              {smoothingPresets.map(([label, ms]) => <button key={ms} type="button" className="secondary-btn"
                aria-pressed={preset?.[1] === ms} onClick={() => update(draft => {
                  draft.gyro.motion.smoothing = 'ema';
                  draft.gyro.smoothing_ms = ms;
                })}>{label}</button>)}
            </div>
            <p className={styles.hint}>画面有细小抖动时试试均衡或更稳。平滑越多，转向响应越慢。</p>
            {!preset && <p className={styles.hint}>当前沿用进阶平滑设置。</p>}
          </SettingsGroup>
        </>}
      </div>
      {gyro.enabled && <div className={styles.simpleTest}>
        <p className={styles.description}>应用修改并启动映射后，轻转手柄，在设备页查看输出。</p>
        <button type="button" className="secondary-btn" onClick={onPreviewOutput}>查看输出预览</button>
      </div>}
    </section> : <>
    <nav className={styles.nav} aria-label="体感调校分类">
      {tabs.map(([id, label, hint], index) => <button key={id} type="button" disabled={measuring&&id!=='calibration'} aria-current={tab === id ? 'page' : undefined} onClick={() => { if (id !== tab) onCancelRecording(); setTab(id); }}>
        <span className={styles.number}>{String(index + 1).padStart(2, '0')}</span>
        <span><strong>{label}</strong><small>{hint}</small></span>
      </button>)}
    </nav>
    <section className={styles.panel + ' app-card'}>
      {tab === 'behavior' && <>
        <h2>体感行为</h2>
        <div className={styles.grid}>
          <div className={styles.group}>
            <div className={styles.groupHeading}><h3>输出与方向</h3></div>
            <Toggle label="启用体感" value={gyro.enabled} onChange={value => setGyro('enabled', value)} />
            <label className="studio-field">体感输出
              <select value={gyro.output} disabled={gyro.motion.mode!=='pointer'} onChange={event => setGyro('output', event.target.value as Gyro['output'])}>
                <option value="mouse">鼠标</option>
                <option value="right_stick" disabled={!hasGamepad}>虚拟手柄右摇杆</option>
              </select>
            </label>
            {!hasGamepad && <p className={styles.hint}>在游戏配置页头选择虚拟手柄，即可使用右摇杆输出。</p>}
            <Toggle label="反转垂直方向" value={gyro.invert_y} onChange={value => setGyro('invert_y', value)} />
          </div>
          <div className={styles.group}>
            <div className={styles.groupHeading}><h3>开启和暂停</h3></div>
            {activationSettings}
            <GyroBindingInput field="pause_input" value={gyro.pause_input} blocked={gyro.activation}
              split={split} available={canRecord} locked={bindingLocked} recording={recording?.gyroField === 'pause_input' ? recording : undefined}
              onChange={value => setGyro('pause_input', value || null)} onRecord={() => onRecord('pause_input')} onCancel={onCancelRecording}/>
            {(activation !== 'always' || gyro.pause_input) &&
              <Range label="扳机与摇杆阈值" value={gyro.activation_threshold} min={0} max={0.95} step={0.05}
                onChange={value => setGyro('activation_threshold', value)} />}
            <p className={styles.hint}>按住暂停键可调整握姿，松开后恢复原先的体感状态。</p>
            <p className={styles.hint}>触摸板按下与手指接触可以分别绑定。连接并选择手柄后，也可以直接监听录入。</p>
          </div>
          <MotionBehavior profile={profile} update={update}/>
        </div>
      </>}
      {tab === 'sensitivity' && <>
        <h2>灵敏度与响应</h2>
        <div className={styles.grid}>
          <div className={styles.group}>
            <div className={styles.groupHeading}><h3>基础灵敏度</h3></div>
            <Range label="体感灵敏度" value={gyro.sensitivity} min={0.1} max={10} step={0.1} unit="×" onChange={value => setGyro('sensitivity', value)} />
            <Range label="垂直灵敏度比例" value={gyro.vertical_ratio} min={0.1} max={2} step={0.05} unit="×" onChange={value => setGyro('vertical_ratio', value)} />
            {gyro.output === 'right_stick'
              ? <Range label="满杆角速度" value={gyro.stick_full_scale_dps} min={30} max={1440} step={1} unit=" °/s"
                  onChange={value => setGyro('stick_full_scale_dps', value)} help="灵敏度为 1× 时，达到此转速相当于将右摇杆推到底。" />
              : <Range label="基础鼠标计数" value={gyro.counts_per_degree} min={1} max={100} step={0.5} unit=" /°"
                  onChange={value => setGyro('counts_per_degree', value)} help="每旋转一度产生的基础鼠标计数，再乘以体感灵敏度。" />}
          </div>
          <div className={styles.group}>
            <div className={styles.groupHeading}><h3>转动加速</h3></div>
            <Range label="快速转动倍率" value={gyro.acceleration.fast_multiplier} min={1} max={4} step={0.1} unit="×"
              onChange={value => update(draft => { draft.gyro.acceleration.fast_multiplier = value; })}
              help="慢速微调沿用基础灵敏度。快速转动时逐步提高倍率，1× 关闭加速。" />
            {gyro.acceleration.fast_multiplier > 1 && <>
              <Range label="加速起点" value={gyro.acceleration.start_dps} min={0} max={1000} step={1} unit=" °/s"
                onChange={value => update(draft => { draft.gyro.acceleration.start_dps = value; })} />
              <Range label="达到最高倍率" value={gyro.acceleration.full_dps} min={1} max={2000} step={1} unit=" °/s"
                onChange={value => update(draft => { draft.gyro.acceleration.full_dps = value; })} />
            </>}
          </div>
          <AccelerationSettings profile={profile} update={update}/>
        </div>
      </>}
      {tab === 'noise' && <>
        <h2>防抖与稳态</h2>
        <StabilitySettings profile={profile} update={update} onOpenDevice={onOpenDevice} deviceNoise={deviceNoise}/>
        <CustomFilters key={profileId} profile={profile} update={update}
          running={status?.profile_id===profileId&&['running','observing'].includes(status.state)}
          statuses={status?.profile_id===profileId?status.motion?.filters:undefined}
          onReset={()=>onMotionAction('reset_filters')}/>
      </>}
      {tab==='turning'&&<><h2>高级转向</h2><AdvancedTurning profile={profile} update={update}/></>}
      {tab==='calibration'&&<GameAngleCalibration profile={profile} canMeasure={canMeasure} onOpenCalibration={onOpenCalibration}/>}
    </section>
    </>}
  </div>;
}

