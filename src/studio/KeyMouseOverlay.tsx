import {modifierOutputs} from '../keyboard';
import {t} from '../i18n';
import {mouseTrajectory,signedCount} from '../mouse-trajectory';
import {outputLabel,type KeyMouseEvent,type MouseTracePoint} from '../types';
import type {OverlayStatus} from '../controller-overlay';
import styles from './KeyMouseOverlay.module.css';

const KEY_ECHO_MS=700;
const MOUSE_ECHO_MS=450;
const TRAIL_MS=300;
const modifierRank=new Map(modifierOutputs.map((name,index)=>[name,index]));
const keyOrder=(a:string,b:string)=>(modifierRank.get(a)??100)-(modifierRank.get(b)??100)||a.localeCompare(b);

// Walk back through real edges to recover even a chord pressed and released
// entirely between two status frames. Separate taps must stay separate.
function lastReleasedKeys(held:string[],events:KeyMouseEvent[],ageMs:number) {
  const state=new Set(held);
  let releaseAge:number|undefined;
  for(let i=events.length-1;i>=0;i--){
    const edge=events[i];
    if(edge.age_ms+ageMs>KEY_ECHO_MS)break;
    if(edge.kind!=='button'||!edge.name.startsWith('key:'))continue;
    if(edge.down){
      if(releaseAge!==undefined)break;
      state.delete(edge.name);
    }else{
      if(releaseAge!==undefined&&edge.age_ms-releaseAge>90)break;
      releaseAge??=edge.age_ms;
      state.add(edge.name);
    }
  }
  return releaseAge===undefined?[]:[...state].filter(name=>!held.includes(name)).sort(keyOrder);
}

function Keycaps({names,limit,released=false}:{names:string[];limit:number;released?:boolean}) {
  const visible=names.slice(0,limit);
  return <span className={styles.caps} data-released={released} aria-label={names.map(outputLabel).join(' + ')}>
    {visible.map(name=><kbd key={name}>{outputLabel(name)}</kbd>)}
    {names.length>limit&&<small>+{names.length-limit}</small>}
  </span>;
}

const mouseButtons=[
  ['mouse:left','M26 6C17 7 11 14 11 26v4h15Z'],
  ['mouse:right','M30 6c10 1 16 9 16 20v4H30Z'],
  ['mouse:middle','M28 13a3.5 3.5 0 0 1 3.5 3.5v8a3.5 3.5 0 0 1-7 0v-8A3.5 3.5 0 0 1 28 13Z'],
  ['mouse:x2','M4 27h4v10H4Z'],
  ['mouse:x1','M4 40h4v10H4Z'],
] as const;

function MouseButtons({held,recent}:{held:string[];recent:KeyMouseEvent[]}) {
  return <svg className={styles.mouse} viewBox="0 0 51 66" role="img" aria-label="鼠标按键">
    <path className={styles.shell} d="M28 3C16 3 8 12 8 27v15c0 13 6 21 19 21s22-8 22-21V27C49 12 41 3 28 3Z"/>
    {mouseButtons.map(([name,d])=>{
      const state=held.includes(name)?'held':recent.some(e=>e.kind==='button'&&e.name===name&&!e.down)?'released':'idle';
      return <path key={name} className={styles.mouseButton} data-state={state} d={d}><title>{outputLabel(name)}</title></path>;
    })}
  </svg>;
}

function Wheel({events}:{events:KeyMouseEvent[]}) {
  const ticks=[0,0,0,0];
  for(const event of events){
    if(event.kind!=='wheel')continue;
    const [x,y]=event.delta;
    ticks[y>0?0:1]+=Math.abs(y);
    ticks[x<0?2:3]+=Math.abs(x);
  }
  const symbols=['↑','↓','←','→'];
  return <div className={styles.wheel} aria-label="滚轮">
    <span className={styles.label}>滚轮</span>
    <div className={styles.ticks}>{ticks.some(Boolean)?ticks.map((count,index)=>count>0&&
      <span key={index}>{symbols[index]}{count>1&&<small>{count}</small>}</span>):<span className={styles.quiet}>—</span>}</div>
  </div>;
}

function Movement({points,ageMs}:{points:MouseTracePoint[];ageMs:number}) {
  const from=points.findIndex(point=>(point.age_ms??0)+ageMs<=TRAIL_MS);
  // Keep the preceding cumulative position as the origin for the first delta.
  const tail=from<0?[]:points.slice(Math.max(0,from-1));
  const {path,start,end,delta}=mouseTrajectory(tail,128,40,4);
  return <div className={styles.movement}>
    <svg viewBox="0 0 128 40" role="img" aria-label="鼠标相对位移轨迹">
      <path className={styles.grid} d="M16 0v40m16-40v40m16-40v40m16-40v40m16-40v40m16-40v40m16-40v40M0 10h128M0 20h128M0 30h128"/>
      {tail.length>1&&<>
        <circle className={styles.origin} cx={start[0]} cy={start[1]} r="1.8"/>
        <polyline className={styles.trail} points={path}/>
        <circle className={styles.tip} cx={end[0]} cy={end[1]} r="2.6"/>
      </>}
    </svg>
    <div className={styles.counts} aria-label="相对位移（计数）">
      <span>X <b>{signedCount(delta[0])}</b></span><span>Y <b>{signedCount(delta[1])}</b></span>
    </div>
  </div>;
}

export function KeyMouseOverlay({status,ageMs,width}:{status?:OverlayStatus;ageMs:number;width:number}) {
  const preview=status?.emitting&&!status.binding_capture&&!status.ui_navigation?status.output_preview:null;
  const held=preview?.key_mouse?.held??[];
  const events=preview?.key_mouse?.recent??[];
  const keys=held.filter(name=>name.startsWith('key:')).sort(keyOrder);
  const released=lastReleasedKeys(keys,events,ageMs);
  const recentMouse=events.filter(edge=>edge.age_ms+ageMs<=MOUSE_ECHO_MS);
  const limit=width<240?3:width<400?5:8;
  const live=!!preview;
  const state=live?'实际输出':status?.state==='observing'?'仅观察输入':'输出已暂停';
  return <section className={styles.panel} data-live={live} aria-label="键鼠输出">
    <header className={styles.header}><strong>键鼠输出</strong><span className={styles.status} aria-label={t(state)}><i/>{t(state)}</span></header>
    <div className={styles.keyboard} aria-label="键盘">
      {keys.length>0&&<Keycaps names={keys} limit={limit}/>}
      {released.length>0&&<span className={styles.echo}>
        <span className={styles.label}>刚松开</span><Keycaps names={released} limit={keys.length?1:limit} released/>
      </span>}
      {!keys.length&&!released.length&&<span className={styles.idleKeys}>{live?'等待按键':'—'}</span>}
    </div>
    <div className={styles.mouseRow}>
      <MouseButtons held={held} recent={recentMouse}/><Wheel events={recentMouse}/>
      <Movement points={preview?.mouse??[]} ageMs={ageMs}/>
    </div>
  </section>;
}
