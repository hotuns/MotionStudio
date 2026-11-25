import {useId,useState} from 'react';
import {Dialog, Range, Toggle} from './Controls';
import {inputConfig, inputDeadzone, setInputDeadzone} from '../controller-input';
import type {ControllerInputConfig, Profile, StickDirections} from '../types';
import native from '../native-input-defaults.json';
import styles from './MotionControls.module.css';
import {SettingField,SettingLabel,SettingSection,settingStyles} from './SettingLayout';
import {StickOverlapPreview} from './StickOverlapPreview';

type Props = {profile: Profile; update: (change: (profile: Profile) => void) => void};

export function StickInputSettings({profile,update}:Props) {
  return <SettingSection id="stick-input" title="基础输入与防回弹"
    summary={<>左 {Math.round(inputDeadzone(profile,'left')*100)}% · 右 {Math.round(inputDeadzone(profile,'right')*100)}%</>}>
    <div className={settingStyles.fieldGrid}>
      {(['left','right'] as const).map(side=><Range key={side}
        label={side==='left'?'左摇杆输入死区':'右摇杆输入死区'}
        settingPath={['input.'+(side==='left'?'deadzone_left':'deadzone_right'),...(profile.stick_mouse.stick===side?['stick_mouse.deadzone']:[])]}
        value={inputDeadzone(profile,side)*100} min={0} max={94} step={1} unit="%"
        onChange={v=>update(p=>setInputDeadzone(p,side,v/100))}/>)}
    </div>
    <p className={styles.hint}>摇杆鼠标、快速转向、方向映射和虚拟摇杆共用这里的输入死区。</p>
    <Toggle label="启用摇杆防回弹" settingPath="stick_rebound_ms" value={profile.stick_rebound_ms!=null}
      onChange={enabled=>update(p=>{p.stick_rebound_ms=enabled?40:null;})}/>
    {profile.stick_rebound_ms!=null&&<Range label="防回弹窗口" value={profile.stick_rebound_ms} min={10} max={150} step={5} unit=" ms" slider={false}
      onChange={ms=>update(p=>{p.stick_rebound_ms=ms;})}/>}
    <p className={styles.hint}>抑制快速松杆时越过中心造成的短暂反向输入。默认关闭，可按手柄手感启用。</p>
  </SettingSection>;
}

export function StickDirectionSettings({profile,update}:Props) {
  const c=inputConfig(profile);
  const directionId=useId();
  const defaultOverlap=Number((90-2*Math.atan(native.stick_direction_ratio)*180/Math.PI).toFixed(2));
  return <SettingSection id="stick-directions" title="摇杆方向映射" summary="按轴阈值或八向重叠">
    <div className={settingStyles.fieldGrid}>
      {(['lstick_directions','rstick_directions'] as const).map((key,index)=>{
        const overlapKey=index===0?'lstick_overlap_degrees':'rstick_overlap_degrees';
        const degrees=c[overlapKey]??defaultOverlap;
        const label=index===0?'左摇杆方向判定':'右摇杆方向判定';
        const id=directionId+'-'+key;
        return <div key={key}>
          <SettingField label={label} path={'input.'+key}>
            <div className={settingStyles.row}>
              <SettingLabel label={label} htmlFor={id}/>
              <select id={id} aria-label={label} value={c[key]??'axis_threshold'} onChange={event=>update(p=>{p.input={...inputConfig(p),[key]:event.target.value as StickDirections};})}>
                <option value="eight_way">八向重叠（适合 WASD）</option><option value="axis_threshold">按轴阈值</option>
              </select>
            </div>
          </SettingField>
          {c[key]==='eight_way'&&<>
            <Range label={index===0?'左摇杆斜向重叠角度':'右摇杆斜向重叠角度'} settingPath={'input.'+overlapKey}
              value={degrees} min={0} max={90} step={0.01} unit="°"
              onChange={value=>update(p=>{p.input={...inputConfig(p),[overlapKey]:value};})}/>
            <StickOverlapPreview degrees={degrees} label={index===0?'左摇杆方向区域预览':'右摇杆方向区域预览'}
              onReset={()=>update(p=>{p.input={...inputConfig(p),[overlapKey]:null};})}/>
          </>}
        </div>;
      })}
    </div>
    <p className={styles.hint}>八向重叠：斜推同时触发相邻两键，例如 W+D；映射阈值按摇杆整体推离中心的幅度判断。按轴阈值：水平和垂直方向分别达到阈值才触发。</p>
    {(c.lstick_directions==='eight_way'||c.rstick_directions==='eight_way')&&<p className={styles.hint}>重叠角度越大，同时触发相邻两方向的范围越宽。0° 只触发单方向，90° 时除正方向外均触发相邻两方向。默认约 36.87°。</p>}
  </SettingSection>;
}

export function StickCalibrationSettings({profile,update}:Props) {
  const c=inputConfig(profile);
  const [radial,setRadial]=useState<{left:string;right:string}|null>(null);
  const [error,setError]=useState<string|null>(null);
  return <SettingSection id="stick-calibration" title="中心与圆周校准" summary="偏移补偿与方向倍率">
    <p className={styles.hint}>中心偏移使用右为正、上为正的坐标。补偿随当前配置保存。</p>
    <div className={settingStyles.fieldGrid}>
      {(['lstick_center','rstick_center'] as const).map((key,side)=>
        <SettingField key={key} label={side===0?'左摇杆中心偏移':'右摇杆中心偏移'} path={'input.'+key}>
          {[0,1].map(axis=><Range key={axis} settingPath={[]} slider={false}
            label={side===0?(axis===0?'左摇杆中心水平偏移':'左摇杆中心垂直偏移'):(axis===0?'右摇杆中心水平偏移':'右摇杆中心垂直偏移')}
            value={c[key][axis]} min={-1} max={1} step={0.001} onChange={value=>update(p=>{
              const input=inputConfig(p),center=[...input[key]];center[axis]=value;p.input={...input,[key]:center};
            })}/>)}
        </SettingField>)}
      {(['lstick_radial','rstick_radial'] as const).map((key,index)=><SettingField key={key}
        label={index===0?'左摇杆圆周倍率':'右摇杆圆周倍率'} path={'input.'+key}>
        <SettingLabel label={index===0?'左摇杆圆周倍率':'右摇杆圆周倍率'}/><p className={styles.hint}>{c[key].every(v=>v===1)?'无修正':'已自定义'}</p>
      </SettingField>)}
    </div>
    <button type="button" className="secondary-btn" onClick={()=>{
      setError(null);setRadial({left:JSON.stringify(c.lstick_radial),right:JSON.stringify(c.rstick_radial)});
    }}>编辑摇杆圆周校准</button>
    {radial&&<Dialog title="编辑摇杆圆周校准" onClose={()=>setRadial(null)} error={error??undefined}
      actions={<><button type="button" className="ghost-btn" onClick={()=>setRadial(null)}>取消</button><button type="button" className="primary-btn" onClick={()=>{
        try {
          const left:unknown=JSON.parse(radial.left),right:unknown=JSON.parse(radial.right);
          for(const values of [left,right])if(!Array.isArray(values)||values.length!==native.radial_buckets||values.some(v=>typeof v!=='number'||!Number.isFinite(v)||v<0.05||v>20))throw new Error('圆周校准需要 72 个 0.05–20 的有限倍率。');
          update(p=>{p.input={...inputConfig(p),lstick_radial:left as number[],rstick_radial:right as number[]};});setRadial(null);
        }catch(e){setError(String(e));}
      }}>完成</button></>}>
      <p>每个摇杆使用 72 个方向倍率，依次对应每 5° 的圆周采样。全为 1 表示不修正圆周。可粘贴已有校准数组。</p>
      <label className="studio-field">左摇杆圆周倍率<textarea aria-label="左摇杆圆周倍率" spellCheck={false} value={radial.left} onChange={e=>{setRadial({...radial,left:e.target.value});setError(null);}}/></label>
      <label className="studio-field">右摇杆圆周倍率<textarea aria-label="右摇杆圆周倍率" spellCheck={false} value={radial.right} onChange={e=>{setRadial({...radial,right:e.target.value});setError(null);}}/></label>
      <button type="button" className="secondary-btn" onClick={()=>{setRadial({left:JSON.stringify(Array(native.radial_buckets).fill(1)),right:JSON.stringify(Array(native.radial_buckets).fill(1))});setError(null);}}>恢复单位圆倍率</button>
    </Dialog>}
  </SettingSection>;
}

export function TriggerInputSettings({profile, update}: Props) {
  const c = inputConfig(profile);
  const set = <K extends keyof ControllerInputConfig>(key: K, value: ControllerInputConfig[K]) =>
    update(p=>{p.input={...inputConfig(p),[key]:value};});
  const number = (key: 'ltrig_min'|'ltrig_max'|'rtrig_min'|'rtrig_max'|'ltrig_digital_threshold'|'rtrig_digital_threshold'|'trig_hysteresis'|'trig_debounce_ms',
    label: string, min=0, max=1, step=0.01) =>
    <Range label={label} value={c[key]} min={min} max={max} step={step} slider={key!=='trig_debounce_ms'} unit={key==='trig_debounce_ms'?' ms':''} onChange={v=>set(key,v)}/>;

  return <><SettingSection id="trigger-input" title="扳机行程" summary="最小与最大输入">
    <p className={styles.hint}>按当前配置修正左右扳机的最小和最大输入。全拉按修正后的最大行程识别。</p>
    <div className={settingStyles.fieldGrid}>
      {number('ltrig_min','左扳机最小原始值')}{number('rtrig_min','右扳机最小原始值')}
      {number('ltrig_max','左扳机最大原始值')}{number('rtrig_max','右扳机最大原始值')}
    </div>
    </SettingSection>
    <SettingSection id="trigger-digital" title="数字扳机" summary={c.digital_triggers?'已启用':'关闭'}>
      <Toggle label="使用数字扳机" value={c.digital_triggers} onChange={v=>set('digital_triggers',v)}/>
      {c.digital_triggers&&<div className={settingStyles.fieldGrid}>
        {number('ltrig_digital_threshold','左扳机数字阈值')}{number('rtrig_digital_threshold','右扳机数字阈值')}
        {number('trig_hysteresis','扳机数字回差',0,0.5)}
        {number('trig_debounce_ms','扳机松开防抖时间',native.trigger_debounce_ms,5000,1)}
      </div>}
      <p className={styles.hint}>数字扳机达到阈值时输出满行程。轻拉与全拉仍根据实际行程区分。</p>
    </SettingSection>
  </>;
}
