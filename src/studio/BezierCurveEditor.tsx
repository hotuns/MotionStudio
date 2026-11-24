import {invoke} from '@tauri-apps/api/core';
import {useId, useLayoutEffect, useRef, useState} from 'react';
import {bounds, fixedAnchor, moveBezier, removeBezier, type BezierDomain} from '../bezier';
import {t} from '../i18n';
import type {ResponsePoint} from '../types';
import {CurvePlot, plotNumber} from './CurvePlot';
import {CurvePointField} from './CurvePointField';
import {useCurveHandles} from './useCurveHandles';
import {useCurvePreview} from './useCurvePreview';
import styles from './MotionControls.module.css';

const axes = {
  game:['游戏摇杆曲线','推杆量（%）','游戏转速（%）'],
  acceleration:['自定义加速曲线','输入转速（°/s）','倍率'],
  action:['动作响应曲线','输入幅度（%）','输出幅度（%）'],
  stick_mouse:['摇杆鼠标响应曲线','摇杆推杆量（%）','鼠标速度（%）'],
} as const;
const fit = (value: number, min: number, max: number) =>
  Math.min(max,min*2**Math.ceil(Math.log2(Math.max(1,value*1.2/min))));

function SegmentParameters({points,start,domain,onChange}: {
  points: ResponsePoint[]; start: number; domain: BezierDomain; onChange: (points: ResponsePoint[])=>void;
}) {
  const [draft,setDraft]=useState<string|null>(null);
  const id=useId();
  const [a,b,c,d]=points.slice(start,start+4);
  const dx=d[0]-a[0], dy=d[1]-a[1];
  if (dy===0) return null;
  const saved=[(b[0]-a[0])/dx,(b[1]-a[1])/dy,(c[0]-a[0])/dx,(c[1]-a[1])/dy];
  const value=draft??'cubic-bezier('+saved.map(plotNumber).join(', ')+')';
  const parts=value.trim().replace(/^cubic-bezier\s*\((.*)\)$/i,'$1').split(/\s*[,，]\s*/);
  const [x1,y1,x2,y2]=parts.map(Number);
  const h1: ResponsePoint=[a[0]+dx*x1,a[1]+dy*y1], h2: ResponsePoint=[a[0]+dx*x2,a[1]+dy*y2];
  const valid=parts.length===4 && parts.every(s=>s.trim() && Number.isFinite(Number(s)))
    && [x1,x2].every(x=>x>=0 && x<=1) && (domain!=='game' || [y1,y2].every(y=>y>=0 && y<=1))
    && [h1,h2].every(p=>p[1]>=0 && p[1]<=bounds(domain)[1]);
  function apply() {
    if (!valid || draft===null) return;
    const next=[...points];
    next.splice(start+1,2,h1,h2);
    setDraft(null); onChange(next);
  }
  return <div className={styles.bezierParameters}>
    <label htmlFor={id}>本段 cubic-bezier 参数</label>
    <input id={id} value={value} spellCheck={false} aria-invalid={draft!==null && !valid}
      onChange={e=>setDraft(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'){e.preventDefault();apply();}}}/>
    <button type="button" className="secondary-btn" disabled={!valid || draft===null} onClick={apply}>应用到本段</button>
    {draft!==null && !valid && <small role="status">请输入四个逗号分隔的数值，手柄需位于允许的坐标范围内。</small>}
  </div>;
}

export function BezierCurveEditor({domain,points,onChange}: {
  domain: BezierDomain; points: ResponsePoint[]; onChange: (next: ResponsePoint[])=>void;
}) {
  const [full,setFull]=useState(false);
  const [target,setTarget]=useState(20);
  const [pending,setPending]=useState(false);
  const [error,setError]=useState('');
  const request=useRef(0);
  const id=useId();
  const key=JSON.stringify([domain,points]);
  useLayoutEffect(()=>{
    setPending(false); setError('');
    return ()=>{request.current++;};
  },[key]);
  const [maxX,maxY]=bounds(domain);
  const normalized=domain!=='acceleration';
  const range: ResponsePoint=normalized || full ? [maxX,maxY] :
    [fit(Math.max(...points.map(p=>p[0])),100,maxX),fit(Math.max(...points.map(p=>p[1])),1,maxY)];
  const scale=normalized?100:1;
  const limit=domain==='game'?256:32;
  const canAdd=(points.length-1)/3+1<limit;
  const fixed=(i: number,p: ResponsePoint[])=>fixedAnchor(domain,i,p);
  const removable=(i: number,p: ResponsePoint[])=>i>0 && i<p.length-1 && i%3===0;
  const plot=useCurveHandles({
    points,range,fixed,removable,onChange,
    move:(p,i,x,y)=>moveBezier(p,i,x,y,domain),
    removePoint:(p,i)=>removeBezier(p,i,domain),
    onEmptyPress:x=>void add(x),
  });
  const {visible,selected,dragging,range:[width,height]}=plot;
  const first=Math.min(selected-selected%3,visible.length-4);
  const last=visible.length-1;
  const [title,xLabel,yLabel]=axes[domain];
  const format=(v: number)=>plotNumber(v*scale);
  const coordinate=(i: number)=>format(visible[i][0])+(normalized?'%':' °/s')+' → '+format(visible[i][1])+(normalized?'%':'×');
  const pointName=(i: number)=>i%3===0?t(`锚点 ${i/3+1}`):t(i%3===1?'起始手柄':'结束手柄');
  const xy=(i: number)=>[100*visible[i][0]/width,100*(1-visible[i][1]/height)].join(' ');
  let path='M0 '+(100*(1-visible[0][1]/height))+' L'+xy(0);
  for(let i=1;i<visible.length;i+=3) path+=' C'+xy(i)+' '+xy(i+1)+' '+xy(i+2);
  path+=' H100';
  const probeKey=domain==='game'?JSON.stringify([visible,target]):undefined;
  const probe=useCurvePreview(probeKey,()=>invoke<number>('preview_bezier_travel',{points:visible,targetPercent:target}));

  async function add(rawX: number) {
    if (!canAdd || dragging || pending) return;
    const x=Math.fround(Math.min(maxX,Math.max(0,rawX)));
    const token=++request.current;
    setPending(true); setError('');
    try {
      const [next,index]=await invoke<[ResponsePoint[],number]>('split_bezier_curve',{points,travel:x,domain});
      if (token!==request.current) return;
      onChange(next);
      plot.selectAndFocus(index);
    } catch (e) {
      if (token===request.current) setError(String(e));
    } finally { if (token===request.current) setPending(false); }
  }
  function edit(axis: 0|1, text: string) {
    const v=Number(text)/scale;
    if (!text.trim() || !Number.isFinite(v) || v<0 || v>(axis===0?maxX:maxY)) return '数值超出此轴范围。';
    const [x,y]=points[selected];
    const next=moveBezier(points,selected,axis===0?v:x,axis===1?v:y,domain);
    if (Math.fround(next[selected][axis])!==Math.fround(v)) return '所选坐标超出了相邻锚点允许的范围。';
    if (next!==points) onChange(next);
  }
  return <fieldset className={styles.bezierEditor} disabled={pending}>
    <div className={styles.freeCurve}>
      <div className={styles.stickCurveActions}>
        <button type="button" className="secondary-btn" disabled={dragging || !canAdd}
          onClick={()=>void add((points[first][0]+points[first+3][0])/2)}>添加锚点</button>
        <button type="button" className="ghost-btn" disabled={dragging || !removable(selected,visible)} onClick={()=>plot.remove(selected)}>删除选中点</button>
        {!normalized && <button type="button" className="ghost-btn" disabled={dragging} onClick={()=>setFull(!full)}>{full?'适应当前曲线':'显示全范围'}</button>}
      </div>
      <CurvePlot plot={plot} title={title} xLabel={xLabel} yLabel={yLabel} formatX={format} formatY={format}
        label={i=>pointName(i)+' · '+coordinate(i)} valueText={coordinate} fixed={i=>fixed(i,visible)}
        showPoint={i=>i%3===0 || i===first+1 || i===first+2} tangent={i=>i%3!==0}
        hint="点图添加锚点，曲线会保持原形。拖动锚点改变响应值，拖动菱形手柄调整弯曲程度。">
        <path className={styles.curveReference} d={normalized?'M0 100L100 0':'M0 '+(100-100/height)+'H100'}/>
        <path className={styles.curveProcessed} data-series="processed" d={path}/>
        <path className={styles.bezierTangents} d={'M'+xy(first)+' L'+xy(first+1)+' M'+xy(first+2)+' L'+xy(first+3)}/>
      </CurvePlot>
      <details className={styles.pointPrecision}><summary>精确调整选中点</summary>
        <div className={styles.stickCurveActions}>
          {[first,first+1,first+2,first+3].map(i=><button key={i} type="button" className="secondary-btn"
            disabled={dragging} aria-pressed={i===selected} onClick={()=>plot.selectAndFocus(i)}>{pointName(i)}</button>)}
        </div>
        <div key={selected} className={styles.stickPointFields}>
          {([0,1] as const).map(axis=><CurvePointField key={axis} label={axis===0?xLabel:yLabel}
            max={(axis===0?maxX:maxY)*scale} value={Number((visible[selected][axis]*scale).toPrecision(12))}
            disabled={dragging || fixed(selected,visible)} onEdit={s=>edit(axis,s)}/>)}
        </div>
        {!dragging && <SegmentParameters key={first+'-'+key} points={points} start={first} domain={domain} onChange={onChange}/>}
      </details>
      {domain==='game' && <div className={styles.stickProbe}>
        <label htmlFor={id}>目标游戏转速（%）</label>
        <input id={id} type="range" min={0} max={100} step={1} value={target} onChange={e=>setTarget(e.currentTarget.valueAsNumber)}/>
        <output htmlFor={id}>{`${target}% 目标转速 → ${probe?.key===probeKey && probe?.data!=null ? Number(probe.data.toFixed(2)) : '…'}% 虚拟推杆`}</output>
        {probe?.key===probeKey && probe && probe.data===undefined && <p role="status" className={styles.hint}>曲线暂未更新，请检查当前参数。</p>}
      </div>}
      {error && <p role="status" className={styles.hint}>{t(error)}</p>}
    </div>
  </fieldset>;
}
