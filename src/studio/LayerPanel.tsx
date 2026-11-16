import type {Profile,Status} from '../types';
import {inputLabel,outputLabel} from '../types';
import {newLayer,layerConflicts} from '../layers';
import {InputChordFields,triggerNames} from './BindingFields';
import {t} from '../i18n';
import {Range} from './Controls';
import {PadInputPicker} from './PadInputPicker';
import {padChoices} from './binding-choices';
import {useState} from 'react';
import styles from './CursorSwitchPanel.module.css';
export function LayerPanel({profile,selected,onSelect,onUpdate,blocked,status}:{profile:Profile;selected:string;onSelect:(id:string)=>void;onUpdate:(change:(profile:Profile)=>void)=>void;blocked:boolean;status?:Status}){
 const layers=profile.layers??[],layer=layers.find(layer=>layer.id===selected);
 const [disabledInput,setDisabledInput]=useState('south');
 const split=(status?.device?.touchpads??0)>=2;
 const conflicts=layerConflicts(profile,selected);
 const optionalGroups=[{options:[{value:'',label:'不设置'}]},...padChoices(undefined,[],split)];
 const patch=(change:(layer:NonNullable<Profile['layers']>[number])=>void)=>onUpdate(profile=>{const layer=profile.layers?.find(layer=>layer.id===selected);if(layer)change(layer);});
 return <fieldset className={styles.panel} disabled={blocked}>
   <div className={styles.heading}><div><h4>配置层</h4><p>可按住启用，也可按一下切换开关。未覆盖的设置与动作继承基础层；多个层按列表顺序叠加，后面的覆盖前面的。</p></div>
   <button type="button" className="secondary-btn" disabled={layers.length>=8} onClick={()=>{const layer=newLayer();onUpdate(profile=>{(profile.layers??=[]).push(layer);});onSelect(layer.id);}}>添加配置层</button></div>
   <div className={styles.controls}><label>正在编辑<select aria-label="管理的配置层" autoFocus value={layer?.id??''} onChange={event=>onSelect(event.target.value)}>
     <option value="">基础层</option>{layers.map(layer=><option key={layer.id} value={layer.id} translate="no">{layer.name}</option>)}
   </select></label>
   {layer&&<><label>配置层名称<input aria-label="配置层名称" value={layer.name} maxLength={60} onChange={event=>patch(layer=>{layer.name=event.target.value;})}/></label>
   <button type="button" className="secondary-btn" onClick={()=>{onUpdate(profile=>{profile.layers=profile.layers?.filter(item=>item.id!==layer.id);});onSelect('');}}>删除配置层</button></>}</div>
   <div className={styles.field}>返回基础层<PadInputPicker label="返回基础层" value={profile.layer_reset_input??''} groups={optionalGroups} onChange={input=>onUpdate(profile=>{profile.layer_reset_input=input||null;})}/></div>
   <p className={styles.hint}>关闭所有操作层；仍按住的启用输入需先松开再按。此按键原有的动作映射仍会执行。</p>
   {layer&&<>
     <p className={styles.hint}>这里只编辑此层的覆盖项，不会立即启用该层。体感、摇杆页面的修改同样写入当前层；输出设备由基础层决定。</p>
     <InputChordFields key={layer.id} value={layer.input} split={split} onChange={input=>patch(layer=>{layer.input=input;})}/>
     <div className={styles.controls}><label>层启用方式<select aria-label="层启用方式" value={layer.activation_mode??'hold'} onChange={event=>patch(layer=>{layer.activation_mode=event.target.value as 'hold'|'toggle';})}>
       <option value="hold">按住启用，松开恢复</option><option value="toggle">按一下开启，再按一下关闭</option>
     </select></label></div>
     <div className={styles.field}>关闭此层<PadInputPicker key={'exit:'+layer.id} label="关闭此层" value={layer.exit_input??''} groups={optionalGroups} onChange={input=>patch(layer=>{layer.exit_input=input||null;})}/></div>
     {layer.activation_mode==='toggle'&&<p className={styles.hint}>停止映射、断连或开始录入会清除层开关。重连时已按住的输入需先松开再按。</p>}
     {layer.input.split('+').some(input=>/trigger|stick_(up|down|left|right)$/.test(input))&&<>
       <Range label="层触发阈值" value={layer.activation_threshold??0.5} min={0.05} max={0.95} step={0.05} onChange={value=>patch(layer=>{layer.activation_threshold=value;layer.activation_hysteresis=Math.min(layer.activation_hysteresis??0.1,value/2);})}/>
       <Range label="层释放回差" value={layer.activation_hysteresis??0.1} min={0} max={Math.min(0.3,(layer.activation_threshold??0.5)-0.01)} step={0.01} onChange={value=>patch(layer=>{layer.activation_hysteresis=value;})}/>
     </>}
     <div className={styles.controls}><button type="button" className="secondary-btn" disabled={layers[0]?.id===layer.id} onClick={()=>onUpdate(profile=>{const all=profile.layers!;const i=all.findIndex(item=>item.id===layer.id);[all[i-1],all[i]]=[all[i],all[i-1]];})}>降低层优先级</button>
       <button type="button" className="secondary-btn" disabled={layers.at(-1)?.id===layer.id} onClick={()=>onUpdate(profile=>{const all=profile.layers!;const i=all.findIndex(item=>item.id===layer.id);[all[i+1],all[i]]=[all[i],all[i+1]];})}>提高层优先级</button>
       {Object.keys(layer.settings).map(key=><button key={key} type="button" className="ghost-btn" onClick={()=>patch(layer=>{delete layer.settings[key];})}>恢复继承：{({gyro:'体感',gamepad_bindings:'虚拟手柄映射',stick_mouse:'摇杆鼠标',stick_deadzone:'摇杆死区',stick_zones:'摇杆区域',stick_rebound_ms:'防回弹'} as Record<string,string>)[key]??key}</button>)}
     </div>
     <p className={styles.hint}>此层动作会覆盖相同手柄输入在基础层的所有触发方式，其余动作继续继承。删除此层动作即恢复继承。</p>
     <div className={styles.controls}><div className={styles.field}>禁用继承的手柄输入<PadInputPicker key={layer.id} label="禁用继承输入" value={disabledInput} groups={padChoices(undefined, [], split)} onChange={setDisabledInput}/></div>
       <button type="button" className="secondary-btn" disabled={layer.disabled_inputs.includes(disabledInput)} onClick={()=>patch(layer=>{layer.disabled_inputs.push(disabledInput);})}>在此层禁用</button>
       {layer.disabled_inputs.map(input=><button type="button" className="ghost-btn" key={input} onClick={()=>patch(layer=>{layer.disabled_inputs=layer.disabled_inputs.filter(value=>value!==input);})}>恢复输入：{inputLabel(input)}</button>)}
     </div>
     <details className={styles.card} data-layer-conflicts><summary>层覆盖预览 · {conflicts.length}</summary>
       <p className={styles.hint}>下列覆盖在对应层同时启用时生效。同一输入的短按、长按、双击会一起被覆盖。</p>
       {conflicts.length?<ul className={styles.ruleList}>{conflicts.map((item,index)=><li key={index} className={styles.ruleTarget}>
         <strong>{t(item.kind==='overridden'?'会被高优先级层覆盖':item.kind==='disabled'?'禁用继承':'替换继承')} · <span translate="no">{item.source}</span></strong>
         <span><span translate="no">{item.action.name}</span> · {inputLabel(item.action.input)} · {t(triggerNames[item.action.trigger??'down'])} → {outputLabel(item.action.output)}</span>
       </li>)}</ul>:<p>此层没有相同输入的动作覆盖。</p>}
       {Object.hasOwn(layer.settings,'gamepad_bindings')&&<p>此层会整体替换虚拟手柄映射列表。</p>}
     </details>
   </>}
   <p className={styles.hint}>当前运行层：{status?.active_layers?.length?<span translate="no">{status.active_layers.join(' + ')}</span>:'基础层'}</p>
 </fieldset>;
}
