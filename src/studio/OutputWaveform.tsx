import {useState} from 'react';
import {t} from '../i18n';
import type {MouseTracePoint,PadTracePoint} from '../types';
import styles from './OutputMonitor.module.css';

type Point={time:number;x:number;y:number};
type Source='mouse'|'right'|'left';
const W=360,H=190,L=44,R=12,T=22,B=30;
const percent=(value:number)=>100*value/(value<0?32768:32767);
const number=(value:number)=>Number.isFinite(value)?String(Number(value.toFixed(1))):'—';

function mouseSeries(points:MouseTracePoint[],now:number,windowMs:number,sampleMs:number,cumulative:boolean):Point[]{
  if(points.length<2)return [];
  // Bucket deltas, not distance divided by the gap between mouse packets:
  // a quiet gap is zero output, not a slow movement spread over that gap.
  const buckets=new Map<number,[number,number]>();
  for(let i=1;i<points.length;i++){
    const previous=points[i-1],current=points[i];
    if(current.sequence!==previous.sequence+1)continue;
    const key=Math.floor(current.time_ms/sampleMs)*sampleMs;
    buckets.set(key,[current.position[0]-previous.position[0],current.position[1]-previous.position[1]]);
  }
  // Exclude the currently accumulating bucket from the velocity estimate.
  const end=Math.floor(now/sampleMs)*sampleMs;
  const start=Math.max(Math.ceil((now-windowMs)/sampleMs)*sampleMs,points[0].time_ms);
  const values:Point[]=[];
  let x=0,y=0;
  for(let time=start;time<end;time+=sampleMs){
    const delta=buckets.get(time)??[0,0];
    if(cumulative){
      if(!values.length)values.push({time,x,y});
      x+=delta[0];y+=delta[1];
      values.push({time:time+sampleMs,x,y});
    }else values.push({time,x:delta[0]*1000/sampleMs,y:delta[1]*1000/sampleMs});
  }
  if(!cumulative&&values.length){
    const last=values[values.length-1];
    values.push({...last,time:end});
  }
  return values;
}

function padSeries(points:PadTracePoint[],now:number,windowMs:number,source:Source):Point[]{
  const axis=source==='left'?0:2;
  const start=now-windowMs;
  const index=points.findIndex(point=>point.time_ms>=start);
  const visible=index<0?points.slice(-1):points.slice(Math.max(0,index-1));
  const values=visible.map(point=>({time:Math.max(start,point.time_ms),x:percent(point.axes[axis]),y:percent(point.axes[axis+1])}));
  const last=values.at(-1);
  if(last&&last.time<now)values.push({...last,time:now});
  return values;
}

export function OutputWaveform({mouse,pad,mouseTime,padTime,hasPad,sampleMs}:{
  mouse:MouseTracePoint[];pad:PadTracePoint[];mouseTime:number;padTime:number;hasPad:boolean;sampleMs:number;
}){
  const [source,setSource]=useState<Source>(()=>hasPad?'right':'mouse');
  const [cumulative,setCumulative]=useState(false);
  const [windowMs,setWindowMs]=useState(1000);
  const [cursor,setCursor]=useState<number>();
  const selected=hasPad?source:'mouse';
  const isMouse=selected==='mouse';
  const now=isMouse?mouseTime:padTime;
  const start=now-windowMs;
  const points=isMouse?mouseSeries(mouse,now,windowMs,sampleMs,cumulative):padSeries(pad,now,windowMs,selected);
  const max=isMouse?Math.max(1,...points.flatMap(point=>[Math.abs(point.x),Math.abs(point.y)])):100;
  const bound=isMouse?Math.ceil(max/10**Math.floor(Math.log10(max)))*10**Math.floor(Math.log10(max)):100;
  const x=(time:number)=>L+(time-start)/windowMs*(W-L-R);
  const y=(value:number)=>T+(1-value/bound)/2*(H-T-B);
  const path=(axis:'x'|'y')=>points.map((point,index)=>index===0?`M${x(point.time)},${y(point[axis])}`:`H${x(point.time)}V${y(point[axis])}`).join(' ');
  const hovered=cursor===undefined?undefined:points.reduce<Point|undefined>((best,point)=>!best||Math.abs(point.time-(start+cursor*windowMs))<Math.abs(best.time-(start+cursor*windowMs))?point:best,undefined);
  const reading=hovered??points.at(-1);
  const unit=isMouse?cumulative?'计数':'计数/秒':'%';
  const peak=Math.max(0,...points.flatMap(point=>[Math.abs(point.x),Math.abs(point.y)]));
  return <div className={styles.waveform}>
    <div className={styles.tools} role="group" aria-label="波形输出来源">
      {([['mouse','鼠标'],...(hasPad?[['right','右摇杆'],['left','左摇杆']]:[])] as [Source,string][]).map(([value,label])=>
        <button type="button" className="ghost-btn" key={value} aria-pressed={selected===value} onClick={()=>{setSource(value);setCursor(undefined);}}>{label}</button>)}
    </div>
    <div className={styles.waveOptions}>
      {isMouse?<div className={styles.tools} role="group" aria-label="鼠标波形类型">
        <button type="button" className="ghost-btn" aria-pressed={!cumulative} onClick={()=>setCumulative(false)}>位移速度</button>
        <button type="button" className="ghost-btn" aria-pressed={cumulative} onClick={()=>setCumulative(true)}>累计位移</button>
      </div>:<span className={styles.explanation}>最终摇杆幅度</span>}
      <div className={styles.tools} role="group" aria-label="波形时间范围">
        {[500,1000,3000].map(ms=><button type="button" className="ghost-btn" key={ms} aria-pressed={windowMs===ms}
          onClick={()=>{setWindowMs(ms);setCursor(undefined);}}>{ms/1000} s</button>)}
      </div>
    </div>
    <div className={styles.legend}>
      <span className={styles.horizontal}>X · 水平</span><span className={styles.vertical}>Y · 垂直</span>
      <span>{t(unit)}{isMouse?' · '+t('自动量程'):''}</span>
    </div>
    <div className={styles.plot}>
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={t('最终输出随时间变化')} tabIndex={0}
        onKeyDown={event=>{
          if(event.key==='ArrowLeft'||event.key==='ArrowRight'){
            event.preventDefault();setCursor(Math.max(0,Math.min(1,(cursor??1)+(event.key==='ArrowLeft'?-1:1)*sampleMs/windowMs)));
          }else if(event.key==='Escape')setCursor(undefined);
        }}
        onPointerMove={event=>{const rect=event.currentTarget.getBoundingClientRect();setCursor(Math.max(0,Math.min(1,((event.clientX-rect.left)/rect.width*W-L)/(W-L-R))));}}
        onPointerLeave={()=>setCursor(undefined)}>
        <title>{t('最终输出随时间变化')}</title>
        {[-1,-0.5,0,0.5,1].map(ratio=><g key={ratio}>
          <path className={ratio===0?styles.zeroLine:styles.gridLines} d={`M${L} ${y(bound*ratio)}H${W-R}`}/>
          <text className={styles.axisLabel} x={L-6} y={y(bound*ratio)+4} textAnchor="end">{number(bound*ratio)}</text>
        </g>)}
        {[0,0.25,0.5,0.75,1].map(ratio=><g key={ratio}>
          <path className={styles.gridLines} d={`M${x(start+windowMs*ratio)} ${T}V${H-B}`}/>
          <text className={styles.axisLabel} x={x(start+windowMs*ratio)} y={H-10} textAnchor="middle">{Math.round((ratio-1)*windowMs)}</text>
        </g>)}
        <text className={styles.axisLabel} x={W-R} y={13} textAnchor="end">ms</text>
        {points.length>1&&<><path className={styles.trace} d={path('x')}/><path className={styles.traceY} d={path('y')}/></>}
        {hovered&&<><path className={styles.cursor} d={`M${x(hovered.time)} ${T}V${H-B}`}/>
          <circle className={styles.end} cx={x(hovered.time)} cy={y(hovered.x)} r={3}/>
          <circle className={styles.dotY} cx={x(hovered.time)} cy={y(hovered.y)} r={3}/></>}
      </svg>
      {points.length<2&&<span className={styles.empty}>操作手柄后显示波形</span>}
    </div>
    <div className={styles.caption}>
      <strong>{reading?`X ${number(reading.x)} · Y ${number(reading.y)}`:'—'} {t(unit)}</strong>
      <span>{hovered?`${Math.round(hovered.time-now)} ms`:t('窗口峰值')+' '+number(peak)+' '+t(unit)}</span>
    </div>
    <p className={styles.explanation}>{isMouse
      ?cumulative?'累计位移从当前窗口起点计数；曲线越陡，移动越快。':'位移速度按 8 ms 分段统计；可观察停顿和拖尾，短于分段的变化会合并。'
      :'幅度固定为 ±100%；平台表示持续推杆，回到 0 表示回中。'}</p>
    <p className={styles.explanation}>{isMouse?'鼠标：X 正向右，Y 正向下。':'摇杆：X 正向右，Y 正向上。'}</p>
  </div>;
}
