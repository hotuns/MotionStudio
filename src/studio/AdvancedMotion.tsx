import type { MotionConfig, Profile, Status } from '../types';
import { Range, Toggle } from './Controls';
import {SettingField, SettingLabel, SettingSection, settingStyles} from './SettingLayout';
import {OptionPicker} from './OptionPicker';
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
  return <SettingField label={label}><div className="studio-field"><SettingLabel label={label}/><OptionPicker label={label} value={value??''}
    groups={[{options:[{value:'',label:'不设置'}]}, ...padChoices()]} onChange={v=>onChange(v||null)}/></div></SettingField>;
}
const sticks:[string,string][]=[['right','右摇杆'],['left','左摇杆']];
const spaces:[string,string][]=[['pitch_yaw','本地：俯仰与偏航'],['pitch_roll','本地：俯仰与横滚'],['player','玩家坐标'],['world','世界坐标']];
const mutate=(update:MotionUpdate,key:string,value:unknown)=>update(p=>{(p.gyro.motion as unknown as Record<string,unknown>)[key]=value;});
const nested=(update:MotionUpdate,group:'edge'|'rotation'|'flick'|'trackball',key:string,value:unknown)=>update(p=>{(p.gyro.motion[group] as unknown as Record<string,unknown>)[key]=value;});

export function MotionBehavior({profile,update}:Props) {
  const m=profile.gyro.motion;const set=(k:string,v:unknown)=>mutate(update,k,v);
  return <>
    <SettingSection id="motion-direction" title="方向与握持方式" summary={spaces.find(([value])=>value===m.space)?.[1]}>
      <Choice label="体感坐标" value={m.space} options={spaces} onChange={v=>set('space',v)}/>
      <p className={styles.hint}>玩家和世界坐标依靠加速度计估计重力方向。世界坐标的重力参考变化更缓慢。</p>
      <Toggle label="反转垂直方向" value={profile.gyro.invert_y} onChange={v=>update(p=>{p.gyro.invert_y=v;})}/>
      <Toggle label="反转水平方向" value={m.invert_x} onChange={v=>set('invert_x',v)}/>
      {m.space==='pitch_yaw'&&<Range label="横滚参与水平转向" value={m.roll_contribution} min={-100} max={100} step={1} unit="%" onChange={v=>set('roll_contribution',v)}/>}
      <SettingField label="习惯握持角度" path="gyro.motion.hold_angles">
        {['俯仰','偏航','横滚'].map((label,i)=><Range key={label} label={label+'补偿'} settingPath={[]} slider={false} value={m.hold_angles[i]} min={-180} max={180} step={1} unit="°" onChange={v=>update(p=>{p.gyro.motion.hold_angles[i]=v;})}/>)}
      </SettingField>
    </SettingSection>
    <SettingSection id="motion-posture" title="控制方式与姿态" summary={m.mode==='pointer'?'角速度瞄准':m.mode==='steering'?'姿态控制摇杆':'体感旋转摇杆方向'}>
      <Choice label="控制方式" value={m.mode} settingPath={['gyro.motion.mode','gyro.output','gyro.motion.edge.mode']}
        options={[["pointer","角速度瞄准"],["steering","姿态控制摇杆"],["stick_rotation","体感旋转摇杆方向"]]} onChange={v=>update(p=>{
          p.gyro.motion.mode=v as MotionConfig['mode'];
          if(v!=='pointer') {p.gyro.output='right_stick';p.gyro.motion.edge.mode='off';}
        })}/>
      <ControlInput label="姿态归零按键" value={m.reset_input} onChange={v=>set('reset_input',v)}/>
      <ControlInput label="设置正前方按键" value={m.neutral_input} onChange={v=>set('neutral_input',v)}/>
      <p className={styles.hint}>姿态归零重新记录当前朝向。设置正前方只重置水平朝向，保留重力方向上的俯仰与横滚。</p>
      <AdvancedTurning profile={profile} update={update}/>
    </SettingSection>
    <SettingSection id="motion-inertia" title="Trackball 惯性体感" summary={m.trackball.horizontal_input||m.trackball.vertical_input?'已设置惯性按键':'未设置'}>
        <ControlInput label="保持水平惯性" value={m.trackball.horizontal_input} onChange={v=>nested(update,'trackball','horizontal_input',v)}/>
        <ControlInput label="保持垂直惯性" value={m.trackball.vertical_input} onChange={v=>nested(update,'trackball','vertical_input',v)}/>
        <Range label="惯性衰减" value={m.trackball.decay} min={0} max={20} step={0.1} onChange={v=>nested(update,'trackball','decay',v)} help="按住指定按键后沿之前的方向继续转动；0 表示保持速度。松开后恢复手腕控制。"/>
    </SettingSection>
    <EdgeTurning profile={profile} update={update}/>
  </>;
}
export function AccelerationSettings({profile,update}:Props) {
  const a=profile.gyro.acceleration;
  const set=(key:string,value:unknown)=>update(p=>{(p.gyro.acceleration as unknown as Record<string,unknown>)[key]=value;});
  return <SettingSection id="motion-acceleration" title="转动加速" summary={a.fast_multiplier>1?`${a.fast_multiplier}×`:'关闭'} lazy>
    <div className={settingStyles.columns}><div>
    <Range label="快速转动倍率" value={a.fast_multiplier} min={1} max={4} step={0.1} unit="×" onChange={v=>set('fast_multiplier',v)}
      help="慢速微调沿用基础灵敏度。快速转动时逐步提高倍率，1× 关闭加速。"/>
    {a.fast_multiplier>1&&<>
      <Range label="加速起点" value={a.start_dps} min={0} max={1000} step={1} unit=" °/s" slider={false} onChange={v=>set('start_dps',v)}/>
      <Range label="达到最高倍率" value={a.full_dps} min={1} max={2000} step={1} unit=" °/s" slider={false} onChange={v=>set('full_dps',v)}/>
    </>}
    <Choice label="加速度曲线" value={a.curve} options={[["linear","线性"],["natural","自然"],["power","幂函数"],["quadratic","二次"],["sigmoid","S 曲线"],["jump","跳变"],["custom","自定义控制点"]]} onChange={v=>set('curve',v)}/>
    {a.curve==='natural'&&<Numbers object={a} specs={[["natural_half","达到半程倍率的速度",0.01,4000,1," °/s"]]} onChange={set}/>}
    {a.curve==='power'&&<Numbers object={a} specs={[["power_reference","参考转速",0.01,4000,1," °/s"],["power_exponent","幂指数",0.01,10,0.1]]} onChange={set}/>}
    {a.curve==='sigmoid'&&<Numbers object={a} specs={[["sigmoid_mid","过渡中点",0,4000,1," °/s"],["sigmoid_width","过渡宽度",0.01,2000,1," °/s"]]} onChange={set}/>}
    {a.curve==='jump'&&<Numbers object={a} specs={[["jump_width","跳变过渡宽度",0,2000,1," °/s"]]} onChange={set}/>}
    {a.curve==='custom'&&<SettingField label="自定义控制点"><SettingLabel label="自定义控制点"/><CurveEditor points={a.points} onChange={v=>set('points',v)} maxX={4000} maxY={10} xLabel="转速 °/s" yLabel="倍率" showPreview={false}/></SettingField>}
    </div><div><AccelerationCurve acceleration={a}/></div></div>
  </SettingSection>;
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

    <SettingSection id="motion-smoothing" title="防抖与平滑" defaultOpen
      summary={m.smoothing==='one_euro'?<>One Euro · {m.one_euro_cutoff} Hz</>:<>{m.smoothing==='ema'?'EMA':'Decay'} · {gyro.smoothing_ms} ms</>}>
      <div className={settingStyles.columns + ' ' + styles.smoothingPair}>
        <div>
          <div className={styles.samplingFields}>
            <Choice label="平滑方式" value={m.smoothing} options={[["ema","EMA 时间平滑"],["decay","随速度衰减的平滑"],["one_euro","One Euro 自适应滤波"]]} onChange={v=>set('smoothing',v)}/>
          </div>
          {m.smoothing!=='one_euro'&&<Range label="平滑时间" value={gyro.smoothing_ms} min={0} max={50} step={1} unit=" ms"
            onChange={value=>update(draft=>{draft.gyro.smoothing_ms=value;})} help="按实际采样间隔平滑角速度，0 为关闭。"/>}
          {m.smoothing==='decay'&&<Numbers object={m} specs={[["decay_threshold","退出平滑的转速",0.1,2000,1," °/s"]]} onChange={set}/>}
          {m.smoothing==='one_euro'&&<Numbers object={m} specs={[["one_euro_cutoff","最低截止频率",0.01,100,0.1," Hz"],["one_euro_beta","速度响应系数",0,10,0.01]]} onChange={set}/>}
          <p className={styles.hint}>采样时钟用于实际设备。示例每 2 ms 取样，展示慢转和快转时的平滑效果。</p>
        </div>
        <ResponseCurve title="平滑响应曲线" points={curves?.smoothing} xLabel="时间（ms）" yLabel="角速度（°/s）"/>
      </div>
    </SettingSection>
    <SettingSection id="motion-idle" title="静止抑制与恢复" summary={<>{gyro.noise_threshold} °/s · {m.recovery_ms} ms</>}>
      <div className={styles.stabilityGrid}>
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
      </div>
    </SettingSection>
    <SettingSection id="motion-direction-stability" title="方向稳定与姿态漂移" summary={<>{m.angle_snap}°</>}>
      <div className={styles.stabilityGrid}>
      <div className={settingStyles.columns}>
        <div>
          <Numbers object={m} specs={[["angle_snap","水平／垂直吸附角",0,45,0.5,"°"]]} onChange={set}/>
          <Toggle label="柔和进入角度吸附" value={m.angle_snap_ease} onChange={v=>set('angle_snap_ease',v)}/>
        </div>
          <ResponseCurve title="角度吸附曲线" points={curves?.snap} xLabel="输入方向角（°）" yLabel="输出方向角（°）"/>
      </div>
      <div className={settingStyles.columns}>
        <div>
          <div>
            <Toggle label="使用重力修正姿态漂移" value={m.drift_correction} onChange={v=>set('drift_correction',v)} help="可修正倾斜漂移；加速度计无法观测绕重力方向的水平漂移。"/>
            <p className={styles.hint}>示例展示手柄放稳后，约 6° 的倾斜误差如何随时间变化。</p>
          </div>
        </div>
          <ResponseCurve title="倾斜误差曲线" points={curves?.drift} xLabel="时间（ms）" yLabel="倾斜误差（°）"/>
      </div>
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
function EdgeTurning({profile,update}:Props) {
  const m=profile.gyro.motion;
  return <SettingSection id="motion-edge" title="边缘持续转向" summary={m.edge.mode==='off'?'关闭':'已启用'}>
      {m.mode==='pointer'?<>
        <Choice label="边缘转向方式" value={m.edge.mode} options={[["off","关闭"],["ramp","随转角提速"],["momentum","保持出界速度"],["both","提速与惯性"]]} onChange={v=>nested(update,'edge','mode',v)}/>
        {m.edge.mode!=='off'&&<>
          <Numbers object={m.edge} specs={[["zone","开始持续转向的角度",0,170,1,"°"],["blend","接管过渡区间",0,170,1,"°"],["ramp","达到最大速度的区间",0,170,1,"°"],["max_dps","最大持续转速",0,4000,1," °/s"],["momentum","出界速度保留倍率",0,10,0.1,"×"],["decay_s","惯性衰减时间",0,30,0.1," s"],["gyro_keep","边缘处保留手腕控制",0,1,0.05]]} onChange={(k,v)=>nested(update,'edge',k,v)}/>
          <Toggle label="只作用于水平方向" value={m.edge.horizontal_only} onChange={v=>nested(update,'edge','horizontal_only',v)}/>
          <p className={styles.hint}>先用姿态归零键确定中立位。手腕超过边界后继续转向，向中心回拉可制动。</p>
        </>}
      </>:<p className={styles.hint}>在「控制方式与姿态」选择角速度瞄准后，可配置边缘转向。</p>}
    </SettingSection>;
}

export function FlickStickSettings({profile,update}:Props) {
  const f=profile.gyro.motion.flick;
  const change=(key:string,value:unknown)=>nested(update,'flick',key,value);
  const active=f.enabled||f.stick_aim;
  const release=Math.min(f.deadzone,Math.max(Math.min(0.15,f.deadzone),f.deadzone*(f.release_ratio??0.5)));
  return <>
    <SettingSection id="flick-main" title="Flick Stick" defaultOpen summary={f.enabled?<>{f.smooth_ms} ms · {Math.round((f.release_ease??0.5)*100)}%</>:'关闭'}>
      <Toggle label="启用 Flick Stick" value={f.enabled} onChange={v=>change('enabled',v)}/>
      {f.enabled&&<>
        <Choice label="Flick 使用的摇杆" value={f.stick} options={sticks} onChange={v=>change('stick',v)}/>
        <p className={styles.hint}>快推选择朝向，沿摇杆外圈转动控制镜头。</p>
        <div className={settingStyles.columns}>
          <div>
            <Numbers object={f} specs={[["deadzone","快推转身阈值",0.05,0.99,0.01],["smooth_ms","180° 基准释放时间",0,2000,1," ms"]]} onChange={change}/>
            <Range label="按转角调整时间" value={f.time_exponent??0} min={0} max={2} step={0.05} onChange={v=>change('time_exponent',v)}
              help="时间指数：0 为固定时间（默认），1 按转角等比例缩短，2 让小角度转身更快。上方释放时间以 180° 为基准，只影响初始转身，不影响绕圈追踪。"/>
            <p className={styles.hint}>预计转角释放时间：45° {Number((f.smooth_ms*Math.pow(0.25,f.time_exponent??0)).toFixed(1))} ms · 90° {Number((f.smooth_ms*Math.pow(0.5,f.time_exponent??0)).toFixed(1))} ms · 180° {f.smooth_ms} ms</p>
            <Range label="转角缓出强度" value={(f.release_ease??0.5)*100} min={0} max={100} step={1} unit="%" onChange={v=>change('release_ease',v/100)}
              help="0% 匀速释放，50% 使用 JSM 缓出（默认）。高于 50% 起步更快，100% 使用三次缓出。"/>
          </div>
          <div><FlickReleaseCurve strength={f.release_ease??0.5}/></div>
        </div>
        <p className={styles.hint}>{profile.gyro.output==='right_stick'
          ? '预计时间表示转角释放时间；游戏完成转身仍受实际满杆速度限制。释放时间为 0 时立即提交全部转角。'
          : '释放时间为 0 时立即输出全部转角。'}</p>
      </>}
    </SettingSection>
    <SettingSection id="stick-aim" title="配套摇杆瞄准" defaultOpen summary={f.stick_aim?<>{f.aim_sensitivity}× · {f.max_dps} °/s</>:'关闭'}>
      <Toggle label="启用配套摇杆瞄准" value={f.stick_aim} onChange={v=>change('stick_aim',v)}/>
      {f.stick_aim&&<>
        {!f.enabled&&<Choice label="Flick 使用的摇杆" value={f.stick} options={sticks} onChange={v=>change('stick',v)}/>}
        <Numbers object={f} specs={[["aim_sensitivity","摇杆瞄准倍率",0.01,100,0.1,"×"],["max_dps","连续瞄准基础转速",1,4000,1," °/s"],["vertical_ratio","摇杆纵向比例",0.01,10,0.05,"×"]]} onChange={change}/>
        <p className={styles.hint}>这些参数只调整连续瞄准，不改变 Flick 的目标转角或游戏本身的满杆转速。</p>
      </>}
    </SettingSection>
    {f.enabled&&<><SettingSection id="flick-tracking" title="Flick 追踪与退出" summary={<>追踪下限 {Number((release*100).toFixed(1))}% · {f.disengage_ms??10} ms</>}>
        <Range label="追踪下限比例" value={(f.release_ratio??0.5)*100} min={0} max={100} step={1} unit="%" onChange={v=>change('release_ratio',v/100)}
          help="按快推转身阈值的比例计算，默认 50%。比例越高，绕圈时越要贴近外圈。100% 时追踪下限等于触发阈值。"/>
        <p className={styles.hint}>实际追踪下限 {Number((release*100).toFixed(1))}%</p>
        <p className={styles.hint}>低于最小追踪幅度时按最小值限制，避免接近中心时方向不稳定。</p>
        <Range label="Flick 退出延时" value={f.disengage_ms??10} min={0} max={60} step={1} unit=" ms" slider={false} onChange={v=>change('disengage_ms',v)}
          help="摇杆持续低于追踪下限后，才允许下一次快推转身。默认 10 ms，设为 0 则立即退出。"/>
        <Numbers object={f} specs={[["stabilise_ms","绕圈方向稳定",0,1000,1," ms"]]} onChange={change}/>
    </SettingSection>
    <SettingSection id="flick-trigger" title="Flick 快速判定" summary={f.speed_gate_ms>0?<>{f.speed_gate_ms} ms</>:'不限制推杆时间'}>
        <Numbers object={f} specs={[["forward_deadzone","正前方死角",0,180,1,"°"],["speed_gate_ms","快速推杆判定时间",0,2000,1," ms"]]} onChange={change}/>
        <Toggle label="慢推锁定为连续瞄准" value={f.slow_lock} onChange={v=>change('slow_lock',v)}/>
    </SettingSection>
    </>}
    {active&&<SettingSection id="flick-shared" title="共用启用与输出规则" summary={f.standalone?'关闭体感仍启用':'随体感总开关'}>
        <Toggle label="关闭体感时也启用" value={f.standalone??false} onChange={v=>change('standalone',v)}/>
        <ControlInput label="按住启用（不设置则常开）" value={f.enable_input} onChange={v=>change('enable_input',v)}/>
        <Choice label="原始摇杆输出" value={f.suppress} options={[["full","使用时屏蔽"],["deadzone","只屏蔽外圈"],["off","保留原始输出"]]} onChange={v=>change('suppress',v)}/>
        {!f.standalone&&!profile.gyro.enabled&&<p className={styles.hint}>请在「体感」页启用体感，以使用 Flick Stick 和配套摇杆瞄准。</p>}
        <p className={styles.hint}>这些规则由 Flick Stick 和配套摇杆瞄准共用。</p>
    </SettingSection>}
  </>;
}

export type GameCalibrationProps = {
  status?: Status;
  canMeasure: boolean;
  onMotionAction: (action: string) => void;
  onOpenCalibration: () => void;
};

export function GameAngleCalibration({profile, canMeasure, onOpenCalibration}: Pick<GameCalibrationProps,'canMeasure'|'onOpenCalibration'> & {profile:Profile}) {
  return <>
    <p className={styles.hint}>保存并应用基础层配置，启用映射后打开悬浮窗。对准参照物试转 360°，直接调整数值后再次试转，满意后保存。原来的手动转动测量保留在「更多校准方式」。</p>
    {profile.gyro.output==='right_stick'&&<p className={styles.hint}>使用摇杆 Flick 前，请将游戏摇杆响应曲线设为线性，并关闭边缘及其他转向加速。校准后保持游戏灵敏度不变。</p>}
    <div className="studio-actions">
      <button type="button" className="primary-btn" disabled={!canMeasure||(!profile.gyro.enabled&&!(profile.gyro.motion.flick.standalone&&(profile.gyro.motion.flick.enabled||profile.gyro.motion.flick.stick_aim)))}
        onClick={onOpenCalibration}>打开校准悬浮窗</button>
    </div>
    </>;
}

export function MotionOutputSettings({profile,update,canMeasure,onOpenCalibration}:Props&Pick<GameCalibrationProps,'canMeasure'|'onOpenCalibration'>) {
  const gyro=profile.gyro;
  return <SettingSection id="motion-calibration" title="输出与转角校准" summary={gyro.output==='mouse'?'鼠标':'虚拟右摇杆'}>
    <SettingField label="体感与 Flick 输出" disabled={gyro.motion.mode!=='pointer'}>
      <div className={settingStyles.row}><SettingLabel label="体感与 Flick 输出"/><select aria-label="体感与 Flick 输出" value={gyro.output}
        disabled={gyro.motion.mode!=='pointer'} onChange={event=>update(p=>{p.gyro.output=event.target.value as Profile['gyro']['output'];})}>
        <option value="mouse">鼠标</option><option value="right_stick" disabled={profile.virtual_gamepad==='none'}>虚拟手柄右摇杆</option>
      </select></div>
    </SettingField>
    <p className={styles.hint}>体感、Flick 和配套摇杆瞄准共用这里的输出与转角换算。</p>
    {profile.virtual_gamepad==='none'&&<p className={styles.hint}>在游戏配置页头选择虚拟手柄，即可使用右摇杆输出。</p>}
    {gyro.output==='right_stick'
      ? <Range label="游戏满杆角速度" value={gyro.stick_full_scale_dps} min={30} max={1440} step={1} unit=" °/s" slider={false}
          onChange={v=>update(p=>{p.gyro.stick_full_scale_dps=v;})} help="填写游戏实际满杆时的转向速度，用于将目标转角换算成摇杆输出。提高这里的值不会让游戏转得更快。"/>
      : <Range label="基础鼠标计数" value={gyro.counts_per_degree} min={1} max={100} step={0.5} unit=" /°" slider={false}
          onChange={v=>update(p=>{p.gyro.counts_per_degree=v;})} help="每旋转一度产生的基础鼠标计数，再乘以体感灵敏度。"/>}
    <GameAngleCalibration profile={profile} canMeasure={canMeasure} onOpenCalibration={onOpenCalibration}/>
  </SettingSection>;
}
