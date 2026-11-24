import {invoke} from '@tauri-apps/api/core';
import {useEffect, useMemo, useRef, useState} from 'react';
import type {StickResponsePoint} from '../types';
import {CurvePlot} from './CurvePlot';
import type {useCurveHandles} from './useCurveHandles';
import styles from './MotionControls.module.css';

export type NativePreview = [graph: [number, number, number][], travel: number];
type Shape = [number, number][];

// Segment exponents stay the same when their handles move. Sample each shape with
// the native preview once, then scale its geometry to the two handles while dragging.
function useSegmentShapes(points: StickResponsePoint[]) {
  const key = [...new Set(points.slice(0, -1).map(p => p[2] ?? 1).filter(p => p !== 1))].sort((a,b) => a-b).join(',');
  const cache = useRef(new Map<number, Shape>());
  const [shapes, setShapes] = useState(cache.current);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let active = true;
    const powers = key ? key.split(',').map(Number) : [];
    const next = new Map<number, Shape>();
    for (const power of powers) {
      const known = cache.current.get(power);
      if (known) next.set(power, known);
    }
    const missing = powers.filter(power => !next.has(power));
    const load = async () => {
      // Large imported tables can use distinct exponents for every interval.
      for (let start = 0; active && start < missing.length; start += 8) {
        await Promise.all(missing.slice(start, start + 8).map(async power => {
          const [graph] = await invoke<NativePreview>('preview_right_stick_curve', {
            curve: {points: [[0, 0, power], [1, 1]]}, targetPercent: 0,
          });
          next.set(power, graph.slice(1, -1).map(([x, , y]) => [x / 100, y / 100]));
        }));
      }
      if (active) { cache.current = next; setShapes(next); setFailed(false); }
    };
    void load().catch(() => { if (active) setFailed(true); });
    return () => { active = false; };
  }, [key]);
  return {shapes, failed};
}

export const percent = (fraction: number) => Number((fraction * 100).toPrecision(9));
const plotXY = (x: number, y: number) => `${(x * 100).toFixed(4)} ${((1 - y) * 100).toFixed(4)}`;
export function StickResponsePlot({plot}: {plot: ReturnType<typeof useCurveHandles<StickResponsePoint>>}) {
  const points = plot.visible;
  const {shapes, failed} = useSegmentShapes(points);
  const path = useMemo(()=>points.slice(0, -1).map(([x, y, power = 1], i) => {
    const [endX, endY] = points[i + 1];
    const shape = shapes.get(power);
    if (power !== 1 && endY !== y && !shape) return '';
    const bend = (endY === y ? [] : shape ?? []).map(([t, rate]) =>
      'L' + plotXY(x + t * (endX - x), y + rate * (endY - y))).join('');
    return 'M' + plotXY(x, y) + bend + 'L' + plotXY(endX, endY);
  }).join(''), [points, shapes]);
  return <>
    <CurvePlot plot={plot} title="游戏摇杆曲线" xLabel="右摇杆推杆量（%）" yLabel="游戏转速（%）"
      formatX={v=>String(percent(v))} formatY={v=>String(percent(v))}
      label={i=>`控制点 ${i+1}，推杆 ${percent(points[i][0])}%，转速 ${percent(points[i][1])}%`}
      valueText={i=>`${percent(points[i][0])}% → ${percent(points[i][1])}%`}
      fixed={i=>i===0 || i===points.length-1}
      legend={<span className={styles.curveLegend}><span>线性参考</span><span>游戏响应</span></span>}
      hint="点击图中空白处添加点，拖动点调整曲线。选中后可用方向键微调。">
      <path className={styles.curveReference} d="M0 100L100 0"/>
      <path className={styles.curveProcessed} d={path}/>
    </CurvePlot>
    {failed && <p role="status" className={styles.hint}>曲线暂未更新，请检查当前参数。</p>}
  </>;
}
