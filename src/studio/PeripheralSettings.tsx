import type {Device,Profile,Status,FeedbackConfig,TouchpadConfig} from '../types';
import {Range,Toggle} from './Controls';
import {settingStyles} from './SettingLayout';
import styles from './MotionControls.module.css';
export const feedbackConfig=(profile:Profile)=>({rumble_enabled:true,rumble_gain:1,lightbar_enabled:true,...profile.feedback,audio_haptics_enabled:profile.feedback?.audio_haptics_enabled??false,audio_haptics_gain:profile.feedback?.audio_haptics_gain??1});
export const touchpadConfig=(profile:Profile):TouchpadConfig=>({enabled:false,pointer_speed:1,scroll_speed:1,natural_scroll:true,...profile.touchpad});
type Props={profile:Profile;device?:Device|null;status?:Status;layerMode?:boolean;update:(change:(profile:Profile)=>void)=>void};

export function FeedbackSettings({profile,device,status,layerMode,update}:Props){
 const config=feedbackConfig(profile),runtime=status?.feedback;
 const set=<K extends keyof FeedbackConfig>(key:K,value:FeedbackConfig[K])=>update(p=>{p.feedback={...feedbackConfig(p),[key]:value};});
 return <div className={styles.group}>
  <p className={styles.hint}>把游戏发给虚拟手柄的震动／灯光交回当前所选实体手柄。Xbox 主要提供震动，DualShock 4 还可以提供灯光；实际能力取决于实体接口。</p>
  <div className={settingStyles.fieldGrid}>
   <div>
    <Toggle label="回传普通游戏震动" settingPath="feedback.rumble_enabled" value={config.rumble_enabled} onChange={v=>set('rumble_enabled',v)}/>
    {config.rumble_enabled&&<Range label="普通震动强度" settingPath="feedback.rumble_gain" value={config.rumble_gain} min={0} max={2} step={0.05} unit="×" onChange={v=>set('rumble_gain',v)}/>}
   </div>
   <Toggle label="回传游戏灯光" settingPath="feedback.lightbar_enabled" value={config.lightbar_enabled} onChange={v=>set('lightbar_enabled',v)}/>
  </div>
  {profile.virtual_gamepad==='dualsense'&&<>
   <Toggle label="APEX 6 Pro 音圈触觉（实验性）" settingPath="feedback.audio_haptics_enabled" disabled={layerMode} value={config.audio_haptics_enabled} onChange={v=>set('audio_haptics_enabled',v)}/>
   {layerMode&&<p className={styles.hint}>音圈输出开关在基础层设置，配置层可以调整触觉强度。</p>}
   <p className={styles.hint}>转发游戏原生 DualSense 触觉到 APEX 6 Pro 握把。支持有线和接收器，启动时检查型号与音圈能力。自适应扳机效果尚未转发。</p>
   <p className={styles.hint}>需要先安装 usbip-win2 虚拟 USB 主控制器。游戏声音请选择原有扬声器或耳机；虚拟手柄扬声器暂不转发。</p>
   {config.audio_haptics_enabled&&<Range label="音圈触觉强度" settingPath="feedback.audio_haptics_gain" value={config.audio_haptics_gain} min={0} max={2} step={0.05} unit="×" onChange={v=>set('audio_haptics_gain',v)}/>}
   {runtime?.haptics&&<div role="status">
    <p>{runtime.haptics.error?'音圈输出异常':runtime.haptics.state==='streaming'?'音圈输出已建立':runtime.haptics.state==='starting'?'正在检查音圈接口':runtime.haptics.state==='paused'?'音圈输出已暂停':'音圈输出已停止'}</p>
    {runtime.haptics.error&&<p>{runtime.haptics.error}</p>}
    <p>接收音频帧：<span translate="no">{runtime.haptics.received_frames}</span> · 发送波形包：<span translate="no">{runtime.haptics.sent_packets}</span> · 缓冲：<span translate="no">{runtime.haptics.queued_ms} ms</span></p>
   </div>}
  </>}
  {device&&<p className={styles.hint}>所选接口：{device.rumble?'支持震动':'未报告震动支持'} · {device.lightbar?'支持灯光':'未报告灯光支持'}</p>}
  {profile.virtual_gamepad==='none'?<p className={styles.hint}>选择虚拟手柄输出后，游戏反馈回传才会运行。</p>:runtime?.active?<p role="status">{runtime.awaiting_new_command?'等待新的游戏反馈':'正在回传游戏反馈'} · <span translate="no">{Math.round(runtime.rumble[0]*100)}% / {Math.round(runtime.rumble[1]*100)}%</span></p>:null}
  <p className={styles.hint}>录入、停止、断线和切换时释放震动，恢复后等待新的游戏命令。停止灯光回传时取消待发送颜色，设备保留最后颜色。</p>
 </div>;
}

export function TouchpadSettings({profile,device,status,update}:Props){
 const config=touchpadConfig(profile);
 const set=<K extends keyof TouchpadConfig>(key:K,value:TouchpadConfig[K])=>update(p=>{p.touchpad={...touchpadConfig(p),[key]:value};});
 const count=Math.min(2,Math.max(1,device?.touchpads??0));
 return <section className={styles.group} data-setting-section="touch-general">
  <Toggle label="启用触摸板鼠标" value={config.enabled} onChange={v=>set('enabled',v)}/>
  <p className={styles.hint}>单指移动光标，双指滚动。单指轻点为左键，双指轻点为右键，按住触摸板可拖动。双触摸板使用左板滚动、右板移动，双板同时按下为中键。</p>
  {config.enabled&&<><div className={settingStyles.fieldGrid}>
   <Range label="触摸板指针速度" value={config.pointer_speed} min={0.1} max={5} step={0.05} unit="×" onChange={v=>set('pointer_speed',v)}/>
   <Range label="触摸板滚动速度" value={config.scroll_speed} min={0.1} max={5} step={0.05} unit="×" onChange={v=>set('scroll_speed',v)}/>
  </div><Toggle label="自然滚动" value={config.natural_scroll} onChange={v=>set('natural_scroll',v)}/>
  <p className={styles.hint}>保存后应用。启用或切换时手指已在触摸板上，会等待全部抬起再开始，避免误点击。动作列表中的触摸板按键映射仍会执行。</p></>}
  <details><summary>触摸板方向手势</summary>
   <p className={styles.hint}>在动作映射中添加动作，输入选择「触摸板上滑／下滑／左滑／右滑」，即可分别绑定按键。无需开启触摸板鼠标。</p>
   <p className={styles.hint}>沿主要方向滑动超过触摸板约 18% 的距离后，该方向保持触发，手指停住或移回也会继续，抬手时释放；再次落指可重新滑动。双指或双触摸板的同方向手势会合并，最后一根触发该方向的手指抬起才释放。建议使用默认的按住触发方式。</p>
  </details>
  <details><summary>实时触摸预览</summary>
   <p className={styles.hint}>{device?.touchpads?'当前接口已报告触摸板，预览保留原始触点。':'当前接口未报告触摸板。可以先保存设置，连接支持触摸坐标的接口后使用。'}</p>
   <div className={styles.grid}>{Array.from({length:count},(_,pad)=><svg key={pad} viewBox="0 0 320 180" role="img" aria-label={pad===0?'触摸板 1 预览':'触摸板 2 预览'} style={{width:'100%',maxWidth:480,background:'var(--bg-input)',border:'1px solid var(--panel-border)',borderRadius:'var(--section-radius)'}}>
    {[0,1].map(finger=>{const contact=status?.touches?.[pad]?.[finger];return contact?.active?<g key={finger}><circle cx={16+contact.x*288} cy={16+contact.y*148} r={10} fill="var(--accent)"/><text x={16+contact.x*288} y={20+contact.y*148} textAnchor="middle" fill="var(--btn-primary-text)" fontSize="12">{finger+1}</text></g>:null;})}
   </svg>)}</div>
  </details>
 </section>;
}
