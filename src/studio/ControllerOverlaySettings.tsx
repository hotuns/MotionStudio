import {useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import type {ControllerOverlayConfig} from '../types';
import {defaultControllerOverlay} from '../controller-overlay';
import styles from './SettingsPage.module.css';
import overlayStyles from './ControllerOverlaySettings.module.css';

export function ControllerOverlaySettings({value,busy,onChange}:{
  value?:ControllerOverlayConfig;busy:boolean;onChange:(value:ControllerOverlayConfig)=>void;
}) {
  const settings={...defaultControllerOverlay(),...value};
  const [displays,setDisplays]=useState<{name:string;width:number;height:number}[]>([]);
  useEffect(()=>{
    let disposed=false;
    void invoke<typeof displays>('controller_overlay_displays').then(value=>{if(!disposed)setDisplays(value);}).catch(()=>{});
    return()=>{disposed=true;};
  },[]);
  const change=(patch:Partial<ControllerOverlayConfig>)=>onChange({...settings,...patch});
  return <section className={styles.section} aria-label="三维悬浮手柄">
    <div className={styles.heading}><div><h2>三维悬浮手柄</h2><p className="studio-muted">在屏幕上显示实时手柄动作，供录屏或直播使用。</p></div></div>
    <label className={styles.check}><input type="checkbox" aria-label="显示悬浮手柄" checked={settings.enabled} disabled={busy}
      onChange={event=>change({enabled:event.target.checked})}/>显示悬浮手柄</label>
    {settings.enabled&&<>
      <div className={overlayStyles.controls}>
        <label className="studio-field">显示器<select aria-label="悬浮手柄显示器" disabled={busy} value={settings.monitor??''} onChange={event=>change({monitor:event.target.value||null})}>
          <option value="">主显示器</option>
          {settings.monitor&&!displays.some(display=>display.name===settings.monitor)&&<option value={settings.monitor}>上次使用的显示器（暂不可用）</option>}
          {displays.map((display,index)=><option key={display.name} value={display.name}>{index+1} · {display.width} × {display.height}</option>)}
        </select></label>
        <label className="studio-field">位置<select aria-label="悬浮手柄位置" disabled={busy} value={settings.position} onChange={event=>change({position:event.target.value as ControllerOverlayConfig['position']})}>
          <option value="top_left">左上角</option><option value="top_right">右上角</option>
          <option value="bottom_left">左下角</option><option value="bottom_right">右下角</option>
        </select></label>
        <label className="studio-field">大小<select aria-label="悬浮手柄大小" disabled={busy} value={settings.width} onChange={event=>change({width:Number(event.target.value)})}>
          {Array.from(new Set([160,240,320,400,480,640,settings.width])).sort((a,b)=>a-b).map(width=><option key={width} value={width}>{width}</option>)}
        </select></label>
        <label className="studio-field">不透明度<select aria-label="悬浮手柄不透明度" disabled={busy} value={settings.opacity} onChange={event=>change({opacity:Number(event.target.value)})}>
          {Array.from(new Set([20,40,60,80,100,settings.opacity])).sort((a,b)=>b-a).map(opacity=><option key={opacity} value={opacity}>{opacity}%</option>)}
        </select></label>
      </div>
      <label className={styles.check}><input type="checkbox" aria-label="悬浮手柄跟随姿态" checked={settings.follow_motion} disabled={busy}
        onChange={event=>change({follow_motion:event.target.checked})}/>跟随手柄姿态</label>
      <p className="studio-muted">透明背景、始终置顶、鼠标穿透。主窗口最小化后继续显示；开始映射或观察输入后显示实时动作。</p>
      <p className="studio-muted">录屏请选择显示器捕获。仅捕获游戏窗口可能不包含悬浮手柄；游戏建议使用无边框窗口模式。</p>
    </>}
  </section>;
}
