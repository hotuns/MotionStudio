import {invoke} from '@tauri-apps/api/core';
import {useEffect, useId, useRef, useState} from 'react';
import type {StickResponsePoint} from '../types';
import {curveRows, insertCurvePoint, linearStickResponse, maxStickResponsePoints, moveCurvePoint, parseCurveRows, parseCurveText} from '../stick-response';
import {CurvePointField} from './CurvePointField';
import {useCurveHandles} from './useCurveHandles';
import {useCurvePreview} from './useCurvePreview';
import {Dialog} from './Controls';
import {SettingField} from './SettingLayout';
import {ResponseCurve} from './StabilityCurves';
import {percent, StickResponsePlot, type NativePreview} from './StickResponsePlot';
import styles from './MotionControls.module.css';

function usePreview(points: StickResponsePoint[] | undefined, target: number) {
  const key = points && JSON.stringify([points,target]);
  const reply = useCurvePreview(key,()=>invoke<NativePreview>('preview_right_stick_curve',{
    curve:{points}, targetPercent:target,
  }));
  return reply?.key===key ? reply : undefined;
}

function PasteCurve({points, onSave, onClose}: {
  points: StickResponsePoint[]; onSave: (points: StickResponsePoint[])=>void; onClose: ()=>void;
}) {
  const [text, setText] = useState(()=>curveRows(points).map(row=>row.slice(0,row[2]?3:2).join(', ')).join('\n'));
  const textarea = useRef<HTMLTextAreaElement>(null);
  const parsed = parseCurveText(text);
  const preview = usePreview(parsed.points, 20);
  useEffect(()=>{ textarea.current?.focus(); textarea.current?.select(); }, []);
  return <Dialog title="粘贴曲线数据" onClose={onClose} onSubmit={()=>{if (parsed.points) onSave(parsed.points);}}
    error={parsed.error} className={styles.stickCurveDialog}
    actions={<><button type="button" className="secondary-btn" onClick={onClose}>取消</button>
      <button type="submit" className="primary-btn" disabled={!parsed.points}>使用这条曲线</button></>}>
    <p className={styles.hint}>每行填推杆量、游戏转速，可选第三列为到下一点的指数。支持逗号、空格和制表符，可整表粘贴 0–100 或 0–1 的数据。</p>
    <textarea ref={textarea} aria-label="曲线数据" className={styles.stickCurveText} spellCheck={false}
      value={text} onChange={e=>setText(e.currentTarget.value)}/>
    {parsed.points && <ResponseCurve title="游戏摇杆曲线" points={preview?.data?.[0]} xLabel="右摇杆推杆量（%）" yLabel="游戏转速（%）"/>}
    {preview && !preview.data && <p className={styles.hint} role="status">曲线暂未更新，请检查当前参数。</p>}
  </Dialog>;
}

export function StickResponseEditor({points = linearStickResponse, onChange}: {
  points?: StickResponsePoint[]; onChange: (points: StickResponsePoint[])=>void;
}) {
  const [paste, setPaste] = useState(false);
  const [target, setTarget] = useState(20);
  const targetId = useId();
  const isEndpoint = (i: number, items: StickResponsePoint[]) => i === 0 || i === items.length-1;
  const plot = useCurveHandles({
    points, range:[1,1], move:moveCurvePoint, insert:insertCurvePoint, onChange,
    fixed:isEndpoint, removable:(i, items)=>!isEndpoint(i, items), initialSelection:points.length>2 ? 1 : 0,
  });
  const {visible, dragging, selected, selectAndFocus} = plot;
  const point = visible[selected];
  const endpoint = isEndpoint(selected, visible);
  const preview = usePreview(visible, target);

  function addPoint() {
    if (points.length >= maxStickResponsePoints) return;
    let gap = 0;
    for (let i = 1; i < points.length-1; i++) {
      if (points[i+1][0] - points[i][0] > points[gap+1][0] - points[gap][0]) gap = i;
    }
    const [left, right] = [points[gap], points[gap+1]];
    const x = (left[0] + right[0]) / 2;
    // The engine samples the midpoint of every curved interval for its preview.
    const mid = preview?.data?.[0].find(([travel])=>Math.abs(travel - x*100) < 0.00001);
    const y = mid ? mid[2] / 100 : (left[1] + right[1]) / 2;
    const added = insertCurvePoint(points, x, y);
    if (added.index >= 0) { onChange(added.points); selectAndFocus(added.index); }
  }
  function editValue(axis: number, text: string) {
    const rows = curveRows(points);
    rows[selected][axis] = text;
    const parsed = parseCurveRows(rows);
    if (!parsed.points) return parsed.error;
    const updated = [...points];
    updated[selected] = parsed.points[selected];
    onChange(updated);
  }
  return <SettingField label="游戏曲线点">
    <p className={styles.hint}>填入游戏从中心到满杆的原始响应，包含内外死区。转速按满杆转速的百分比填写，软件会反算所需推杆量。</p>
    <div className={styles.stickCurveActions}>
      <button type="button" className="secondary-btn" disabled={dragging || visible.length>=maxStickResponsePoints} onClick={addPoint}>添加控制点</button>
      <button type="button" className="ghost-btn" disabled={dragging} onClick={()=>setPaste(true)}>粘贴曲线数据</button>
    </div>
    <div className={styles.stickEditor}>
      <StickResponsePlot plot={plot}/>
      <div className={styles.stickInspector}>
        <strong>{endpoint ? (selected===0?'起点（固定）':'终点（固定）') : `控制点 ${selected+1}`}</strong>
        <div key={selected} className={styles.stickPointFields}>
          <CurvePointField step={0.1} label="推杆量（%）" value={percent(point[0])} disabled={endpoint || dragging} max={100} onEdit={text=>editValue(0,text)}/>
          <CurvePointField step={0.1} label="游戏转速（%）" value={percent(point[1])} disabled={endpoint || dragging} max={100} onEdit={text=>editValue(1,text)}/>
          {selected<visible.length-1 && <CurvePointField step={0.1} label="到下一点的指数" value={point[2]??1} disabled={dragging} onEdit={text=>editValue(2,text)}/>}
        </div>
        <button type="button" className="ghost-btn" disabled={endpoint || dragging} onClick={()=>plot.remove(selected)}>删除选中点</button>
        <div className={styles.stickProbe}>
          <label htmlFor={targetId}>目标游戏转速（%）</label>
          <input id={targetId} type="range" min={0} max={100} step={1} value={target} onChange={e=>setTarget(e.currentTarget.valueAsNumber)}/>
          <output htmlFor={targetId}>{`${target}% 目标转速 → ${preview?.data ? Number(preview.data[1].toFixed(2)) : '…'}% 虚拟推杆`}</output>
          {preview && !preview.data && <p role="status" className={styles.hint}>曲线暂未更新，请检查当前参数。</p>}
        </div>
      </div>
    </div>
    {paste && <PasteCurve points={points} onClose={()=>setPaste(false)} onSave={next=>{
      onChange(next); setPaste(false); selectAndFocus(next.length>2 ? 1 : 0);
    }}/>}
  </SettingField>;
}
