import {useState} from 'react';
import {t} from '../i18n';
import type {ResponsePoint} from '../types';
import {CurvePlot, plotNumber} from './CurvePlot';
import {CurvePointField} from './CurvePointField';
import {useCurveHandles} from './useCurveHandles';
import styles from './MotionControls.module.css';

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));
const tick = (v: number, step: number) => Number((Math.round(v/step)*step).toPrecision(12));
const limit = 32;

// Imported points can be closer than the drag increment. Never merge or reorder them at f32 precision.
function inside(edge: number, direction: 1 | -1, step: number) {
  const v = tick(edge, step);
  return Math.sign(Math.fround(v)-Math.fround(edge)) === direction ? v : tick(v+direction*step, step);
}

export function CurveEditor({points, onChange, kind, sampled}: {
  points: ResponsePoint[]; onChange: (points: ResponsePoint[]) => void; kind: 'action' | 'acceleration' | 'stick'; sampled?: ResponsePoint[];
}) {
  const normalized = kind !== 'acceleration';
  const maxX = normalized ? 1 : 4000, maxY = normalized ? 1 : 10;
  const stepX = normalized ? 0.001 : 0.1, stepY = normalized ? 0.001 : 0.01;
  const [full, setFull] = useState(false);
  const fit = (v: number, min: number, max: number) => Math.min(max, min*2**Math.ceil(Math.log2(Math.max(1, v*1.2/min))));
  const width = normalized || full ? maxX : fit(Math.max(...points.map(p=>p[0])), 100, maxX);
  const height = normalized || full ? maxY : fit(Math.max(...points.map(p=>p[1])), 1, maxY);
  const fixed = (i: number, items: ResponsePoint[]) => kind==='stick' && (i===0 || i===items.length-1);
  const removable = (i: number, items: ResponsePoint[]) => items.length>2 && !fixed(i,items);
  const xLabel = kind==='stick' ? '摇杆推杆量（%）' : normalized ? '输入幅度（%）' : '输入转速（°/s）';
  const yLabel = kind==='stick' ? '鼠标速度（%）' : normalized ? '输出幅度（%）' : '倍率';
  const scale = normalized ? 100 : 1;
  const display = (v: number) => plotNumber(v*scale);

  function move(items: ResponsePoint[], i: number, x: number, y: number) {
    if (fixed(i,items)) return items;
    const p = items[i];
    const lo = i ? inside(items[i-1][0], 1, stepX) : 0;
    const hi = i+1<items.length ? inside(items[i+1][0], -1, stepX) : maxX;
    const nx = x===p[0] || lo>hi ? p[0] : clamp(tick(x,stepX),lo,hi);
    const ny = y===p[1] ? p[1] : clamp(tick(y,stepY),0,maxY);
    if (nx===p[0] && ny===p[1]) return items;
    return items.map((other, index): ResponsePoint => i===index ? [nx,ny] : other);
  }
  function insert(items: ResponsePoint[], x: number, y: number) {
    x = clamp(tick(x, stepX),0,maxX);
    const right = items.findIndex(p=>Math.fround(p[0]) >= Math.fround(x));
    if (right>=0 && Math.fround(items[right][0])===Math.fround(x)) return {points:items,index:right};
    if (items.length>=limit) return {points:items,index:-1};
    const index = right<0 ? items.length : right;
    const p: ResponsePoint = [x, clamp(tick(y,stepY),0,maxY)];
    return {points:[...items.slice(0,index),p,...items.slice(index)],index};
  }
  const plot = useCurveHandles({points, range:[width,height], move, insert, fixed, removable, onChange});
  const {selected, visible, dragging, range:[viewX,viewY]} = plot;
  const chosen = visible[selected];
  const coordinate = (p: ResponsePoint) => display(p[0])+(normalized?'%':' °/s')+' → '+display(p[1])+(normalized?'%':'×');
  // The engine holds the first and last values outside the supplied X coordinates.
  const line = (sampled && visible===points ? sampled : [[0,visible[0][1]], ...visible, [viewX,visible[visible.length-1][1]]])
    .map(([x,y])=>[x/viewX*100,(1-y/viewY)*100].join(',')).join(' ');

  function add() {
    // Include the flat tails of curves with movable endpoints.
    const candidates: ResponsePoint[] = [[0,points[0][1]], ...points, [width,points[points.length-1][1]]];
    let gap = 0;
    for (let i=1; i<candidates.length-1; i++) {
      if (candidates[i+1][0]-candidates[i][0] > candidates[gap+1][0]-candidates[gap][0]) gap=i;
    }
    const [a,b] = candidates.slice(gap,gap+2);
    const mid = (a[0]+b[0])/2;
    const y = sampled?.find(p=>p[0]===mid)?.[1] ?? (a[1]+b[1])/2;
    const added = insert(points,mid,y);
    if (added.index>=0) {
      if (added.points!==points) onChange(added.points);
      plot.selectAndFocus(added.index);
    }
  }
  function exact(axis: 0 | 1, text: string) {
    const v = Number(text)/scale;
    if (!text.trim() || !Number.isFinite(v) || v<0 || v>(axis===0?maxX:maxY)) return '数值超出此轴范围。';
    if (axis===0 && ((selected>0 && Math.fround(points[selected-1][0])>=Math.fround(v)) ||
      (selected+1<points.length && Math.fround(points[selected+1][0])<=Math.fround(v)))) return '横坐标必须位于相邻控制点之间。';
    if (points[selected][axis]===v) return;
    const next: ResponsePoint = [...points[selected]];
    next[axis] = v;
    onChange(points.map((p,i)=>i===selected ? next : p));
  }
  return <div className={styles.freeCurve}>
    <div className={styles.stickCurveActions}>
      <button type="button" className="secondary-btn" onClick={add} disabled={dragging || points.length>=limit}>添加控制点</button>
      <button type="button" className="ghost-btn" disabled={dragging || !removable(selected,visible)} onClick={()=>plot.remove(selected)}>删除选中点</button>
      {!normalized && <button type="button" className="ghost-btn" disabled={dragging} onClick={()=>setFull(!full)}>{full?'适应当前曲线':'显示全范围'}</button>}
    </div>
    <CurvePlot plot={plot} title={kind==='stick'?'摇杆鼠标响应曲线':normalized?'动作响应曲线':'自定义加速曲线'} xLabel={xLabel} yLabel={yLabel}
      fixed={i=>fixed(i,visible)}
      formatX={display} formatY={display} label={i=>t(`控制点 ${i+1}`)+' · '+coordinate(visible[i])}
      valueText={i=>coordinate(visible[i])} hint="点击图中空白处添加点，拖动点调整曲线。选中后可用方向键微调。">
      <path className={styles.curveReference} d={normalized?'M0 100L100 0':`M0 ${100-100/viewY}H100`}/>
      <polyline className={styles.curveProcessed} data-series="processed" points={line}/>
    </CurvePlot>
    <details className={styles.pointPrecision}><summary>精确调整选中点</summary>
      <div key={selected} className={styles.stickPointFields}>
        <CurvePointField label={xLabel} value={Number((chosen[0]*scale).toPrecision(15))} disabled={dragging || fixed(selected,visible)} max={maxX*scale} onEdit={v=>exact(0,v)}/>
        <CurvePointField label={yLabel} value={Number((chosen[1]*scale).toPrecision(15))} disabled={dragging || fixed(selected,visible)} max={maxY*scale} onEdit={v=>exact(1,v)}/>
      </div>
    </details>
  </div>;
}
