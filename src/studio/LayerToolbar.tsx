import {useState} from 'react';
import type {Profile,Status} from '../types';
import {Dialog} from './Controls';
import {LayerPanel} from './LayerPanel';
import {StudioIcon} from './StudioIcon';
import styles from './LayerToolbar.module.css';

export function LayerToolbar({profile,profileId,selected,onSelect,onUpdate,blocked,status}:{profile:Profile;profileId:string;selected:string;onSelect:(id:string)=>void;onUpdate:(change:(profile:Profile)=>void)=>void;blocked:boolean;status?:Status}) {
  const [managing,setManaging]=useState(false);
  const active=status?.profile_id===profileId&&status.emitting;
  return <div className={styles.tools}>
    <label className={styles.selector} title="选择要编辑的层；按住启用输入时才会在游戏中生效。">
      <span>编辑层</span>
      <select aria-label="正在编辑的配置层" data-layer-selector value={selected} disabled={blocked} onChange={event=>onSelect(event.target.value)}>
        <option value="">基础层</option>
        {(profile.layers??[]).map(layer=><option key={layer.id} value={layer.id} translate="no">{layer.name}</option>)}
      </select>
    </label>
    <button className={styles.manage} type="button" aria-label="管理配置层" title="管理配置层" disabled={blocked} onClick={()=>setManaging(true)}><StudioIcon name="settings" size={16}/></button>
    {active&&<span className={styles.active} role="status">运行：{status.active_layers?.length?<span translate="no">{status.active_layers.join(' + ')}</span>:'基础层'}</span>}
    {managing&&<Dialog title="管理配置层" className={styles.dialog} onClose={()=>setManaging(false)}
      description="配置层与当前配置一起保存。选择编辑层不会立即启用它，游戏中仍由按住输入控制。"
      actions={<button type="button" className="primary-btn" disabled={(profile.layers??[]).some(layer=>!layer.name.trim())} onClick={()=>setManaging(false)}>完成</button>}>
      <LayerPanel profile={profile} selected={selected} onSelect={onSelect} onUpdate={onUpdate} blocked={blocked} status={status?.profile_id===profileId?status:undefined}/>
    </Dialog>}
  </div>;
}
