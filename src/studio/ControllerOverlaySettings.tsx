import {useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import type {ControllerOverlayConfig} from '../types';
import {defaultControllerOverlay} from '../controller-overlay';
import styles from './SettingsPage.module.css';
import overlayStyles from './ControllerOverlaySettings.module.css';

export function ControllerOverlaySettings({value,busy,onChange}:{
  value?:ControllerOverlayConfig;busy:boolean;onChange:(value:ControllerOverlayConfig)=>void;
}) {
  const settings:ControllerOverlayConfig=value?{...value,content:value.content??'controller'}:defaultControllerOverlay();
  const [displays,setDisplays]=useState<{name:string;width:number;height:number}[]>([]);
  useEffect(()=>{
    let disposed=false;
    void invoke<typeof displays>('controller_overlay_displays').then(value=>{if(!disposed)setDisplays(value);}).catch(()=>{});
    return()=>{disposed=true;};
  },[]);
  const change=(patch:Partial<ControllerOverlayConfig>)=>onChange({...settings,...patch});
  const content=settings.content;
  return <section className={styles.section} aria-label="桌面悬浮显示">
    <div className={styles.heading}><div><h2>桌面悬浮显示</h2><p className="studio-muted">在游戏画面上查看映射发出的键鼠信号，也可以同时对照手柄动作。</p></div>
      <label className={styles.check}><input type="checkbox" aria-label="显示桌面悬浮窗" checked={settings.enabled} disabled={busy}
        onChange={event=>change({enabled:event.target.checked})}/>显示桌面悬浮窗</label>
    </div>
    <div className={overlayStyles.modes} role="group" aria-label="悬浮显示内容">
      {([['key_mouse','键鼠输出'],['controller','三维手柄'],['both','手柄和键鼠']] as const).map(([id,label])=>
        <button key={id} type="button" aria-pressed={content===id} disabled={busy} onClick={()=>change({content:id})}>{label}</button>)}
    </div>
    {settings.enabled&&<>
      <div className={overlayStyles.controls}>
        <label className="studio-field">显示器<select aria-label="悬浮窗显示器" disabled={busy} value={settings.monitor??''} onChange={event=>change({monitor:event.target.value||null})}>
          <option value="">主显示器</option>
          {settings.monitor&&!displays.some(display=>display.name===settings.monitor)&&<option value={settings.monitor}>上次使用的显示器（暂不可用）</option>}
          {displays.map((display,index)=><option key={display.name} value={display.name}>{index+1} · {display.width} × {display.height}</option>)}
        </select></label>
        <label className="studio-field">位置<select aria-label="悬浮窗位置" disabled={busy} value={settings.position} onChange={event=>change({position:event.target.value as ControllerOverlayConfig['position']})}>
          <option value="top_left">左上角</option><option value="top_right">右上角</option>
          <option value="bottom_left">左下角</option><option value="bottom_right">右下角</option>
        </select></label>
        <label className="studio-field">大小<select aria-label="悬浮窗大小" disabled={busy} value={settings.width} onChange={event=>change({width:Number(event.target.value)})}>
          {Array.from(new Set([160,240,320,400,480,640,settings.width])).sort((a,b)=>a-b).map(width=><option key={width} value={width}>{width}</option>)}
        </select></label>
        <label className="studio-field">不透明度<select aria-label="悬浮窗不透明度" disabled={busy} value={settings.opacity} onChange={event=>change({opacity:Number(event.target.value)})}>
          {Array.from(new Set([20,40,60,80,100,settings.opacity])).sort((a,b)=>b-a).map(opacity=><option key={opacity} value={opacity}>{opacity}%</option>)}
        </select></label>
      </div>
      {content!=='key_mouse'&&<label className={styles.check}><input type="checkbox" aria-label="悬浮手柄跟随姿态" checked={settings.follow_motion} disabled={busy}
        onChange={event=>change({follow_motion:event.target.checked})}/>跟随手柄姿态</label>}
      {content!=='controller'&&<p className="studio-muted">启动映射后，亮色表示按住，淡色表示刚松开。鼠标轨迹显示最近 300 毫秒的相对位移。</p>}
      <p className="studio-muted">悬浮窗置顶且允许鼠标穿透。游戏可使用无边框窗口，录屏时请选择显示器捕获。</p>
    </>}
  </section>;
}
