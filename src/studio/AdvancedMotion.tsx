import type { MotionConfig, Profile, Status } from '../types';
import { Range, SettingsGroup as Group, Toggle } from './Controls';
import {OptionPicker} from './OptionPicker';
import {padChoices} from './binding-choices';
import { CurveEditor } from './CurveEditor';
import { AccelerationCurve, FlickReleaseCurve, ResponseCurve, useStabilityPreview } from './StabilityCurves';
import styles from './MotionControls.module.css';

export type MotionUpdate=(change:(profile:Profile)=>void)=>void;
type Props={profile:Profile;update:MotionUpdate};
type NumberSpec=[string,string,number,number,number,string?];
function Numbers({object,specs,onChange}:{object:object;specs:NumberSpec[];onChange:(key:string,value:number)=>void}) {
  return <>{specs.map(([key,label,min,max,step,unit])=><Range key={key} label={label} value={Number((object as Record<string,unknown>)[key])} min={min} max={max} step={step} unit={unit} onChange={v=>onChange(key,v)} />)}</>;
}
function Choice({label,value,options,onChange}:{label:string;value:string;options:[string,string][];onChange:(v:string)=>void}) {
  return <label className="studio-field">{label}<select aria-label={label} value={value} onChange={e=>onChange(e.target.value)}>{options.map(([v,l])=><option key={v} value={v}>{l}</option>)}</select></label>;
}
function ControlInput({label,value,onChange}:{label:string;value:string|null;onChange:(v:string|null)=>void}) {
  return <div className="studio-field">{label}<OptionPicker label={label} value={value??''}
    groups={[{options:[{value:'',label:'不设置'}]}, ...padChoices()]} onChange={v=>onChange(v||null)}/></div>;
}
const sticks:[string,string][]=[['right','右摇杆'],['left','左摇杆']];
const mutate=(update:MotionUpdate,key:string,value:unknown)=>update(p=>{(p.gyro.motion as unknown as Record<string,unknown>)[key]=value;});
const nested=(update:MotionUpdate,group:'edge'|'rotation'|'flick'|'trackball',key:string,value:unknown)=>update(p=>{(p.gyro.motion[group] as unknown as Record<string,unknown>)[key]=value;});

export function MotionBehavior({profile,update}:Props) {
  const m=profile.gyro.motion;const set=(k:string,v:unknown)=>mutate(update,k,v);
  return <>
    <Group title="坐标与握持方式">
      <Choice label="体感坐标" value={m.space} options={[["pitch_yaw","本地：俯仰与偏航"],["pitch_roll","本地：俯仰与横滚"],["player","玩家坐标"],["world","世界坐标"]]} onChange={v=>set('space',v)}/>
      <p className={styles.hint}>玩家和世界坐标依靠加速度计估计重力方向。世界坐标的重力参考变化更缓慢。</p>
      <Choice label="控制方式" value={m.mode} options={[["pointer","角速度瞄准"],["steering","姿态控制摇杆"],["stick_rotation","体感旋转摇杆方向"]]} onChange={v=>update(p=>{
        p.gyro.motion.mode=v as MotionConfig['mode'];
        if(v!=='pointer') {p.gyro.output='right_stick';p.gyro.motion.edge.mode='off';}
      })}/>
      <Toggle label="反转水平方向" value={m.invert_x} onChange={v=>set('invert_x',v)}/>
      {m.space==='pitch_yaw'&&<Range label="横滚参与水平转向" value={m.roll_contribution} min={-100} max={100} step={1} unit="%" onChange={v=>set('roll_contribution',v)}/>}
      <details><summary>习惯握持角度</summary>
        {['俯仰','偏航','横滚'].map((label,i)=><Range key={label} label={label+'补偿'} value={m.hold_angles[i]} min={-180} max={180} step={1} unit="°" onChange={v=>update(p=>{p.gyro.motion.hold_angles[i]=v;})}/>)}
      </details>
    </Group>
    <Group title="姿态、中立位与惯性">
      <ControlInput label="姿态归零按键" value={m.reset_input} onChange={v=>set('reset_input',v)}/>
      <ControlInput label="设置正前方按键" value={m.neutral_input} onChange={v=>set('neutral_input',v)}/>
      <p className={styles.hint}>姿态归零重新记录当前朝向。设置正前方只重置水平朝向，保留重力方向上的俯仰与横滚。</p>
      <details><summary>Trackball 惯性体感</summary>
        <ControlInput label="保持水平惯性" value={m.trackball.horizontal_input} onChange={v=>nested(update,'trackball','horizontal_input',v)}/>
        <ControlInput label="保持垂直惯性" value={m.trackball.vertical_input} onChange={v=>nested(update,'trackball','vertical_input',v)}/>
        <Range label="惯性衰减" value={m.trackball.decay} min={0} max={20} step={0.1} onChange={v=>nested(update,'trackball','decay',v)} help="按住指定按键后沿之前的方向继续转动；0 表示保持速度。松开后恢复手腕控制。"/>
      </details>
    </Group>
  </>;
}
export function AccelerationSettings({profile,update}:Props) {
  const a=profile.gyro.acceleration;
  const set=(key:string,value:unknown)=>update(p=>{(p.gyro.acceleration as unknown as Record<string,unknown>)[key]=value;});
  return <Group title="加速曲线形状">
    <Choice label="响应曲线" value={a.curve} options={[["linear","线性"],["natural","自然"],["power","幂函数"],["quadratic","二次"],["sigmoid","S 曲线"],["jump","跳变"],["custom","自定义控制点"]]} onChange={v=>set('curve',v)}/>
    {a.curve==='natural'&&<Numbers object={a} specs={[["natural_half","达到半程倍率的速度",0.01,4000,1," °/s"]]} onChange={set}/>}
    {a.curve==='power'&&<Numbers object={a} specs={[["power_reference","参考转速",0.01,4000,1," °/s"],["power_exponent","幂指数",0.01,10,0.1]]} onChange={set}/>}
    {a.curve==='sigmoid'&&<Numbers object={a} specs={[["sigmoid_mid","过渡中点",0,4000,1," °/s"],["sigmoid_width","过渡宽度",0.01,2000,1," °/s"]]} onChange={set}/>}
    {a.curve==='jump'&&<Numbers object={a} specs={[["jump_width","跳变过渡宽度",0,2000,1," °/s"]]} onChange={set}/>}
    {a.curve==='custom'
      ? <CurveEditor points={a.points} onChange={v=>set('points',v)} maxX={4000} maxY={10} xLabel="转速 °/s" yLabel="倍率"/>
      : <AccelerationCurve acceleration={a}/>}
  </Group>;
}
export function StabilitySettings({profile,update,onOpenDevice,deviceNoise}:Props & {onOpenDevice:()=>void;deviceNoise?:number}) {
  const gyro = profile.gyro;
  const m = gyro.motion;
  const set = (key: string, value: unknown) => mutate(update, key, value);
  const {curves, failed} = useStabilityPreview(gyro, deviceNoise);
  return <div className={styles.stabilityGrid}>
    <div className={styles.curveIntro}>
      <p className="studio-muted">用示例输入预览当前编辑值。调节参数时曲线随即更新。</p>
      <div className={styles.curveLegend}>
        <span>参考线</span><span>当前设置</span>
      </div>
    </div>
    {failed ? <p role="status" className="studio-muted">曲线暂未更新，请检查当前参数。</p>
      : !curves && <p role="status" className="studio-muted">正在计算响应曲线…</p>}
    {m.mode==='steering' && <p className="studio-muted">姿态控制摇杆模式下，角速度曲线用于对照，倾斜误差曲线展示姿态估计。</p>}

    <Group title="平滑与采样">
      <div className={styles.stabilityPair + ' ' + styles.smoothingPair}>
        <ResponseCurve title="平滑响应曲线" points={curves?.smoothing} xLabel="时间（ms）" yLabel="角速度（°/s）"/>
        <div>
          <div className={styles.samplingFields}>
            <Choice label="采样时钟" value={gyro.timestamp_source}
              options={[["host","SDL 事件时间戳（推荐）"],["sensor","传感器时间戳"]]}
              onChange={value=>update(draft=>{draft.gyro.timestamp_source=value as Profile['gyro']['timestamp_source'];})}/>
            <Choice label="平滑方式" value={m.smoothing} options={[["ema","EMA 时间平滑"],["decay","随速度衰减的平滑"],["one_euro","One Euro 自适应滤波"]]} onChange={v=>set('smoothing',v)}/>
          </div>
          {m.smoothing!=='one_euro'&&<Range label="平滑时间" value={gyro.smoothing_ms} min={0} max={50} step={1} unit=" ms"
            onChange={value=>update(draft=>{draft.gyro.smoothing_ms=value;})} help="按实际采样间隔平滑角速度，0 为关闭。"/>}
          {m.smoothing==='decay'&&<Numbers object={m} specs={[["decay_threshold","退出平滑的转速",0.1,2000,1," °/s"]]} onChange={set}/>}
          {m.smoothing==='one_euro'&&<Numbers object={m} specs={[["one_euro_cutoff","最低截止频率",0.01,100,0.1," Hz"],["one_euro_beta","速度响应系数",0,10,0.01]]} onChange={set}/>}
          <p className={styles.hint}>采样时钟用于实际设备。示例每 2 ms 取样，展示慢转和快转时的平滑效果。</p>
        </div>
      </div>
    </Group>
    <Group title="静止抑制与恢复">
      <div className={styles.stabilityPair}>
        <div>
          <ResponseCurve title="低速抑制曲线" points={curves?.idle} xLabel="输入转速（°/s）" yLabel="稳态输出转速（°/s）"/>
          <Range label="静止抑制" value={gyro.noise_threshold} min={0} max={5} step={0.05} unit=" °/s"
            onChange={value=>update(draft=>{draft.gyro.noise_threshold=value;})} help="减去微小的角速度，抑制手柄静止时的微动。"/>
          <Numbers object={m} specs={[["noise_gate","实测噪声抑制宽度",0,10,0.1,"×"]]} onChange={set}/>
          {curves && <p className={styles.hint}>{curves.measured_noise
            ? `预览使用设备实测噪声底 ${Number(curves.noise_floor.toFixed(4))} °/s。`
            : `暂无实测噪声底，曲线使用示例值 ${curves.noise_floor} °/s。完成静止校准后使用实测值。`}</p>}
          <button type="button" className="secondary-btn" onClick={onOpenDevice}>打开设备校准</button>
        </div>
        <div>
          <ResponseCurve title="输出恢复曲线" points={curves?.recovery} xLabel="时间（ms）" yLabel="输出比例（%）"/>
          <Numbers object={m} specs={[["cutoff_dps","低速截止",0,100,0.1," °/s"],["recovery_ms","截止后的恢复时间",0,1000,1," ms"]]} onChange={set}/>
          <p className={styles.hint}>超过低速截止后，输出逐渐恢复到 100%。低速截止为 0 时，仅在处理状态重置后恢复。</p>
        </div>
      </div>
    </Group>
    <Group title="停准与方向稳定">
      <div className={styles.stabilityTriplet}>
        <div>
          <ResponseCurve title="角度吸附曲线" points={curves?.snap} xLabel="输入方向角（°）" yLabel="输出方向角（°）"/>
          <Numbers object={m} specs={[["angle_snap","水平／垂直吸附角",0,45,0.5,"°"]]} onChange={set}/>
          <Toggle label="柔和进入角度吸附" value={m.angle_snap_ease} onChange={v=>set('angle_snap_ease',v)}/>
        </div>
        <div>
          <ResponseCurve title="减速刹车曲线" points={curves?.brake} xLabel="10 ms 内转速减少量（°/s）" yLabel="速度保留（%）"/>
          <Numbers object={m} specs={[["brake_strength","减速刹车强度",0,1,0.05],["brake_threshold","刹车触发减速量",0.01,1000,1," °/s / 10 ms"]]} onChange={set}/>
          <p className={styles.hint}>以减速到 30 °/s 为例，观察急停时保留多少速度。</p>
        </div>
        <div>
          <ResponseCurve title="倾斜误差曲线" points={curves?.drift} xLabel="时间（ms）" yLabel="倾斜误差（°）"/>
          <div>
            <Toggle label="使用重力修正姿态漂移" value={m.drift_correction} onChange={v=>set('drift_correction',v)} help="可修正倾斜漂移；加速度计无法观测绕重力方向的水平漂移。"/>
            <p className={styles.hint}>示例展示手柄放稳后，约 6° 的倾斜误差如何随时间变化。</p>
          </div>
        </div>
      </div>
    </Group>
  </div>;
}
export function AdvancedTurning({profile,update}:Props) {
  const m=profile.gyro.motion;
  const set=(k:string,v:unknown)=>mutate(update,k,v);
  return <div className={styles.grid}>
    {m.mode==='steering'&&<Group title="姿态控制">
      <Numbers object={m} specs={[["steering_range","达到满杆的转角",1,180,1,"°"],["recenter_strength","自动回中强度",0,4,0.1],["reset_ease_ms","手动归零过渡时间",0,2000,10," ms"]]} onChange={set}/>
      <Toggle label="只控制水平轴" value={m.steering_exclude_y} onChange={v=>set('steering_exclude_y',v)}/>
      <p className={styles.hint}>自动回中适用于本地坐标的水平轴；垂直轴使用姿态归零按键回中。玩家／世界坐标无法通过重力自动恢复水平朝向。</p>
    </Group>}
    {m.mode==='stick_rotation'&&<Group title="体感旋转摇杆方向">
      <Choice label="旋转哪个摇杆" value={m.rotation.stick} options={sticks} onChange={v=>nested(update,'rotation','stick',v)}/>
      <Numbers object={m.rotation} specs={[["sensitivity","旋转倍率",0.01,100,0.1,"×"],["deadzone","旋转死区",0,0.95,0.01],["smooth_ms","拇指方向稳定时间",0,1000,1," ms"]]} onChange={(k,v)=>nested(update,'rotation',k,v)}/>
      <Toggle label="相对模式：逐渐返回拇指方向" value={m.rotation.relative} onChange={v=>nested(update,'rotation','relative',v)}/>
      {m.rotation.relative&&<Range label="回归时间" value={m.rotation.return_ms} min={0} max={5000} step={10} unit=" ms" onChange={v=>nested(update,'rotation','return_ms',v)}/>}
      <Toggle label="手腕转动时接管瞄准方向" value={m.rotation.takeover} onChange={v=>nested(update,'rotation','takeover',v)}/>
      {m.rotation.takeover&&<Numbers object={m.rotation} specs={[["takeover_dps","体感接管速度",0,2000,1," °/s"],["outer","外圈交还拇指阈值",0.05,1,0.01]]} onChange={(k,v)=>nested(update,'rotation',k,v)}/>}
      <p className={styles.hint}>摇杆决定粗略方向，手腕细调角度。推力保持不变，摇杆回到中心会清除角度偏移。</p>
    </Group>}
    <Group title="边缘持续转向">
      {m.mode==='pointer'?<>
        <Choice label="边缘转向方式" value={m.edge.mode} options={[["off","关闭"],["ramp","随转角提速"],["momentum","保持出界速度"],["both","提速与惯性"]]} onChange={v=>nested(update,'edge','mode',v)}/>
        {m.edge.mode!=='off'&&<>
          <Numbers object={m.edge} specs={[["zone","开始持续转向的角度",0,170,1,"°"],["blend","接管过渡区间",0,170,1,"°"],["ramp","达到最大速度的区间",0,170,1,"°"],["max_dps","最大持续转速",0,4000,1," °/s"],["momentum","出界速度保留倍率",0,10,0.1,"×"],["decay_s","惯性衰减时间",0,30,0.1," s"],["gyro_keep","边缘处保留手腕控制",0,1,0.05]]} onChange={(k,v)=>nested(update,'edge',k,v)}/>
          <Toggle label="只作用于水平方向" value={m.edge.horizontal_only} onChange={v=>nested(update,'edge','horizontal_only',v)}/>
          <p className={styles.hint}>先用姿态归零键确定中立位。手腕超过边界后继续转向，向中心回拉可制动。</p>
        </>}
      </>:<p className={styles.hint}>在「体感行为」选择角速度瞄准后，可配置边缘转向。</p>}
    </Group>
  </div>;
}

export function FlickStickSettings({profile, update}: Props) {
  const m = profile.gyro.motion;
  const flickChange=(key:string,value:unknown)=>update(p=>{
    const f=p.gyro.motion.flick;(f as unknown as Record<string,unknown>)[key]=value;
  });
  return <Group title="Flick Stick 与摇杆瞄准">
    <Toggle label="启用 Flick Stick" value={m.flick.enabled} onChange={v=>flickChange('enabled',v)}/>
    <Toggle label="启用配套摇杆瞄准" value={m.flick.stick_aim} onChange={v=>flickChange('stick_aim',v)}/>
    {m.flick.stick_aim&&<>
      <Numbers object={m.flick} specs={[["aim_sensitivity","摇杆瞄准倍率",0.01,100,0.1,"×"],["max_dps","连续瞄准基础转速",1,4000,1," °/s"],["vertical_ratio","摇杆纵向比例",0.01,10,0.05,"×"]]} onChange={(k,v)=>flickChange(k,v)}/>
      <p className={styles.hint}>这三个参数只影响配套摇杆的连续瞄准，不改变快推 Flick 的目标转角。水平目标转速由基础转速、瞄准倍率和推杆进度共同决定，纵向再乘纵向比例。</p>
      {profile.gyro.output==='right_stick'&&<p className={styles.hint}>「满杆角速度」是游戏实际满杆转速的校准值，用于把目标转速换算成虚拟摇杆幅度。提高连续瞄准基础转速不会提高游戏本身的满杆转速。</p>}
    </>}
    {(m.flick.enabled||m.flick.stick_aim)&&<>
      <Toggle label="关闭体感时也启用" value={m.flick.standalone ?? false} onChange={v=>flickChange('standalone',v)}/>
      {!m.flick.standalone && !profile.gyro.enabled && <p className={styles.hint}>请在「体感」页启用体感，以使用 Flick Stick 和配套摇杆瞄准。</p>}
      <Choice label="Flick 使用的摇杆" value={m.flick.stick} options={sticks} onChange={v=>flickChange('stick',v)}/>
      <p className={styles.hint}>{profile.gyro.output === 'right_stick'
        ? '与体感共用输出 · 虚拟右摇杆'
        : '与体感共用输出 · 鼠标'}</p>
      <ControlInput label="按住启用（不设置则常开）" value={m.flick.enable_input} onChange={v=>flickChange('enable_input',v)}/>
      <Choice label="原始摇杆输出" value={m.flick.suppress} options={[["full","使用时屏蔽"],["deadzone","只屏蔽外圈"],["off","保留原始输出"]]} onChange={v=>flickChange('suppress',v)}/>
      <Numbers object={m.flick} specs={[["deadzone","快推转身阈值",0.05,0.99,0.01],["smooth_ms","转身完成时间",0,2000,1," ms"],["forward_deadzone","正前方死角",0,180,1,"°"],["stabilise_ms","绕圈方向稳定",0,1000,1," ms"],["speed_gate_ms","快速推杆判定时间",0,2000,1," ms"]]} onChange={(k,v)=>flickChange(k,v)}/>
      {m.flick.enabled && <>
        <Range label="转角缓出强度" value={(m.flick.release_ease ?? 0.5) * 100} min={0} max={100} step={1} unit="%"
          onChange={value=>flickChange('release_ease', value / 100)}
          help="0% 匀速释放，50% 使用 JSM 缓出（默认）。高于 50% 起步更快，100% 使用三次缓出。"/>
        <FlickReleaseCurve strength={m.flick.release_ease ?? 0.5}/>
        <p className={styles.hint}>曲线控制初始转身的转角进度。完成时间设为 0 时立即输出全部转角。</p>
      </>}
      <Toggle label="慢推锁定为连续瞄准" value={m.flick.slow_lock} onChange={v=>flickChange('slow_lock',v)}/>

      <p className={styles.hint}>快推选择朝向，沿摇杆外圈转动控制镜头。在「游戏配置 → 体感 → 游戏内转角校准」测量游戏转角，可以让转身角度对应游戏中的朝向。</p>
    </>}
  </Group>;
}

export type GameCalibrationProps = {
  status?: Status;
  canMeasure: boolean;
  onMotionAction: (action: string) => void;
  onOpenCalibration: () => void;
};

export function GameAngleCalibration({profile, canMeasure, onOpenCalibration}: Pick<GameCalibrationProps,'canMeasure'|'onOpenCalibration'> & {profile:Profile}) {
  return <Group title="游戏内转角校准">
    <p className={styles.hint}>保存并应用基础层配置，启用映射后打开悬浮窗。对准参照物试转 360°，直接调整数值后再次试转，满意后保存。原来的手动转动测量保留在「更多校准方式」。</p>
    {profile.gyro.output==='right_stick'&&<p className={styles.hint}>使用摇杆 Flick 前，请将游戏摇杆响应曲线设为线性，并关闭边缘及其他转向加速。校准后保持游戏灵敏度不变。</p>}
    <div className="studio-actions">
      <button type="button" className="primary-btn" disabled={!canMeasure||(!profile.gyro.enabled&&!(profile.gyro.motion.flick.standalone&&(profile.gyro.motion.flick.enabled||profile.gyro.motion.flick.stick_aim)))}
        onClick={onOpenCalibration}>打开校准悬浮窗</button>
    </div>
    </Group>;
}
