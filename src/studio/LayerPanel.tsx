import type {Profile,Status} from '../types';
import {inputLabel} from '../types';
import {newLayer} from '../layers';
import {InputChordFields} from './BindingFields';
import {Range} from './Controls';
import {OptionPicker} from './OptionPicker';
import {padChoices} from './binding-choices';
import {useState} from 'react';
import styles from './CursorSwitchPanel.module.css';
export function LayerPanel({profile,selected,onSelect,onUpdate,blocked,status}:{profile:Profile;selected:string;onSelect:(id:string)=>void;onUpdate:(change:(profile:Profile)=>void)=>void;blocked:boolean;status?:Status}){
 const layers=profile.layers??[],layer=layers.find(layer=>layer.id===selected);
 const [disabledInput,setDisabledInput]=useState('south');
 const split=(status?.device?.touchpads??0)>=2;
 const patch=(change:(layer:NonNullable<Profile['layers']>[number])=>void)=>onUpdate(profile=>{const layer=profile.layers?.find(layer=>layer.id===selected);if(layer)change(layer);});
 return <fieldset className={styles.panel} disabled={blocked}>
   <div className={styles.heading}><div><h4>配置层</h4><p>按住启用，松开恢复。未覆盖的设置与动作继承基础层；多个层按列表顺序叠加，后面的覆盖前面的。</p></div>
   <button type="button" className="secondary-btn" disabled={layers.length>=8} onClick={()=>{const layer=newLayer();onUpdate(profile=>{(profile.layers??=[]).push(layer);});onSelect(layer.id);}}>添加配置层</button></div>
   <div className={styles.controls}><label>正在编辑<select aria-label="管理的配置层" autoFocus value={layer?.id??''} onChange={event=>onSelect(event.target.value)}>
     <option value="">基础层</option>{layers.map(layer=><option key={layer.id} value={layer.id} translate="no">{layer.name}</option>)}
   </select></label>
   {layer&&<><label>配置层名称<input aria-label="配置层名称" value={layer.name} maxLength={60} onChange={event=>patch(layer=>{layer.name=event.target.value;})}/></label>
   <button type="button" className="secondary-btn" onClick={()=>{onUpdate(profile=>{profile.layers=profile.layers?.filter(item=>item.id!==layer.id);});onSelect('');}}>删除配置层</button></>}</div>
   {layer&&<>
     <p className={styles.hint}>这里只编辑此层的覆盖项，不会立即启用该层。体感、摇杆页面的修改同样写入当前层；输出设备由基础层决定。</p>
     <InputChordFields value={layer.input} split={split} onChange={input=>patch(layer=>{layer.input=input;})}/>
     {layer.input.split('+').some(input=>/trigger|stick_(up|down|left|right)$/.test(input))&&<>
       <Range label="层触发阈值" value={layer.activation_threshold??0.5} min={0.05} max={0.95} step={0.05} onChange={value=>patch(layer=>{layer.activation_threshold=value;layer.activation_hysteresis=Math.min(layer.activation_hysteresis??0.1,value/2);})}/>
       <Range label="层释放回差" value={layer.activation_hysteresis??0.1} min={0} max={Math.min(0.3,(layer.activation_threshold??0.5)-0.01)} step={0.01} onChange={value=>patch(layer=>{layer.activation_hysteresis=value;})}/>
     </>}
     <div className={styles.controls}><button type="button" className="secondary-btn" disabled={layers[0]?.id===layer.id} onClick={()=>onUpdate(profile=>{const all=profile.layers!;const i=all.findIndex(item=>item.id===layer.id);[all[i-1],all[i]]=[all[i],all[i-1]];})}>降低层优先级</button>
       <button type="button" className="secondary-btn" disabled={layers.at(-1)?.id===layer.id} onClick={()=>onUpdate(profile=>{const all=profile.layers!;const i=all.findIndex(item=>item.id===layer.id);[all[i+1],all[i]]=[all[i],all[i+1]];})}>提高层优先级</button>
       {Object.keys(layer.settings).map(key=><button key={key} type="button" className="ghost-btn" onClick={()=>patch(layer=>{delete layer.settings[key];})}>恢复继承：{({gyro:'体感',stick_mouse:'摇杆鼠标',stick_deadzone:'摇杆死区',stick_zones:'摇杆区域',stick_rebound_ms:'防回弹'} as Record<string,string>)[key]??key}</button>)}
     </div>
     <p className={styles.hint}>此层动作会覆盖相同手柄输入在基础层的所有触发方式，其余动作继续继承。删除此层动作即恢复继承。</p>
     <div className={styles.controls}><div className={styles.field}>禁用继承的手柄输入<OptionPicker label="禁用继承输入" value={disabledInput} groups={padChoices(undefined, [], split)} onChange={setDisabledInput}/></div>
       <button type="button" className="secondary-btn" disabled={layer.disabled_inputs.includes(disabledInput)} onClick={()=>patch(layer=>{layer.disabled_inputs.push(disabledInput);})}>在此层禁用</button>
       {layer.disabled_inputs.map(input=><button type="button" className="ghost-btn" key={input} onClick={()=>patch(layer=>{layer.disabled_inputs=layer.disabled_inputs.filter(value=>value!==input);})}>恢复输入：{inputLabel(input)}</button>)}
     </div>
   </>}
   <p className={styles.hint}>当前运行层：{status?.active_layers?.length?<span translate="no">{status.active_layers.join(' + ')}</span>:'基础层'}</p>
 </fieldset>;
}
