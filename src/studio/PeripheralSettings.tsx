import type {Device,Profile,Status,FeedbackConfig,TouchpadConfig} from '../types';
import {Range,Toggle} from './Controls';
import styles from './MotionControls.module.css';
export const feedbackConfig=(profile:Profile):FeedbackConfig=>({rumble_enabled:true,rumble_gain:1,lightbar_enabled:true,...profile.feedback});
export const touchpadConfig=(profile:Profile):TouchpadConfig=>({enabled:false,pointer_speed:1,scroll_speed:1,natural_scroll:true,...profile.touchpad});
type Props={profile:Profile;device?:Device|null;status?:Status;update:(change:(profile:Profile)=>void)=>void};

export function FeedbackSettings({profile,device,status,update}:Props){
 const config=feedbackConfig(profile),runtime=status?.feedback;
 const set=<K extends keyof FeedbackConfig>(key:K,value:FeedbackConfig[K])=>update(p=>{p.feedback={...feedbackConfig(p),[key]:value};});
 return <div className={styles.group}>
  <p className={styles.hint}>把游戏发给虚拟手柄的震动／灯光交回当前所选实体手柄。Xbox 主要提供震动，DualShock 4 还可以提供灯光；实际能力取决于实体接口。</p>
  <Toggle label="回传游戏震动" value={config.rumble_enabled} onChange={v=>set('rumble_enabled',v)}/>
  {config.rumble_enabled&&<Range label="震动强度" value={config.rumble_gain} min={0} max={2} step={0.05} unit="×" onChange={v=>set('rumble_gain',v)}/>}
  <Toggle label="回传游戏灯光" value={config.lightbar_enabled} onChange={v=>set('lightbar_enabled',v)}/>
  {device&&<p className={styles.hint}>所选接口：{device.rumble?'支持震动':'未报告震动支持'} · {device.lightbar?'支持灯光':'未报告灯光支持'}</p>}
  {profile.virtual_gamepad==='none'?<p className={styles.hint}>选择虚拟 Xbox／DualShock 4 输出后，游戏反馈回传才会运行。</p>:runtime?.active?<p role="status">{runtime.awaiting_new_command?'等待新的游戏反馈':'正在回传游戏反馈'} · <span translate="no">{Math.round(runtime.rumble[0]*100)}% / {Math.round(runtime.rumble[1]*100)}%</span></p>:null}
  <p className={styles.hint}>录入、停止、断线和切换时释放震动，恢复后等待新的游戏命令。停止灯光回传时取消待发送颜色，设备保留最后颜色。</p>
 </div>;
}

export function TouchpadSettings({profile,device,status,update}:Props){
 const config=touchpadConfig(profile);
 const set=<K extends keyof TouchpadConfig>(key:K,value:TouchpadConfig[K])=>update(p=>{p.touchpad={...touchpadConfig(p),[key]:value};});
 const count=Math.min(2,Math.max(1,device?.touchpads??0));
 return <section className={styles.group}>
  <div className={styles.groupHeading}><h3>原生触摸板鼠标</h3></div>
  <Toggle label="启用触摸板鼠标" value={config.enabled} onChange={v=>set('enabled',v)}/>
  <p className={styles.hint}>单指移动光标，双指滚动。单指轻点为左键，双指轻点为右键，按住触摸板可拖动。双触摸板使用左板滚动、右板移动，双板同时按下为中键。</p>
  {config.enabled&&<><div className={styles.grid}>
   <Range label="触摸板指针速度" value={config.pointer_speed} min={0.1} max={5} step={0.05} unit="×" onChange={v=>set('pointer_speed',v)}/>
   <Range label="触摸板滚动速度" value={config.scroll_speed} min={0.1} max={5} step={0.05} unit="×" onChange={v=>set('scroll_speed',v)}/>
  </div><Toggle label="自然滚动" value={config.natural_scroll} onChange={v=>set('natural_scroll',v)}/>
  <p className={styles.hint}>保存后应用。启用或切换时手指已在触摸板上，会等待全部抬起再开始，避免误点击。动作列表中的触摸板按键映射仍会执行。</p></>}
  <details><summary>实时触摸预览</summary>
   <p className={styles.hint}>{device?.touchpads?'当前接口已报告触摸板，预览保留原始触点。':'当前接口未报告触摸板。可以先保存设置，连接支持触摸坐标的接口后使用。'}</p>
   <div className={styles.grid}>{Array.from({length:count},(_,pad)=><svg key={pad} viewBox="0 0 320 180" role="img" aria-label={pad===0?'触摸板 1 预览':'触摸板 2 预览'} style={{width:'100%',maxWidth:480,background:'var(--bg-input)',border:'1px solid var(--border-1)',borderRadius:12}}>
    {[0,1].map(finger=>{const contact=status?.touches?.[pad]?.[finger];return contact?.active?<g key={finger}><circle cx={16+contact.x*288} cy={16+contact.y*148} r={10} fill="var(--accent)"/><text x={16+contact.x*288} y={20+contact.y*148} textAnchor="middle" fill="white" fontSize="12">{finger+1}</text></g>:null;})}
   </svg>)}</div>
  </details>
 </section>;
}
