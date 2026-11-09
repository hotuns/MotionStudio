import {chordParts,inputs,outputLabel,wheelOutputs,type Action} from '../types';
import {Range,Toggle} from './Controls';
import {OptionPicker} from './OptionPicker';
import {padChoices, outputChoices} from './binding-choices';
import styles from './GameProfilesPage.module.css';
import {modifierOutputs as modifiers} from '../keyboard';
export const inputOpposites:Record<string,string>={left_stick_up:'left_stick_down',left_stick_down:'left_stick_up',left_stick_left:'left_stick_right',left_stick_right:'left_stick_left',right_stick_up:'right_stick_down',right_stick_down:'right_stick_up',right_stick_left:'right_stick_right',right_stick_right:'right_stick_left',dpad_up:'dpad_down',dpad_down:'dpad_up',dpad_left:'dpad_right',dpad_right:'dpad_left',left_stick_inner:'left_stick_outer',left_stick_outer:'left_stick_inner',right_stick_inner:'right_stick_outer',right_stick_outer:'right_stick_inner'};
export const triggerNames={down:'按住',short:'短按',long:'长按',double:'双击',on_press:'按下时触发',on_release:'松开时触发'};
export function ActionTriggerFields({action,onChange,showHint=true}:{action:Action;onChange:(patch:Partial<Action>)=>void;showHint?:boolean}){
 const trigger=action.trigger??'down';
 const choose=(next:Action['trigger'])=>{if(next!==trigger)onChange({trigger:next,trigger_ms:next==='on_press'||next==='on_release'?50:250});};
 return <div className={styles.bindingFields}>
   <div className={styles.triggerModes} role="group" aria-label="动作触发方式">{Object.entries(triggerNames).map(([value,label])=><button type="button" key={value} aria-pressed={trigger===value} onClick={()=>choose(value as Action['trigger'])}>{label}</button>)}</div>
   {trigger!=='down'&&<Range label={trigger==='long'?'长按时间':trigger==='double'?'双击时间窗':trigger==='short'?'短按最长时间':'触发持续时间'} value={action.trigger_ms??250} min={10} max={5000} step={10} unit=" ms" onChange={trigger_ms=>onChange({trigger_ms})}/>}
   {trigger==='long'&&<Toggle label="长按触发后保持，直到松开" value={action.sustain!==false} onChange={sustain=>onChange({sustain})}/>}
   <Toggle label="按住连发" value={action.turbo_ms!=null} onChange={enabled=>onChange({turbo_ms:enabled?100:null})}/>
   {action.turbo_ms!=null&&<Range label="连发周期" value={action.turbo_ms} min={20} max={2000} step={10} unit=" ms" onChange={turbo_ms=>onChange({turbo_ms})}/>}
   {showHint&&<small>普通按住立即生效。短按在松开后确认；双击在第二次按下时生效，保持到松开。同一输入可添加多种触发，短按与长按可避免同时触发普通按住动作。</small>}
 </div>;
}
export function InputChordFields({value,onChange,allowed=inputs,showHint=true,split=false}: {
  value: string;
  onChange: (value: string) => void;
  allowed?: string[];
  showHint?: boolean;
  split?: boolean;
}) {
  const parts=chordParts(value);
  const blocked = (others: string[]) => others.flatMap(input => [input, inputOpposites[input]].filter(Boolean));
  const add = parts.length ? '添加组合按键' : '选择手柄输入';
  return <div className={styles.bindingFields}>
    <span>手柄输入 · 同时按住</span>
    {parts.map((part,i)=><div key={i} className={styles.bindingRow}>
      <OptionPicker label={'手柄输入 '+(i+1)} value={part} groups={padChoices(allowed, blocked(parts.filter((_,j)=>j!==i)), split)}
        onChange={next=>onChange(parts.map((value,j)=>j===i?next:value).join('+'))}/>
      {parts.length>1&&<button type="button" className="ghost-btn" aria-label={'移除手柄输入 '+(i+1)} onClick={()=>onChange(parts.filter((_,j)=>j!==i).join('+'))}>移除</button>}
    </div>)}
    {parts.length<4&&<OptionPicker label={add} placeholder={add} value={null} groups={padChoices(allowed, blocked(parts), split)}
      className={styles.addInput} onChange={next=>onChange([...parts,next].join('+'))}/>}
    {showHint&&<small>最多 4 个输入。组合动作生效时，优先于其中的单键动作。</small>}
  </div>;
}
export function OutputChordFields({action,onChange,showRepeat=true}:{action:Action;onChange:(patch:Partial<Action>)=>void;showRepeat?:boolean}) {
  const parts=chordParts(action.output),mods=modifiers.filter(v=>parts.includes(v)),main=parts.filter(v=>!modifiers.includes(v));
  const emit=(values:string[])=>{
    const p=[...new Set(values)];onChange({output:p.join('+'),repeat_ms:p.some(v=>wheelOutputs.includes(v))?action.repeat_ms??null:null});
  };
  const hasWheel=parts.some(v=>wheelOutputs.includes(v));
  return <div className={styles.bindingFields}>
    <span>键鼠操作 · 同时输出</span>
    <div className={styles.modifiers}>{modifiers.map(mod=><label key={mod}>
      <input type="checkbox" checked={mods.includes(mod)} disabled={!mods.includes(mod)&&parts.length>=4} onChange={e=>{
        const next=modifiers.filter(v=>v===mod?e.target.checked:mods.includes(v));
        emit([...next,...main,...(!next.length&&!main.length?['key:SPACE']:[])]);
      }}/>{outputLabel(mod)}
    </label>)}</div>
    {(main.length?main:['']).map((part,i)=><div className={styles.bindingRow} key={i}>
      <OptionPicker label={'键鼠输出 '+(i+1)} value={part} placeholder="选择键鼠操作"
        groups={[
          ...(mods.length ? [{options:[{value:'',label:'仅使用修饰键'}]}] : []),
          ...outputChoices(false, [...main.filter((_,j)=>j!==i), ...(main.some((v,j)=>j!==i&&wheelOutputs.includes(v))?wheelOutputs:[])]),
        ]} onChange={next=>{
          const values=[...main];values[i]=next;emit([...mods,...values.filter(Boolean)]);
        }}/>
      {main.length>1&&<button type="button" className="ghost-btn" aria-label={'移除键鼠输出 '+(i+1)} onClick={()=>emit([...mods,...main.filter((_,j)=>j!==i)])}>移除</button>}
    </div>)}
    {parts.length<4&&<OptionPicker label="添加同时输出" placeholder="添加同时输出" value={null} className={styles.addInput}
      groups={outputChoices(false, [...parts,...(hasWheel?wheelOutputs:[])])} onChange={next=>emit([...parts,next])}/>}
    {hasWheel&&showRepeat&&<>
      <Toggle label="按住连续滚动" value={action.repeat_ms!=null} onChange={value=>onChange({repeat_ms:value?100:null})}/>
      {action.repeat_ms!=null?<Range label="滚动间隔" value={action.repeat_ms} min={20} max={2000} step={10} unit=" ms" onChange={value=>onChange({repeat_ms:value})}/>:<small>每次触发滚动 1 格，松开后可再次触发。</small>}
    </>}
  </div>;
}
