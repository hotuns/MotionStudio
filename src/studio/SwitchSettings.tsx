import { targetFrom, targetValue, type Entry, type Settings, type Shortcut, type Status, type Target } from '../types';
import {useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {defaultShortcut, shortcutFromEvent, shortcutKeys, shortcutLabel, shortcutModifier} from '../shortcuts';
import styles from './CursorSwitchPanel.module.css';
import {PadShortcutSettings} from './PadShortcutSettings';
import {OptionPicker} from './OptionPicker';
import {keyboardChoices} from './binding-choices';

const keyChoices = keyboardChoices(shortcutKeys);

export function SwitchSettings({ entries, settings, busy, onChange, status, onPreparePad, onPadCapture }: {
  entries: Entry[];
  settings: Settings;
  busy: boolean;
  onChange: (patch: Partial<Settings>) => void;
  status?:Status;
  onPreparePad:()=>Promise<Status>;
  onPadCapture:(active:boolean)=>Promise<void>;
}) {
  const [padCapturing,setPadCapturing]=useState(false);
  const [recording,setRecording]=useState(false);
  const [starting,setStarting]=useState(false);
  const [error,setError]=useState<string|null>(null);
  const [overlayError,setOverlayError]=useState<string|null>(null);
  const [delay,setDelay]=useState(String(settings.cursor_debounce_ms??250));
  const [delayError,setDelayError]=useState(false);
  useEffect(()=>{setDelay(String(settings.cursor_debounce_ms??250));setDelayError(false);},[settings.cursor_debounce_ms]);
  const saveDelay=()=>{
    const value=Number(delay);
    if(!delay.trim()||!Number.isInteger(value)||value<0||value>5000){setDelayError(true);return;}
    setDelayError(false);
    if(!busy&&value!==(settings.cursor_debounce_ms??250))onChange({cursor_debounce_ms:value});
  };
  const latestChange=useRef(onChange);latestChange.current=onChange;
  const mounted=useRef(true),keyboardLease=useRef(false);
  const latestCapture=useRef(onPadCapture);latestCapture.current=onPadCapture;
  const releaseKeyboard=()=>{if(!keyboardLease.current)return;keyboardLease.current=false;void latestCapture.current(false).catch(()=>{});void invoke('set_shortcut_capture',{active:false}).catch(()=>{});};
  const key = settings.shortcut_key === undefined ? defaultShortcut() : settings.shortcut_key;
  useEffect(()=>{mounted.current=true;return()=>{mounted.current=false;releaseKeyboard();};},[]);
  useEffect(()=>{
    if(!recording)return;
    let modifier: Shortcut | undefined;
    const finish=(shortcut:Shortcut)=>{
      setRecording(false);
      setError(null);
      latestChange.current({shortcut_key:shortcut});
    };
    const keydown=(event:KeyboardEvent)=>{
      event.preventDefault();event.stopImmediatePropagation();
      if(event.repeat)return;
      if(event.key==='Escape'){setRecording(false);setError(null);return;}
      modifier=undefined;
      const result=shortcutFromEvent(event);
      if(result.error){setError(result.error);return;}
      if(!result.shortcut)return;
      if(shortcutModifier(result.shortcut.key)){modifier=result.shortcut;setError(null);}
      else finish(result.shortcut);
    };
    const keyup=(event:KeyboardEvent)=>{
      if(modifier&&!event.ctrlKey&&!event.altKey&&!event.shiftKey&&!event.metaKey){
        event.preventDefault();event.stopImmediatePropagation();finish(modifier);modifier=undefined;
      }
    };
    const cancel=()=>{setRecording(false);};
    const visibility=()=>{if(document.hidden)cancel();};
    window.addEventListener('keydown',keydown,true);window.addEventListener('keyup',keyup,true);window.addEventListener('blur',cancel);
    document.addEventListener('visibilitychange',visibility);
    const timer=window.setInterval(()=>{
      void Promise.all([invoke('set_shortcut_capture',{active:true}),latestCapture.current(true)]).catch(error=>{setRecording(false);setError(String(error));});
    },1000);
    return()=>{
      window.removeEventListener('keydown',keydown,true);window.removeEventListener('keyup',keyup,true);window.removeEventListener('blur',cancel);
      document.removeEventListener('visibilitychange',visibility);window.clearInterval(timer);releaseKeyboard();
    };
  },[recording]);
  async function record(){
    setStarting(true);setError(null);
    keyboardLease.current=true;
    try {await invoke('set_shortcut_capture',{active:true});
      if(!mounted.current){releaseKeyboard();return;}
      await latestCapture.current(true);
      if(mounted.current&&document.hasFocus()&&!document.hidden)setRecording(true);
      else {keyboardLease.current=true;releaseKeyboard();}
    }catch(error){releaseKeyboard();if(mounted.current)setError(String(error));}
    finally{if(mounted.current)setStarting(false);}
  }
  const locked = busy || starting || recording || padCapturing;
  const profiles = {label:'游戏配置', options:entries.map(entry => ({value:entry.id, label:entry.profile.name, localize:false as const}))};
  const targetOptions = (label: string, value: Target, onSelect: (target: Target) => void) => (
    <div className={styles.field}>{label}
      <OptionPicker label={label} value={targetValue(value)} disabled={locked}
        groups={[{options:[{value:'none',label:'不切换'}, {value:'restore',label:'恢复切换前的配置'}]}, profiles]}
        onChange={next => onSelect(targetFrom(next))}/>
    </div>
  );

  return <div>
    <section className={styles.panel}>
      <div className={styles.heading}>
        <div><h4>光标显示状态</h4><p>分别选择光标显示和隐藏时要使用的配置。</p></div>
        <label className={styles.toggle}>
          <input type="checkbox" checked={settings.cursor_enabled} disabled={locked} onChange={event => onChange({ cursor_enabled: event.target.checked })} />
          启用光标自动切换
        </label>
      </div>
      <div className={styles.controls}>
        {targetOptions('光标显示时', settings.cursor_shown, target => onChange({ cursor_shown: target }))}
        {targetOptions('光标隐藏时', settings.cursor_hidden, target => onChange({ cursor_hidden: target }))}
        <label>状态确认时间<div className={styles.delayInput}>
          <input aria-label="光标状态确认时间" type="number" min={0} max={5000} step={10} inputMode="numeric"
            value={delay} disabled={locked} onChange={event=>{setDelay(event.target.value);setDelayError(false);}}
            onBlur={saveDelay} onKeyDown={event=>{if(event.key==='Enter'){event.preventDefault();event.currentTarget.blur();}}}/><span>ms</span>
        </div></label>
      </div>
      {delayError&&<p className={styles.error} role="alert">请输入 0–5000 毫秒之间的整数。</p>}
      <p className={styles.hint}>光标状态连续保持该时间后才切换。设为 0 可取消额外等待；检测仍受约 50 ms 检查周期和配置加载时间影响。输入后按回车或移开焦点保存。</p>
      <p className={styles.hint}>「恢复切换前的配置」会回到本轮自动切换前使用的配置。</p>
    </section>
    <section className={styles.panel}>
      <div className={styles.heading}><div><h4>快捷键切换</h4><p>触发一次切到目标配置，松开后再触发一次返回。</p></div></div>
      <div className={styles.target}>
        <div className={styles.field}>目标配置
          <OptionPicker label="快捷键目标配置" value={settings.shortcut_profile??''} disabled={locked}
            groups={[{options:[{value:'',label:'不启用'}]}, profiles]}
            onChange={value=>onChange({shortcut_profile:value||null})}/>
        </div>
        <p className={styles.hint}>两种快捷键共用此目标，在映射或观察运行时生效。</p>
      </div>
      <div className={styles.shortcutGrid}>
        <section className={styles.card}>
          <div className={styles.heading}><h5>键盘快捷键</h5>
            {key ? <kbd translate="no">{shortcutLabel(key)}</kbd> : <span>未绑定</span>}
          </div>
          <p className={styles.hint}>可选择单键，或录入单键和 Ctrl、Alt、Shift 组合键。</p>
          <OptionPicker label="选择键盘按键" placeholder="选择键盘按键" value={null} groups={keyChoices} disabled={locked}
            onChange={next=>{setError(null);onChange({shortcut_key:{key:next,ctrl:false,alt:false,shift:false}});}}/>
          <div className={styles.shortcutActions}>
            <button type="button" className="secondary-btn" aria-label="录入切换快捷键" disabled={locked} onClick={()=>void record()}>{starting?'正在准备…':recording?'请按下快捷键…':'录入快捷键'}</button>
            {recording?<button type="button" className="ghost-btn" onClick={()=>{setRecording(false);setError(null);}}>取消录入</button>
              :<button type="button" className="ghost-btn" disabled={locked||!key} onClick={()=>{setError(null);onChange({shortcut_key:null});}}>清除键盘快捷键</button>}
          </div>
          {recording&&<p className={styles.hint} role="status">请按单键或组合键。Esc 取消录入，录入期间暂停游戏输出和自动切换。</p>}
          {error&&<p className={styles.error} role="alert">{error}</p>}
        </section>
        <PadShortcutSettings settings={settings} status={status} busy={busy||starting||recording}
          onChange={onChange} onPrepare={onPreparePad} onCapture={onPadCapture} onRecordingChange={setPadCapturing}/>
      </div>
      <p className={styles.hint}><kbd>F12</kbd> 可停止映射并释放输出。</p>
    </section>
    <section className={styles.panel}>
      <div className={styles.heading}>
        <div><h4>游戏内配置提示</h4><p>自动或手动切换成功后，在游戏所在屏幕显示当前配置。</p></div>
        <label className={styles.toggle}><input type="checkbox" aria-label="显示游戏内配置提示"
          checked={settings.overlay_enabled!==false} disabled={locked} onChange={event=>onChange({overlay_enabled:event.target.checked})}/>启用配置提示</label>
      </div>
      <div className={styles.controls}>
        <label>提示位置<select aria-label="配置提示位置" value={settings.overlay_position??'top_right'} disabled={locked||settings.overlay_enabled===false}
          onChange={event=>onChange({overlay_position:event.target.value as Settings['overlay_position']})}>
          <option value="top_right">右上角</option><option value="top_left">左上角</option><option value="bottom_right">右下角</option><option value="bottom_left">左下角</option>
        </select></label>
        <label>显示时长<select aria-label="配置提示显示时长" value={settings.overlay_duration_ms??2500} disabled={locked||settings.overlay_enabled===false}
          onChange={event=>onChange({overlay_duration_ms:Number(event.target.value)})}>
          {Array.from(new Set([1000,2500,4000,6000,8000,settings.overlay_duration_ms??2500])).sort((a,b)=>a-b).map(ms=><option key={ms} value={ms}>{ms/1000} 秒</option>)}
        </select></label>
        <button type="button" className="secondary-btn" disabled={locked||settings.overlay_enabled===false} onClick={()=>{setOverlayError(null);void invoke('preview_overlay').catch(error=>setOverlayError(String(error)));}}>预览配置提示</button>
      </div>
      {overlayError&&<p className={styles.error} role="alert">{overlayError}</p>}
      <div className={styles.overlayPreview} aria-label="配置提示样式示例">
        <span>Motion Studio · 配置已切换</span><strong translate="no">{entries.find(entry=>entry.id===settings.selected_profile)?.profile.name??'体感瞄准'}</strong><span>光标隐藏 · 自动切换</span>
      </div>
      <p className={styles.hint}>提示会自动消失，不抢焦点、不接收鼠标点击。连续切换会显示最新配置。支持窗口化和无边框全屏；独占全屏可能无法显示，可尝试改为无边框全屏。</p>
    </section>
  </div>;
}
