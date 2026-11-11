import {useEffect,useRef,useState} from 'react';
import {createPortal} from 'react-dom';
import type {NavCommand,Status} from '../types';
import {Dialog} from './Controls';
import {OptionDialog} from './OptionPicker';
import * as dom from './gamepadDom';
import styles from './GamepadNavigation.module.css';
const directions=['up','down','left','right'];
type Edit={element:HTMLInputElement;original:string};
type Keyboard={element:HTMLInputElement|HTMLTextAreaElement;value:string;title:string};
type Choice={element:HTMLSelectElement;options:{value:string;label:string;disabled:boolean}[];original:string;title:string};
export function GamepadNavigation({status,foreground,blocked,onSave,onPlay,onDisable,canPlay,canSave}:{status?:Status;foreground:boolean;blocked:boolean;onSave:()=>void;onPlay:()=>void;onDisable:()=>void;canPlay:boolean;canSave:boolean}) {
  const [hint,setHint]=useState(''),[edit,setEdit]=useState<Edit|null>(null),[keyboard,setKeyboard]=useState<Keyboard|null>(null),[choice,setChoice]=useState<Choice|null>(null),[quick,setQuick]=useState(false),[caps,setCaps]=useState(false);
  const enabled=!!status?.ui_navigation&&!!status.device&&foreground;
  const latest=useRef({status,blocked,enabled,onSave,onPlay,onDisable,canPlay,canSave});latest.current={status,blocked,enabled,onSave,onPlay,onDisable,canPlay,canSave};
  const baseline=useRef<number|null>(null),pending=useRef<NavCommand[]>([]),repeat=useRef<{command:NavCommand;next:number}|null>(null),lastFrame=useRef(0),lastDevice=useRef(''),lastZone=useRef('toolbar');
  const restore=(el:HTMLElement)=>requestAnimationFrame(()=>{if(el.isConnected)dom.focus(el);});
  useEffect(()=>{
    document.documentElement.dataset.gamepad=enabled?'on':'off';
    baseline.current=status?.navigation_serial??0;pending.current=[];repeat.current=null;
    if(enabled){requestAnimationFrame(()=>{const root=dom.scope();if(!dom.targets(root).includes(document.activeElement as HTMLElement))dom.focus(root.querySelector<HTMLElement>('[data-workbench-home]')??dom.targets(root)[0]);});}
    else{setChoice(null);setKeyboard(null);setQuick(false);setEdit(null);}
    return()=>{delete document.documentElement.dataset.gamepad;};
  },[enabled,status?.device?.path]);
  useEffect(()=>{
    const serial=status?.navigation_serial??0,path=status?.device?.path??'';
    if(!enabled||blocked||baseline.current===null||serial<baseline.current||path!==lastDevice.current){baseline.current=serial;lastDevice.current=path;pending.current=[];repeat.current=null;return;}
    const fresh=(status?.navigation_events??[]).filter(e=>e.serial>baseline.current!);
    baseline.current=serial;pending.current.push(...fresh.map(e=>e.command));if(pending.current.length>16)pending.current=pending.current.slice(-16);
  },[enabled,blocked,status?.navigation_serial,status?.device?.path]);
  useEffect(()=>{
    if(!enabled)return;
    const update=()=>{setHint(dom.label(document.activeElement));lastZone.current=document.activeElement?.closest<HTMLElement>('[data-pad-zone]')?.dataset.padZone??lastZone.current;};document.addEventListener('focusin',update);update();
    const observer=new MutationObserver(()=>{requestAnimationFrame(()=>{if(!latest.current.enabled||latest.current.blocked)return;const root=dom.scope();if(!dom.targets(root).includes(document.activeElement as HTMLElement)){const zone=root.querySelector<HTMLElement>('[data-pad-zone="'+lastZone.current+'"]')??root.querySelector<HTMLElement>('[data-pad-zone="dialog-body"]')??root;dom.focus(dom.targets(zone)[0]);}});});
    observer.observe(document.body,{childList:true,subtree:true});
    return()=>{observer.disconnect();document.removeEventListener('focusin',update);};
  },[enabled]);
  useEffect(()=>{
    if(edit?.element.isConnected)edit.element.dataset.padEditing='true';
    return()=>{if(edit)delete edit.element.dataset.padEditing;};
  },[edit]);
  const openKeyboard=(element:HTMLInputElement|HTMLTextAreaElement)=>setKeyboard({element,value:element.value,title:dom.label(element)});
  const openChoice=(element:HTMLSelectElement)=>setChoice({element,options:Array.from(element.options).map(option=>({value:option.value,label:option.text,disabled:option.disabled||(option.parentElement instanceof HTMLOptGroupElement&&option.parentElement.disabled)})),original:element.value,title:dom.label(element)});
  const act=(command:NavCommand)=>{
    if(!latest.current.enabled||latest.current.blocked)return;
    const root=dom.scope();let element=document.activeElement;
    if(directions.includes(command)){
      if(edit&&edit.element.isConnected){
        const el=edit.element,step=Number(el.step)||1,min=el.min===''?-Infinity:Number(el.min),max=el.max===''?Infinity:Number(el.max),sign=command==='left'||command==='down'?-1:1;
        const value=Math.min(max,Math.max(min,(Number(el.value)||0)+step*sign*(command==='up'||command==='down'?10:1)));
        dom.writeValue(el,String(Number(value.toFixed(6))));
      }else dom.move(command as 'up'|'down'|'left'|'right');
      return;
    }
    if(command==='back'){
      if(edit){dom.writeValue(edit.element,edit.original);setEdit(null);return;}
      if(root instanceof HTMLDialogElement){const event=new Event('cancel',{cancelable:true});root.dispatchEvent(event);if(!event.defaultPrevented)root.close();return;}
      const home=root.querySelector<HTMLButtonElement>('[data-workbench-home]')??undefined;
      const expanded=Array.from(root.querySelectorAll<HTMLDetailsElement>('details[open]'));
      if(expanded.length){
        const current=element instanceof HTMLElement?element.closest<HTMLDetailsElement>('details[open]'):null;
        expanded.forEach(el=>el.removeAttribute('open'));
        dom.focus(current?.querySelector<HTMLElement>(':scope > summary')??home);return;
      }
      if(home?.getAttribute('aria-current')!=='page')home?.click();
      dom.focus(home);return;
    }
    if(command==='previous_zone'||command==='next_zone'){setEdit(null);dom.changeZone(command==='previous_zone'?-1:1);return;}
    if(command==='sidebar'){if(!(root instanceof HTMLDialogElement))dom.focus(root.querySelector<HTMLElement>('[data-workbench-home]')??undefined);return;}
    if(command==='save'){if(!(root instanceof HTMLDialogElement)&&latest.current.canSave){setEdit(null);latest.current.onSave();}return;}
    if(command==='menu'){if(!(root instanceof HTMLDialogElement))setQuick(true);return;}
    if(command==='confirm'||command==='edit'){
      if(edit){setEdit(null);return;}
      if(!dom.targets(root).includes(element as HTMLElement)){dom.focus(dom.targets(root)[0]);return;}
      if(element instanceof HTMLSelectElement){openChoice(element);return;}
      if(element instanceof HTMLInputElement||element instanceof HTMLTextAreaElement){
        if(element.readOnly)return;
        if(element instanceof HTMLInputElement&&['range','number'].includes(element.type)){setEdit({element,original:element.value});return;}
        if(element instanceof HTMLTextAreaElement||['text','search','email','url','password','tel'].includes(element.type)){openKeyboard(element);return;}
      }
      if(command==='confirm'&&element instanceof HTMLElement)element.click();
    }
  };
  const action=useRef(act);action.current=act;
  useEffect(()=>{
    let frame=0;const tick=(now:number)=>{
      const state=latest.current;
      if(!state.enabled||state.blocked){pending.current=[];repeat.current=null;lastFrame.current=now;frame=requestAnimationFrame(tick);return;}
      if(lastFrame.current&&now-lastFrame.current>250){pending.current=[];repeat.current=null;baseline.current=state.status?.navigation_serial??0;}
      const dt=Math.min((now-lastFrame.current)/1000,0.05);lastFrame.current=now;
      const command=pending.current.shift();
      if(command){action.current(command);repeat.current=directions.includes(command)?{command,next:now+360}:null;}
      const held=state.status?.navigation_held??[];
      if(repeat.current&&!held.includes(repeat.current.command))repeat.current=null;
      if(!command&&repeat.current&&now>=repeat.current.next){action.current(repeat.current.command);repeat.current.next=now+110;}
      if(!edit&&!keyboard&&!choice){const axis=state.status?.axes[3]??0;if(Math.abs(axis)>0.35){const root=dom.scope();const scroll=root instanceof HTMLDialogElement?root.querySelector<HTMLElement>('[data-pad-zone="dialog-body"]'):document.querySelector<HTMLElement>('[data-pad-scroll]');scroll?.scrollBy({top:axis*750*dt});}}
      frame=requestAnimationFrame(tick);
    };frame=requestAnimationFrame(tick);return()=>cancelAnimationFrame(frame);
  },[edit,keyboard,choice]);
  const closeKeyboard=()=>{if(keyboard)restore(keyboard.element);setKeyboard(null);};
  const closeChoice=()=>{if(choice)restore(choice.element);setChoice(null);};
  const append=(text:string)=>setKeyboard(current=>{if(!current)return current;const value=current.value+text;return current.element.maxLength>=0&&value.length>current.element.maxLength?current:{...current,value};});
  return <>
    {enabled&&<div className={styles.hint} aria-label="手柄操作提示"><span><kbd>方向</kbd> 移动　<kbd>A</kbd> {edit?'完成调整':'确认'}　<kbd>B</kbd> {edit?'撤销调整':'返回'}　<kbd>LB/RB</kbd> 切区域　<kbd>Y</kbd> 保存　<kbd>Menu</kbd> 快捷操作</span><strong>{edit?'调整中 · 左右微调，上下 10 步':blocked?'正在录入或处理操作':hint||'选择一个控件'}</strong></div>}
    {keyboard&&createPortal(<Dialog title="屏幕键盘" description={keyboard.title} onClose={closeKeyboard} onSubmit={()=>{dom.writeValue(keyboard.element,keyboard.value);closeKeyboard();}}
      actions={<><button type="button" className="secondary-btn" onClick={closeKeyboard}>取消输入</button><button type="submit" className="primary-btn">完成输入</button></>}>
      <output className={styles.text} aria-label="待输入文字">{keyboard.value?<span translate="no">{keyboard.value}</span>:'尚未输入'}</output>
      <div className={styles.keys}>{['1234567890','qwertyuiop','asdfghjkl','zxcvbnm','.:\\/_-+'].map((row,i)=><div key={i}>{Array.from(row).map(key=><button type="button" className="secondary-btn" key={key} aria-label={'输入 '+(caps?key.toUpperCase():key)} onClick={()=>append(caps?key.toUpperCase():key)}>{caps?key.toUpperCase():key}</button>)}</div>)}</div>
      <div className={styles.keyTools}><button type="button" className="secondary-btn" onClick={()=>setCaps(!caps)}>{caps?'切为小写':'切为大写'}</button><button type="button" className="secondary-btn" onClick={()=>append(' ')}>空格</button><button type="button" className="secondary-btn" onClick={()=>setKeyboard(current=>current&&{...current,value:Array.from(current.value).slice(0,-1).join('')})}>退格</button><button type="button" className="secondary-btn" onClick={()=>setKeyboard(current=>current&&{...current,value:''})}>清空</button>{keyboard.element instanceof HTMLTextAreaElement&&<button type="button" className="secondary-btn" onClick={()=>append('\n')}>换行</button>}</div>
    </Dialog>,document.body)}
    {choice&&<OptionDialog title="选择选项" description={choice.title} value={choice.original} onClose={closeChoice}
      groups={[{options:choice.options.map(option=>({...option,localize:false}))}]}
      onChoose={value=>{if(!choice.element.matches(':disabled'))dom.writeValue(choice.element,value);closeChoice();}}/>}
    {quick&&createPortal(<Dialog title="手柄快捷操作" onClose={()=>setQuick(false)} actions={<button type="button" className="secondary-btn" onClick={()=>setQuick(false)}>返回界面</button>}>
      <div className={styles.options}>
        <button type="button" className="primary-btn" disabled={!canPlay} onClick={()=>{setQuick(false);onPlay();}}>启用当前配置并继续游戏</button>
        <button type="button" className="secondary-btn" disabled={!canSave} onClick={()=>{setQuick(false);onSave();}}>保存配置</button>
        <button type="button" className="secondary-btn" onClick={()=>{
          setQuick(false);
          document.querySelector<HTMLButtonElement>('[data-profile-picker] button[aria-haspopup="dialog"]')?.click();
        }}>切换当前配置</button>
        <button type="button" className="secondary-btn" onClick={()=>{setQuick(false);onDisable();}}>停用手柄界面操作</button>
      </div><p>继续游戏后停止界面导航。在窗口前台按 View + Menu 可暂停映射并切回手柄操作界面。</p>
    </Dialog>,document.body)}
  </>;
}
