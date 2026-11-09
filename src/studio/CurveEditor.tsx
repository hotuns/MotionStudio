import { useId } from 'react';
import styles from './MotionControls.module.css';

export function CurveEditor({points,onChange,maxX,maxY,xLabel='输入',yLabel='输出'}:{points:number[][];onChange:(points:number[][])=>void;maxX:number;maxY:number;xLabel?:string;yLabel?:string}) {
  const id=useId();
  const sx=Math.max(1,...points.map(p=>p[0]));const sy=Math.max(1,...points.map(p=>p[1]));
  const valid=points.every((p,i)=>p.every(Number.isFinite)&&p[0]>=0&&p[1]>=0&&(i===0||p[0]>points[i-1][0]));
  return <div className={styles.curve}>
    <svg viewBox="0 0 320 120" role="img" aria-label={`${xLabel}与${yLabel}曲线`}>
      <path d="M12 8V108H308" fill="none" stroke="var(--border-strong)" />
      {valid&&<polyline points={points.map(p=>`${12+p[0]/sx*296},${108-p[1]/sy*96}`).join(' ')} fill="none" stroke="var(--accent)" strokeWidth="2" />}
    </svg>
    <div className={styles.curveRows}>
      <span>{xLabel}</span><span>{yLabel}</span><span />
      {points.map((point,index)=><div className={styles.curveRow} key={index}>
        {point.map((value,axis)=><input key={axis} aria-label={`${axis===0?xLabel:yLabel} 控制点 ${index+1}`} id={`${id}-${index}-${axis}`} type="number" min={0} max={axis===0?maxX:maxY} step="any" value={Number.isFinite(value)?value:''} onChange={event=>onChange(points.map((p,i)=>i===index?p.map((v,a)=>a===axis?event.target.valueAsNumber:v):p))} />)}
        <button type="button" className="ghost-btn" disabled={points.length<=2} onClick={()=>onChange(points.filter((_,i)=>i!==index))} aria-label={`删除控制点 ${index+1}`}>移除</button>
      </div>)}
    </div>
    {!valid&&<p role="alert">横坐标需按从小到大排列，且所有值都需有效。</p>}
    <button type="button" className="secondary-btn" disabled={points.length>=32||!valid} onClick={()=>{
      let at=0;for(let i=1;i<points.length-1;i++)if(points[i+1][0]-points[i][0]>points[at+1][0]-points[at][0])at=i;
      onChange([...points.slice(0,at+1),[(points[at][0]+points[at+1][0])/2,(points[at][1]+points[at+1][1])/2],...points.slice(at+1)]);
    }}>添加控制点</button>
  </div>;
}
