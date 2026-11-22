import {memo} from 'react';
import {isSensorOutput,sensorOutputLabel} from '../sensor-passthrough';
import type { MotionConfig, Profile } from '../types';
import {useLanguage} from '../i18n';
import {nativeSmoothing,usesJsmAim} from '../motion-presets';
import { Range, Toggle } from './Controls';
import {SettingField, SettingLabel, SettingSection, settingStyles} from './SettingLayout';
import {PadInputPicker} from './PadInputPicker';
import {padChoices} from './binding-choices';
import { CurveEditor } from './CurveEditor';
import { AccelerationCurve, FlickReleaseCurve, ResponseCurve, useStabilityPreview } from './StabilityCurves';
import styles from './MotionControls.module.css';

export type MotionUpdate=(change:(profile:Profile)=>void)=>void;
type Props={profile:Profile;update:MotionUpdate};
type NumberSpec=[string,string,number,number,number,string?];
function Numbers({object,specs,onChange}:{object:object;specs:NumberSpec[];onChange:(key:string,value:number)=>void}) {
  return <>{specs.map(([key,label,min,max,step,unit])=><Range key={key} label={label} value={Number((object as Record<string,unknown>)[key])} min={min} max={max} step={step} unit={unit} slider={!unit?.includes('ms')&&!unit?.includes('°/s')} onChange={v=>onChange(key,v)} />)}</>;
}
export function MotionChoice({label,value,options,onChange,disabled,settingPath}:{label:string;value:string;options:readonly (readonly [string,string])[];onChange:(v:string)=>void;disabled?:boolean;settingPath?:string|string[]}) {
  return <SettingField label={label} path={settingPath} disabled={disabled}><div className={settingStyles.row}><SettingLabel label={label}/><select aria-label={label} value={value} disabled={disabled} onChange={e=>onChange(e.target.value)}>{options.map(([v,l])=><option key={v} value={v}>{l}</option>)}</select></div></SettingField>;
}
const Choice=MotionChoice;
function ControlInput({label,value,onChange}:{label:string;value:string|null;onChange:(v:string|null)=>void}) {
  return <SettingField label={label}><div className="studio-field"><SettingLabel label={label}/><PadInputPicker label={label} value={value??''}
    groups={[{options:[{value:'',label:'不设置'}]}, ...padChoices()]} onChange={v=>onChange(v||null)}/></div></SettingField>;
}
const sticks:[string,string][]=[['right','右摇杆'],['left','左摇杆']];
const localSpaces = [['pitch_yaw','本地：俯仰与偏航'],['pitch_roll','本地：俯仰与横滚']] as const;
const aimSpaces = [['pitch_yaw','本地坐标'], ['yaw_plus_roll','偏航与横滚合成'], ['player','玩家空间（转动）'], ['player_lean','玩家空间（侧倾）'],
  ['world','世界空间（转动）'], ['world_lean','世界空间（侧倾）']] as const;
const postureSpaces = [...localSpaces, ['player','玩家坐标'], ['world','世界坐标']] as const;
const axisOptions = [['0','关闭'], ['1','X'], ['2','Y'], ['4','Z'], ['3','X + Y'], ['5','X + Z'], ['6','Y + Z'], ['7','X + Y + Z']] as const;
const spaceHelp = {
  player: '左右转动手柄控制水平瞄准，上下瞄准跟随手柄自身的俯仰。',
  player_lean: '向两侧倾斜手柄控制水平瞄准，上下瞄准跟随手柄自身的俯仰。',
  world: '绕重力竖轴转动控制水平瞄准，上下瞄准也以地面为参考。',
  world_lean: '向两侧倾斜手柄控制水平瞄准，水平和垂直两个方向都以地面为参考。',
};
const mutate=(update:MotionUpdate,key:string,value:unknown)=>update(p=>{(p.gyro.motion as unknown as Record<string,unknown>)[key]=value;});
const nested=(update:MotionUpdate,group:'rotation'|'flick'|'trackball',key:string,value:unknown)=>update(p=>{(p.gyro.motion[group] as unknown as Record<string,unknown>)[key]=value;});

export function MotionBehavior({profile,update}:Props) {
  const m=profile.gyro.motion;const set=(k:string,v:unknown)=>mutate(update,k,v);
  const jsm=usesJsmAim(profile.gyro);
  const spaces=jsm?aimSpaces:postureSpaces;
  const space=jsm?(m.space==='pitch_roll'?'pitch_yaw':m.space):({player_lean:'player',world_lean:'world',yaw_plus_roll:'pitch_yaw'} as Record<string,string>)[m.space]??m.space;
  const local=space.startsWith('pitch_');
  const response=profile.gyro.jsm_response;
  const setAxis=(key:'mouse_x_axes'|'mouse_y_axes',value:string)=>update(p=>{
    p.gyro.jsm_response={...p.gyro.jsm_response,[key]:Number(value)};
  });
  return <>
    <SettingSection id="motion-direction" title="方向与握持方式" summary={spaces.find(([value])=>value===space)?.[1]}>
      <Choice label="体感坐标" value={space} options={spaces} onChange={v=>update(p=>{
        if(jsm&&p.gyro.motion.space==='pitch_roll'&&p.gyro.jsm_response?.mouse_x_axes==null) {
          p.gyro.jsm_response={...p.gyro.jsm_response,mouse_x_axes:4};
        }
        p.gyro.motion.space=v as MotionConfig['space'];
      })}/>
      {jsm&&space in spaceHelp&&<p className={styles.hint}>{spaceHelp[space as keyof typeof spaceHelp]}</p>}
      {jsm&&local&&<div className={settingStyles.columns}>
        <Choice label="水平瞄准输入轴" settingPath="gyro.jsm_response.mouse_x_axes"
          value={String(response?.mouse_x_axes??(m.space==='pitch_roll'?4:2))} options={axisOptions} onChange={v=>setAxis('mouse_x_axes',v)}/>
        <Choice label="垂直瞄准输入轴" settingPath="gyro.jsm_response.mouse_y_axes"
          value={String(response?.mouse_y_axes??1)} options={axisOptions} onChange={v=>setAxis('mouse_y_axes',v)}/>
      </div>}
      <Toggle label="反转垂直方向" value={profile.gyro.invert_y} onChange={v=>update(p=>{p.gyro.invert_y=v;})}/>
      <Toggle label="反转水平方向" value={m.invert_x} onChange={v=>set('invert_x',v)}/>
      {(jsm?space==='yaw_plus_roll':space==='pitch_yaw')&&<Range label="横滚参与水平转向" value={m.roll_contribution} min={-100} max={100} step={1} unit="%" onChange={v=>set('roll_contribution',v)}/>}
      {(!jsm||local)&&<SettingField label="习惯握持角度" path="gyro.motion.hold_angles">
        {(jsm?['俯仰']:['俯仰','偏航','横滚']).map((label,i)=><Range key={label} label={label+'补偿'} settingPath={[]} slider={false} value={m.hold_angles[i]} min={-180} max={180} step={1} unit="°" onChange={v=>update(p=>{p.gyro.motion.hold_angles[i]=v;})}/>)}
      </SettingField>}
    </SettingSection>
    <SettingSection id="motion-posture" title="控制方式与姿态" summary={m.mode==='pointer'?'角速度瞄准':m.mode==='steering'?'姿态控制摇杆':'体感旋转摇杆方向'}>
      <Choice label="控制方式" value={m.mode} settingPath={['gyro.motion.mode','gyro.output','gyro.motion.edge.mode']}
        options={[["pointer","角速度瞄准"],["steering","姿态控制摇杆"],["stick_rotation","体感旋转摇杆方向"]]} onChange={v=>update(p=>{
          p.gyro.motion.mode=v as MotionConfig['mode'];
          if(v!=='pointer') {p.gyro.output='right_stick';p.gyro.motion.edge.mode='off';}
        })}/>
      {!jsm&&<><ControlInput label="姿态归零按键" value={m.reset_input} onChange={v=>set('reset_input',v)}/>
      <ControlInput label="设置正前方按键" value={m.neutral_input} onChange={v=>set('neutral_input',v)}/>
      <p className={styles.hint}>姿态归零重新记录当前朝向。设置正前方只重置水平朝向，保留重力方向上的俯仰与横滚。</p>
      <AdvancedTurning profile={profile} update={update}/></>}
    </SettingSection>
    <SettingSection id="motion-inertia" title="Trackball 惯性体感" summary={m.trackball.horizontal_input||m.trackball.vertical_input?'已设置惯性按键':'未设置'}>
        <ControlInput label="保持水平惯性" value={m.trackball.horizontal_input} onChange={v=>nested(update,'trackball','horizontal_input',v)}/>
        <ControlInput label="保持垂直惯性" value={m.trackball.vertical_input} onChange={v=>nested(update,'trackball','vertical_input',v)}/>
        <Range label="惯性衰减" value={m.trackball.decay} min={0} max={20} step={0.1} onChange={v=>nested(update,'trackball','decay',v)} help="按住指定按键后沿之前的方向继续转动；0 表示保持速度。松开后恢复手腕控制。"/>
    </SettingSection>
  </>;
}
export function AccelerationSettings({profile,update}:Props) {
  const a=profile.gyro.acceleration;
  const jsm=usesJsmAim(profile.gyro);
  const curves:[string,string][]=[["linear","线性"],["natural","自然"],["power","幂函数"],["quadratic","二次"],["sigmoid","S 曲线"],["jump","跳变"]];
  if(!jsm)curves.push(["custom","自定义控制点"]);
  const set=(key:string,value:unknown)=>update(p=>{(p.gyro.acceleration as unknown as Record<string,unknown>)[key]=value;});
  return <SettingSection id="motion-acceleration" title="转动加速" summary={a.fast_multiplier>1?`${a.fast_multiplier}×`:'关闭'} lazy>
    <div className={settingStyles.columns}><div>
    <Range label="快速转动倍率" value={a.fast_multiplier} min={1} max={4} step={0.1} unit="×" onChange={v=>set('fast_multiplier',v)}
      help="慢速微调沿用基础灵敏度。快速转动时逐步提高倍率，1× 关闭加速。"/>
    {a.fast_multiplier>1&&<>
      <Range label="加速起点" value={a.start_dps} min={0} max={1000} step={1} unit=" °/s" slider={false} onChange={v=>set('start_dps',v)}/>
      <Range label="达到最高倍率" value={a.full_dps} min={1} max={2000} step={1} unit=" °/s" slider={false} onChange={v=>set('full_dps',v)}/>
    </>}
    <Choice label="加速度曲线" value={jsm&&a.curve==='custom'?'linear':a.curve} options={curves} onChange={v=>set('curve',v)}/>
    {jsm&&a.curve==='custom'&&<p className={styles.hint}>这份配置保存了自定义控制点，角速度瞄准当前使用线性曲线。</p>}
    {a.curve==='natural'&&<Numbers object={a} specs={[["natural_half","达到半程倍率的速度",0.01,4000,1," °/s"]]} onChange={set}/>}
    {a.curve==='power'&&<Numbers object={a} specs={[["power_reference","参考转速",0.01,4000,1," °/s"],["power_exponent","幂指数",0.01,10,0.1]]} onChange={set}/>}
    {a.curve==='sigmoid'&&<Numbers object={a} specs={[["sigmoid_mid","过渡中点",0,4000,1," °/s"],["sigmoid_width","过渡宽度",0.01,2000,1," °/s"]]} onChange={set}/>}
    {a.curve==='jump'&&<Numbers object={a} specs={[["jump_width","跳变过渡宽度",0,2000,1," °/s"]]} onChange={set}/>}
    {!jsm&&a.curve==='custom'&&<SettingField label="自定义控制点"><SettingLabel label="自定义控制点"/><CurveEditor points={a.points} onChange={v=>set('points',v)} maxX={4000} maxY={10} xLabel="转速 °/s" yLabel="倍率" showPreview={false}/></SettingField>}
    </div><div><AccelerationCurve acceleration={a} jsm={jsm}/></div></div>
  </SettingSection>;
}
export function StabilitySettings({profile,update,onOpenDevice,deviceNoise}:Props & {onOpenDevice:()=>void;deviceNoise?:number}) {
  const gyro = profile.gyro;
  const m = gyro.motion;
  const jsm=usesJsmAim(gyro);
  const response=gyro.jsm_response;
  const {window:nativeWindow,threshold:nativeThreshold,decay:nativeDecay}=nativeSmoothing(gyro);
  const oneEuro=jsm?(response?.one_euro??(m.smoothing==='one_euro')):m.smoothing==='one_euro';
  const setResponse=(change:NonNullable<typeof response>)=>update(p=>{
    p.gyro.jsm_response={...p.gyro.jsm_response,...change};
  });
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

    <SettingSection id="motion-smoothing" title="防抖与平滑" defaultOpen
      summary={jsm?<>{nativeDecay?'Decay':'阈值平滑'} · {nativeWindow} ms{oneEuro?' + One Euro':''}</>
        :oneEuro?<>One Euro · {m.one_euro_cutoff} Hz</>:<>{m.smoothing==='ema'?'EMA':'Decay'} · {gyro.smoothing_ms} ms</>}>
      <div className={settingStyles.columns + ' ' + styles.smoothingPair}>
        <div>
          {jsm?<>
            <Choice label="前置平滑方式" value={nativeDecay?'decay':'rolling'}
              options={[['rolling','阈值平滑'],['decay','随速度衰减的平滑']]} onChange={v=>setResponse({decay_smoothing:v==='decay'})}/>
            <Range label="平滑窗口" value={nativeWindow} min={0} max={2000} step={1} unit=" ms" slider={false}
              onChange={v=>setResponse({smooth_time_ms:v})}/>
            <Range label="平滑阈值" value={nativeThreshold} min={0} max={2000} step={0.1} unit=" °/s" slider={false}
              onChange={v=>setResponse({smooth_threshold_dps:v})} help="转速达到阈值后直接响应。窗口或阈值为 0 时关闭前置平滑。"/>
            <Toggle label="叠加 One Euro 滤波" value={oneEuro} onChange={v=>setResponse({one_euro:v})}/>
          </>:<>
          <div className={styles.samplingFields}>
            <Choice label="平滑方式" value={m.smoothing} options={[["ema","EMA 时间平滑"],["decay","随速度衰减的平滑"],["one_euro","One Euro 自适应滤波"]]} onChange={v=>set('smoothing',v)}/>
          </div>
          {!oneEuro&&<Range label="平滑时间" value={gyro.smoothing_ms} min={0} max={50} step={1} unit=" ms"
            onChange={value=>update(draft=>{draft.gyro.smoothing_ms=value;})} help="按实际采样间隔平滑角速度，0 为关闭。"/>}
          {m.smoothing==='decay'&&<Numbers object={m} specs={[["decay_threshold","退出平滑的转速",0.1,2000,1," °/s"]]} onChange={set}/>}
          </>}
          {oneEuro&&<Numbers object={m} specs={[["one_euro_cutoff","最低截止频率",0.01,100,0.1," Hz"],["one_euro_beta","速度响应系数",0,10,0.01]]} onChange={set}/>}
          <p className={styles.hint}>采样时钟用于实际设备。示例每 2 ms 取样，展示慢转和快转时的平滑效果。</p>
        </div>
        <ResponseCurve title="平滑响应曲线" points={curves?.smoothing} xLabel="时间（ms）" yLabel="角速度（°/s）"/>
      </div>
    </SettingSection>
    <SettingSection id="motion-idle" title="静止抑制与恢复" summary={jsm?<>{m.cutoff_dps} °/s</>:<>{gyro.noise_threshold} °/s · {m.recovery_ms} ms</>}>
      {jsm?<div className={settingStyles.columns}><div>
        <Numbers object={m} specs={[["cutoff_dps","低速截止",0,100,0.1," °/s"]]} onChange={set}/>
        <Range label="完全恢复转速" value={gyro.jsm_response?.cutoff_recovery_dps??0} min={0} max={2000} step={0.1} unit=" °/s" slider={false}
          onChange={v=>update(p=>{p.gyro.jsm_response={...p.gyro.jsm_response,cutoff_recovery_dps:v};})}
          help="高于低速截止时，两者之间的角速度会逐步恢复到完整输出。"/>
      </div><ResponseCurve title="低速抑制曲线" points={curves?.idle} xLabel="输入转速（°/s）" yLabel="稳态输出转速（°/s）"/></div>:<div className={styles.stabilityGrid}>
      <div className={settingStyles.columns}>
        <div>
          <Range label="静止抑制" value={gyro.noise_threshold} min={0} max={5} step={0.05} unit=" °/s"
            onChange={value=>update(draft=>{draft.gyro.noise_threshold=value;})} help="减去微小的角速度，抑制手柄静止时的微动。"/>
          <Numbers object={m} specs={[["noise_gate","实测噪声抑制宽度",0,10,0.1,"×"]]} onChange={set}/>
          {curves && <p className={styles.hint}>{curves.measured_noise
            ? `预览使用设备实测噪声底 ${Number(curves.noise_floor.toFixed(4))} °/s。`
            : `暂无实测噪声底，曲线使用示例值 ${curves.noise_floor} °/s。完成静止校准后使用实测值。`}</p>}
          <button type="button" className="secondary-btn" onClick={onOpenDevice}>打开设备校准</button>
        </div>
          <ResponseCurve title="低速抑制曲线" points={curves?.idle} xLabel="输入转速（°/s）" yLabel="稳态输出转速（°/s）"/>
      </div>
      <div className={settingStyles.columns}>
        <div>
          <Numbers object={m} specs={[["cutoff_dps","低速截止",0,100,0.1," °/s"],["recovery_ms","截止后的恢复时间",0,1000,1," ms"]]} onChange={set}/>
          <p className={styles.hint}>超过低速截止后，输出逐渐恢复到 100%。低速截止为 0 时，仅在处理状态重置后恢复。</p>
        </div>
          <ResponseCurve title="输出恢复曲线" points={curves?.recovery} xLabel="时间（ms）" yLabel="输出比例（%）"/>
      </div>
      </div>}
    </SettingSection>
    <SettingSection id="motion-direction-stability" title={jsm?'方向稳定':'方向稳定与姿态漂移'} summary={<>{m.angle_snap}°</>}>
      <div className={styles.stabilityGrid}>
      <div className={settingStyles.columns}>
        <div>
          <Numbers object={m} specs={[["angle_snap","水平／垂直吸附角",0,45,0.5,"°"]]} onChange={set}/>
          <Toggle label="柔和进入角度吸附" value={m.angle_snap_ease} onChange={v=>set('angle_snap_ease',v)}/>
        </div>
          <ResponseCurve title="角度吸附曲线" points={curves?.snap} xLabel="输入方向角（°）" yLabel="输出方向角（°）"/>
      </div>
      {!jsm&&<div className={settingStyles.columns}>
        <div>
          <div>
            <Toggle label="使用重力修正姿态漂移" value={m.drift_correction} onChange={v=>set('drift_correction',v)} help="可修正倾斜漂移；加速度计无法观测绕重力方向的水平漂移。"/>
            <p className={styles.hint}>示例展示手柄放稳后，约 6° 的倾斜误差如何随时间变化。</p>
          </div>
        </div>
          <ResponseCurve title="倾斜误差曲线" points={curves?.drift} xLabel="时间（ms）" yLabel="倾斜误差（°）"/>
      </div>}
      </div>
    </SettingSection>
    <SettingSection id="motion-brake" title="减速刹车" summary={m.brake_strength>0?<>{Math.round(m.brake_strength*100)}%</>:'关闭'}>
      <div className={settingStyles.columns}>
        <div><Numbers object={m} specs={[["brake_strength","减速刹车强度",0,1,0.05],["brake_threshold","刹车触发减速量",0.01,1000,1," °/s / 10 ms"]]} onChange={set}/></div>
        <ResponseCurve title="减速刹车曲线" points={curves?.brake} xLabel="10 ms 内转速减少量（°/s）" yLabel="速度保留（%）"/>
      </div>
      <p className={styles.hint}>以减速到 30 °/s 为例，观察急停时保留多少速度。</p>
    </SettingSection>
  </div>;
}
export function AdvancedTurning({profile,update}:Props) {
  const m=profile.gyro.motion;
  const set=(k:string,v:unknown)=>mutate(update,k,v);
  return <>
    {m.mode==='steering'&&<>
      <Numbers object={m} specs={[["steering_range","达到满杆的转角",1,180,1,"°"],["recenter_strength","自动回中强度",0,4,0.1],["reset_ease_ms","手动归零过渡时间",0,2000,10," ms"]]} onChange={set}/>
      <Toggle label="只控制水平轴" value={m.steering_exclude_y} onChange={v=>set('steering_exclude_y',v)}/>
      <p className={styles.hint}>自动回中适用于本地坐标的水平轴；垂直轴使用姿态归零按键回中。玩家／世界坐标无法通过重力自动恢复水平朝向。</p>
    </>}
    {m.mode==='stick_rotation'&&<>
      <Choice label="旋转哪个摇杆" value={m.rotation.stick} options={sticks} onChange={v=>nested(update,'rotation','stick',v)}/>
      <Numbers object={m.rotation} specs={[["sensitivity","旋转倍率",0.01,100,0.1,"×"],["deadzone","旋转死区",0,0.95,0.01],["smooth_ms","拇指方向稳定时间",0,1000,1," ms"]]} onChange={(k,v)=>nested(update,'rotation',k,v)}/>
      <Toggle label="相对模式：逐渐返回拇指方向" value={m.rotation.relative} onChange={v=>nested(update,'rotation','relative',v)}/>
      {m.rotation.relative&&<Range label="回归时间" value={m.rotation.return_ms} min={0} max={5000} step={10} unit=" ms" slider={false} onChange={v=>nested(update,'rotation','return_ms',v)}/>}
      <Toggle label="手腕转动时接管瞄准方向" value={m.rotation.takeover} onChange={v=>nested(update,'rotation','takeover',v)}/>
      {m.rotation.takeover&&<Numbers object={m.rotation} specs={[["takeover_dps","体感接管速度",0,2000,1," °/s"],["outer","外圈交还拇指阈值",0.05,1,0.01]]} onChange={(k,v)=>nested(update,'rotation',k,v)}/>}
      <p className={styles.hint}>摇杆决定粗略方向，手腕细调角度。推力保持不变，摇杆回到中心会清除角度偏移。</p>
    </>}
  </>;
}
export function FlickStickSettings({profile,update}:Props) {
  if(isSensorOutput(profile.gyro.output))return <SettingSection id="flick-main" title="快速转向" summary="传感器直通期间停用">
    <p className={styles.hint}>切换到鼠标或虚拟右摇杆体感输出后，可使用拨动和配套摇杆瞄准。</p>
  </SettingSection>;
  const f=profile.gyro.motion.flick;
  const change=(key:string,value:unknown)=>nested(update,'flick',key,value);
  const setControl=(key:'enable_input'|'pause_input',value:string|null)=>update(p=>{
    const flick=p.gyro.motion.flick;
    flick[key]=value;
    if(value)flick[key==='enable_input'?'pause_input':'enable_input']=null;
  });
  const mode=f.enabled?(f.rotate_only?'rotate_only':f.only?'flick_only':'flick'):f.stick_aim?'aim':'off';
  const active=mode!=='off';
  const nativePad=profile.gyro.output==='right_stick';
  const continuous=mode==='aim';
  const modes=[['off','关闭'],['flick','拨动／转动'],['flick_only','仅拨动'],['rotate_only','仅转动'],['aim','连续摇杆瞄准']] as const;
  return <SettingSection id="flick-main" title="快速转向" defaultOpen summary={modes.find(([value])=>value===mode)?.[1]}>
      <Choice label="摇杆模式" value={mode} options={modes}
        settingPath={['gyro.motion.flick.enabled','gyro.motion.flick.only','gyro.motion.flick.rotate_only','gyro.motion.flick.stick_aim']}
        onChange={v=>update(p=>{Object.assign(p.gyro.motion.flick,{
          enabled:v==='flick'||v==='flick_only'||v==='rotate_only', only:v==='flick_only', rotate_only:v==='rotate_only', stick_aim:v==='aim',
        });})}/>
      {active&&<Choice label="拨动使用的摇杆" value={f.stick} options={sticks} onChange={v=>change('stick',v)}/>}
      {f.enabled&&<>
      <Range label="拨动转身阈值" value={f.deadzone} min={0.05} max={1} step={0.01} onChange={v=>change('deadzone',v)}/>
      {!f.rotate_only&&<section className={styles.flickGroup}>
        <h3>拨动</h3>
        <Numbers object={f} specs={[["forward_deadzone","正前方死角",0,180,1,"°"]]} onChange={change}/>
        <Choice label="拨动角度吸附" value={f.snap_mode??'none'}
          options={[["none","不吸附"],["four","四向（每 90°）"],["eight","八向（每 45°）"]]} onChange={v=>change('snap_mode',v)}/>
        {(f.snap_mode??'none')!=='none'&&<Range label="拨动角度吸附强度" value={(f.snap_strength??1)*100} min={0} max={100} step={1} unit="%"
          onChange={v=>change('snap_strength',v/100)} help="100% 对齐最近方向，0% 保留原始角度，中间值柔和偏向最近方向。只作用于初始拨动，转动保持连续。"/>}
        {nativePad?<p className={styles.hint}>{`按当前游戏满杆角速度，转过 90° 约需 ${Math.round(90000/profile.gyro.stick_full_scale_dps)} ms。`}</p>:<div className={settingStyles.columns}>
          <div>
            <Numbers object={f} specs={[["smooth_ms","180° 基准释放时间",0,2000,1," ms"]]} onChange={change}/>
            <Range label="按转角调整时间" value={f.time_exponent??0} min={0} max={2} step={0.05} onChange={v=>change('time_exponent',v)}
              help="时间指数：0 为固定时间（默认），1 按转角等比例缩短，2 让小角度转身更快。上方释放时间以 180° 为基准，只影响初始拨动，不影响转动。"/>
            <p className={styles.hint}>预计转角释放时间：45° {Number((f.smooth_ms*Math.pow(0.25,f.time_exponent??0)).toFixed(1))} ms · 90° {Number((f.smooth_ms*Math.pow(0.5,f.time_exponent??0)).toFixed(1))} ms · 180° {f.smooth_ms} ms</p>
          </div>
          <div><FlickReleaseCurve strength={0.5}/></div>
        </div>}
      </section>}
      {!f.only&&<section className={styles.flickGroup}>
        <h3>转动</h3>
        <Range label="外圈转动平滑阈值" value={f.rotate_smooth_override??-1} min={-1} max={3.14} step={0.01} slider={false}
          onChange={v=>change('rotate_smooth_override',v)} help="单位为弧度。负值使用默认阈值，设为 0 可关闭外圈转动平滑。"/>
      </section>}
      </>}
      {continuous&&<section className={styles.flickGroup}>
        <h3>配套摇杆瞄准</h3>
        <Numbers object={f} specs={[["aim_sensitivity","摇杆瞄准倍率",0.01,100,0.1,"×"],["max_dps","连续瞄准基础转速",1,4000,1," °/s"]]} onChange={change}/>
        {!nativePad&&<Numbers object={f} specs={[["vertical_ratio","摇杆纵向比例",0.01,10,0.05,"×"]]} onChange={change}/>}
        <p className={styles.hint}>这些参数只调整连续瞄准，不改变拨动的目标转角或游戏本身的满杆转速。</p>
      </section>}
      {active&&<section className={styles.flickGroup}>
        <h3>共用启用与输出规则</h3>
        <Toggle label="关闭体感时也启用" value={f.standalone??false} onChange={v=>change('standalone',v)}/>
        <ControlInput label="按住启用（不设置则常开）" value={f.enable_input} onChange={v=>setControl('enable_input',v)}/>
        <ControlInput label="按住暂停拨动" value={f.pause_input??null} onChange={v=>setControl('pause_input',v)}/>
        <p className={styles.hint}>按住启用与按住暂停互斥，设置一个会清除另一个；都不设置时常开，仍受功能启用开关控制。</p>
        <Choice label="原始摇杆输出" value={f.suppress} options={[["full","使用时屏蔽"],["deadzone","只屏蔽外圈"],["off","保留原始输出"]]} onChange={v=>change('suppress',v)}/>
        {!f.standalone&&!profile.gyro.enabled&&<p className={styles.hint}>请在「体感」页启用体感，以使用拨动／转动和配套摇杆瞄准。</p>}
        <p className={styles.hint}>这些规则由拨动／转动和配套摇杆瞄准共用。</p>
      </section>}
  </SettingSection>;
}

type GameCalibrationProps = {
  canMeasure: boolean;
  onOpenCalibration: () => void;
};

export function GameAngleCalibration({profile, canMeasure, onOpenCalibration}: GameCalibrationProps & {profile:Profile}) {
  return <>
    <p className={styles.hint}>保存并应用基础层配置，启用映射后打开悬浮窗。对准参照物试转 360°，直接调整数值后再次试转，满意后保存。原来的手动转动测量保留在「更多校准方式」。</p>
    {profile.gyro.output==='right_stick'&&<p className={styles.hint}>{usesJsmAim(profile.gyro)
      ? '先校准游戏满杆角速度。若慢速移动被游戏忽略，可按游戏设置调整右摇杆内死区补偿和响应指数。'
      : '使用摇杆拨动前，请将游戏摇杆响应曲线设为线性，并关闭边缘及其他转向加速。校准后保持游戏灵敏度不变。'}</p>}
    <div className="studio-actions">
      <button type="button" className="primary-btn" disabled={!canMeasure||(!profile.gyro.enabled&&!(profile.gyro.motion.flick.standalone&&(profile.gyro.motion.flick.enabled||profile.gyro.motion.flick.stick_aim)))}
        onClick={onOpenCalibration}>打开校准悬浮窗</button>
    </div>
    </>;
}

export const MotionOutputSettings = memo(function MotionOutputSettings({profile,update,canMeasure,onOpenCalibration,vigemGamepad}:
  Props&GameCalibrationProps&{vigemGamepad:boolean}) {
  useLanguage();
  const gyro=profile.gyro;
  const passthrough=isSensorOutput(gyro.output);
  const isDs4=profile.virtual_gamepad==='dualshock_4';
  const isDualSense=profile.virtual_gamepad==='dualsense';
  const sensorOutput=isDualSense?'dualsense_imu':'ds4_imu';
  const canUsePassthrough=isDualSense||(isDs4&&vigemGamepad);
  const outputLocked=gyro.motion.mode!=='pointer'&&!passthrough&&!canUsePassthrough;
  const showMouseScale=passthrough
    ? gyro.motion.turns.some(turn=>turn.output!=='right_stick')
    : gyro.output==='mouse';
  const showStickScale=passthrough
    ? gyro.motion.turns.some(turn=>turn.output!=='mouse')
    : gyro.output==='right_stick';
  return <SettingSection id="motion-calibration" title={passthrough?'体感输出':'输出与转角校准'}
    summary={passthrough?sensorOutputLabel(sensorOutput):gyro.output==='mouse'?'鼠标':'虚拟右摇杆'}>
    <SettingField label="体感输出" disabled={outputLocked}>
      <div className={settingStyles.row}><SettingLabel label="体感输出"/><select aria-label="体感输出" value={gyro.output}
        disabled={outputLocked} onChange={event=>{
          const output=event.currentTarget.value as Profile['gyro']['output'];
          update(p=>{p.gyro.output=output;});
        }}>
        <option value="mouse" disabled={gyro.motion.mode!=='pointer'}>鼠标</option><option value="right_stick" disabled={profile.virtual_gamepad==='none'}>虚拟手柄右摇杆</option>
        <option value={sensorOutput} disabled={!canUsePassthrough}>{isDualSense||isDs4?sensorOutputLabel(sensorOutput):'传感器直通'}</option>
      </select></div>
    </SettingField>
    {isDs4&&!vigemGamepad&&<p className={settingStyles.notice}>请在「设置与组件」将虚拟手柄后端设为 ViGEm，再使用 DS4 传感器直通。</p>}
    {!isDs4&&!isDualSense&&<p className={styles.hint}>选择虚拟 DualSense（PS5）或使用 ViGEm 的虚拟 DualShock 4，即可直通六轴传感器。</p>}
    {passthrough?<>
      <p className={styles.hint}>{isDualSense?'通过 USB/IP 向虚拟 DualSense 发送校准后的陀螺仪和加速度计数据。可在支持 DualSense 体感的游戏或 Steam 输入中设置瞄准。':'向虚拟 DualShock 4 发送经设备校准的陀螺仪和加速度计数据。请在支持读取 DS4 传感器的游戏或 Steam 输入中设置体感。'}</p>
      {(showStickScale||showMouseScale)&&<p className={styles.hint}>下面的转角换算用于按键触发的定角转身。</p>}
    </>:<>
      <p className={styles.hint}>体感、拨动和配套摇杆瞄准共用这里的输出与转角换算。</p>
      {profile.virtual_gamepad==='none'&&<p className={styles.hint}>在游戏配置页头选择虚拟手柄，即可使用右摇杆输出。</p>}
    </>}
    {showStickScale&&<Range label="游戏满杆角速度" value={gyro.stick_full_scale_dps} min={30} max={1440} step={1} unit=" °/s" slider={false}
      onChange={v=>update(p=>{p.gyro.stick_full_scale_dps=v;})} help="填写游戏实际满杆时的转向速度，用于将目标转角换算成摇杆输出。提高这里的值不会让游戏转得更快。"/>}
    {showStickScale&&!passthrough&&(usesJsmAim(gyro)||gyro.motion.flick.enabled||gyro.motion.flick.stick_aim)&&<>
      <Range label="右摇杆内死区补偿" value={(gyro.jsm_response?.right_undeadzone_inner??0)*100} min={0} max={99-Math.round((gyro.jsm_response?.right_undeadzone_outer??0)*100)} step={1} unit="%"
        onChange={v=>update(p=>{p.gyro.jsm_response={...p.gyro.jsm_response,right_undeadzone_inner:v/100};})}
        help="填入游戏忽略的摇杆中心范围。手柄静止时，输出会停在这个边界，方便观察镜头是否开始转动。"/>
      <Range label="右摇杆外死区补偿" value={(gyro.jsm_response?.right_undeadzone_outer??0)*100} min={0} max={99-Math.round((gyro.jsm_response?.right_undeadzone_inner??0)*100)} step={1} unit="%"
        onChange={v=>update(p=>{p.gyro.jsm_response={...p.gyro.jsm_response,right_undeadzone_outer:v/100};})}/>
      <Range label="右摇杆响应指数" value={gyro.jsm_response?.right_unpower??0} min={0} max={10} step={0.05}
        onChange={v=>update(p=>{p.gyro.jsm_response={...p.gyro.jsm_response,right_unpower:v};})}
        help="填入游戏的摇杆响应指数以补偿曲线。线性响应可用 0 或 1。"/>
    </>}
    {showMouseScale&&<Range label="基础鼠标计数" value={gyro.counts_per_degree} min={1} max={100} step={0.5} unit=" /°" slider={false}
      onChange={v=>update(p=>{p.gyro.counts_per_degree=v;})} help="每旋转一度产生的基础鼠标计数，再乘以体感灵敏度。"/>}
    {!passthrough&&<GameAngleCalibration profile={profile} canMeasure={canMeasure} onOpenCalibration={onOpenCalibration}/>}
  </SettingSection>;
});
