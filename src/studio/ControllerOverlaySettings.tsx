import {useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import type {ControllerOverlayConfig} from '../types';
import {defaultControllerOverlay} from '../controller-overlay';
import {t} from '../i18n';
import styles from './SettingsPage.module.css';
import overlayStyles from './ControllerOverlaySettings.module.css';

type SourceInfo={url:string;size:[number,number]};
const destinations=[
  {label:'关闭',desktop:false,obs:false}, {label:'仅桌面',desktop:true,obs:false},
  {label:'仅 OBS',desktop:false,obs:true}, {label:'桌面和 OBS',desktop:true,obs:true},
] as const;

export function ControllerOverlaySettings({value,busy,onChange}:{
  value?:ControllerOverlayConfig;busy:boolean;onChange:(value:ControllerOverlayConfig)=>void;
}) {
  const settings:ControllerOverlayConfig=value?{...value,content:value.content??'controller'}:defaultControllerOverlay();
  const [displays,setDisplays]=useState<{name:string;width:number;height:number}[]>([]);
  const [source,setSource]=useState<SourceInfo>();
  const [sourceError,setSourceError]=useState('');
  const [copied,setCopied]=useState(false);
  useEffect(()=>{
    let disposed=false;
    void invoke<typeof displays>('controller_overlay_displays').then(value=>{if(!disposed)setDisplays(value);}).catch(()=>{});
    return()=>{disposed=true;};
  },[]);
  useEffect(()=>{
    let disposed=false;
    setSource(undefined);setSourceError('');setCopied(false);
    if(settings.obs_enabled){
      void invoke<SourceInfo>('obs_source_info',{copy:false}).then(info=>{if(!disposed)setSource(info);})
        .catch(error=>{if(!disposed)setSourceError(String(error));});
    }
    return ()=>{disposed=true;};
  },[settings.obs_enabled,settings.width,settings.content]);
  const copyAddress=async()=>{
    try { await invoke<SourceInfo>('obs_source_info',{copy:true});setCopied(true);setSourceError(''); }
    catch(error){setSourceError(String(error));}
  };
  const change=(patch:Partial<ControllerOverlayConfig>)=>onChange({...settings,...patch});
  const content=settings.content;
  return <section className={styles.section} aria-label="悬浮显示与录制">
    <div className={styles.heading}><div><h2>悬浮显示与录制</h2><p className="studio-muted">在桌面校对映射，或将按键和鼠标动作录给观众看。</p></div></div>
    <div className={overlayStyles.choiceRow}><span>显示位置</span>
      <div className={overlayStyles.modes} role="group" aria-label="显示位置">
        {destinations.map(({label,desktop,obs})=><button key={label} type="button" disabled={busy}
          aria-pressed={settings.enabled===desktop&&!!settings.obs_enabled===obs}
          onClick={()=>change({enabled:desktop,obs_enabled:obs})}>{label}</button>)}
      </div>
    </div>
    <div className={overlayStyles.choiceRow}><span>显示内容</span><div className={overlayStyles.modes} role="group" aria-label="悬浮显示内容">
      {([['key_mouse','键鼠输出'],['controller','三维手柄'],['both','手柄和键鼠']] as const).map(([id,label])=>
        <button key={id} type="button" aria-pressed={content===id} disabled={busy} onClick={()=>change({content:id})}>{label}</button>)}
    </div></div>
    {settings.obs_enabled&&<div className={overlayStyles.recording}>
      <label className="studio-field">OBS 浏览器源地址
        <input readOnly value={source?.url??''} onFocus={event=>event.currentTarget.select()}/>
      </label>
      <button type="button" className="secondary-btn" disabled={!source||busy} onClick={()=>void copyAddress()}>{copied?'已复制地址':'复制地址'}</button>
      <p className="studio-muted">在 OBS「来源」中添加「浏览器」，粘贴地址，并将它放在游戏画面上方。</p>
      {source&&<p className="studio-muted">建议源尺寸 <strong>{source.size[0]} × {source.size[1]} px</strong></p>}
      {sourceError&&<p className="studio-error" role="alert">{t(sourceError)}</p>}
    </div>}
    {(settings.enabled||settings.obs_enabled)&&<>
      <div className={overlayStyles.controls}>
        {settings.enabled&&<><label className="studio-field">显示器<select aria-label="悬浮窗显示器" disabled={busy} value={settings.monitor??''} onChange={event=>change({monitor:event.target.value||null})}>
          <option value="">主显示器</option>
          {settings.monitor&&!displays.some(display=>display.name===settings.monitor)&&<option value={settings.monitor}>上次使用的显示器（暂不可用）</option>}
          {displays.map((display,index)=><option key={display.name} value={display.name}>{index+1} · {display.width} × {display.height}</option>)}
        </select></label>
        <label className="studio-field">位置<select aria-label="悬浮窗位置" disabled={busy} value={settings.position} onChange={event=>change({position:event.target.value as ControllerOverlayConfig['position']})}>
          <option value="top_left">左上角</option><option value="top_right">右上角</option>
          <option value="bottom_left">左下角</option><option value="bottom_right">右下角</option>
        </select></label></>}
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
      {settings.enabled&&<p className="studio-muted">桌面悬浮窗置顶并允许鼠标穿透，适合在无边框游戏中校对。</p>}
    </>}
  </section>;
}
