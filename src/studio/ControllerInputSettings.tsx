import {useState} from 'react';
import {Dialog, Range, Toggle} from './Controls';
import {inputConfig, inputDeadzone, setInputDeadzone} from '../controller-input';
import type {ControllerInputConfig, Profile} from '../types';
import native from '../native-input-defaults.json';
import styles from './MotionControls.module.css';

type Props = {profile: Profile; update: (change: (profile: Profile) => void) => void};

export function StickInputSettings({profile, update}: Props) {
  const c = inputConfig(profile);
  const [radial,setRadial]=useState<{left:string;right:string}|null>(null);
  const [error,setError]=useState<string|null>(null);
  return <section className={styles.group}>
    <div className={styles.groupHeading}><h3>摇杆输入死区</h3></div>
    <div className={styles.grid}>
      <Range label="左摇杆输入死区" value={inputDeadzone(profile,'left')} min={0} max={0.94} step={0.01}
        onChange={v=>update(p=>setInputDeadzone(p,'left',v))}/>
      <Range label="右摇杆输入死区" value={inputDeadzone(profile,'right')} min={0} max={0.94} step={0.01}
        onChange={v=>update(p=>setInputDeadzone(p,'right',v))}/>
    </div>
    <p className={styles.hint}>摇杆鼠标、Flick Stick、方向映射和虚拟摇杆共用这里的输入死区。</p>
    <details>
      <summary>高级摇杆校准</summary>
      <p className={styles.hint}>中心偏移使用右为正、上为正的坐标。补偿随当前配置保存。</p>
      <div className={styles.grid}>{(['lstick_center','rstick_center'] as const).flatMap((key, side)=>[0,1].map(axis=>
        <Range key={key+axis} label={side===0?(axis===0?'左摇杆中心水平偏移':'左摇杆中心垂直偏移'):(axis===0?'右摇杆中心水平偏移':'右摇杆中心垂直偏移')}
          value={c[key][axis]} min={-1} max={1} step={0.001} onChange={value=>update(p=>{
            const input = inputConfig(p);
            const center = [...input[key]];
            center[axis] = value;
            p.input = {...input,[key]:center};
          })}/>
      ))}</div>
      <button type="button" className="secondary-btn" onClick={()=>{
        setError(null);
        setRadial({left:JSON.stringify(c.lstick_radial),right:JSON.stringify(c.rstick_radial)});
      }}>编辑摇杆圆周校准</button>
    </details>
    {radial&&<Dialog title="编辑摇杆圆周校准" onClose={()=>setRadial(null)} error={error??undefined} actions={<><button type="button" className="ghost-btn" onClick={()=>setRadial(null)}>取消</button><button type="button" className="primary-btn" onClick={()=>{
      try{const left:unknown=JSON.parse(radial.left),right:unknown=JSON.parse(radial.right);for(const values of [left,right])if(!Array.isArray(values)||values.length!==native.radial_buckets||values.some(v=>typeof v!=='number'||!Number.isFinite(v)||v<0.05||v>20))throw new Error('圆周校准需要 72 个 0.05–20 的有限倍率。');update(p=>{p.input={...inputConfig(p),lstick_radial:left as number[],rstick_radial:right as number[]};});setRadial(null);}catch(e){setError(String(e));}
    }}>完成</button></>}>
      <p>每个摇杆使用 72 个方向倍率，依次对应每 5° 的圆周采样。全为 1 表示不修正圆周。可粘贴已有校准数组。</p>
      <label className="studio-field">左摇杆圆周倍率<textarea aria-label="左摇杆圆周倍率" spellCheck={false} value={radial.left} onChange={e=>{setRadial({...radial,left:e.target.value});setError(null);}}/></label>
      <label className="studio-field">右摇杆圆周倍率<textarea aria-label="右摇杆圆周倍率" spellCheck={false} value={radial.right} onChange={e=>{setRadial({...radial,right:e.target.value});setError(null);}}/></label>
      <button type="button" className="secondary-btn" onClick={()=>{setRadial({left:JSON.stringify(Array(native.radial_buckets).fill(1)),right:JSON.stringify(Array(native.radial_buckets).fill(1))});setError(null);}}>恢复单位圆倍率</button>
    </Dialog>}
  </section>;
}

export function TriggerInputSettings({profile, update}: Props) {
  const c = inputConfig(profile);
  const set = <K extends keyof ControllerInputConfig>(key: K, value: ControllerInputConfig[K]) =>
    update(p=>{p.input={...inputConfig(p),[key]:value};});
  const number = (key: 'ltrig_min'|'ltrig_max'|'rtrig_min'|'rtrig_max'|'ltrig_digital_threshold'|'rtrig_digital_threshold'|'trig_hysteresis'|'trig_debounce_ms',
    label: string, min=0, max=1, step=0.01) =>
    <Range label={label} value={c[key]} min={min} max={max} step={step} onChange={v=>set(key,v)}/>;

  return <section className={styles.group}>
    <div className={styles.groupHeading}><h3>扳机行程</h3></div>
    <p className={styles.hint}>按当前配置修正左右扳机的最小和最大输入。全拉按修正后的最大行程识别。</p>
    <div className={styles.grid}>
      {number('ltrig_min','左扳机最小原始值')}{number('ltrig_max','左扳机最大原始值')}
      {number('rtrig_min','右扳机最小原始值')}{number('rtrig_max','右扳机最大原始值')}
    </div>
    <details>
      <summary>高级数字扳机设置</summary>
      <Toggle label="使用数字扳机" value={c.digital_triggers} onChange={v=>set('digital_triggers',v)}/>
      {c.digital_triggers&&<div className={styles.grid}>
        {number('ltrig_digital_threshold','左扳机数字阈值')}{number('rtrig_digital_threshold','右扳机数字阈值')}
        {number('trig_hysteresis','扳机数字回差',0,0.5)}
        {number('trig_debounce_ms','扳机松开防抖时间',native.trigger_debounce_ms,5000,1)}
      </div>}
      <p className={styles.hint}>数字扳机达到阈值时输出满行程。轻拉与全拉仍根据实际行程区分。</p>
    </details>
  </section>;
}
