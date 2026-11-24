import {t} from '../i18n';
import type {ResponsePoint} from '../types';
import {CurvePlot, plotNumber} from './CurvePlot';
import {useCurveHandles, type PlotRange} from './useCurveHandles';
import {useCurvePreview} from './useCurvePreview';
import styles from './MotionControls.module.css';

export type CurveSample = [x: number, reference: number, output: number];
export type CurveParameter<K extends string> = {
  key: K; label: string; axis: 'x' | 'y'; value: number; min: number; max: number; step: number; unit: string;
  // Convert a configured parameter to the position of its guide in plot units.
  offset?: number; factor?: number; guideHeight?: number;
};

export function ParameterCurve<K extends string>({title, xLabel, yLabel, samples, controls, minimum=[1,1], onChange, preview}: {
  title: string; xLabel: string; yLabel: string; samples?: CurveSample[]; controls: CurveParameter<K>[];
  minimum?: PlotRange; onChange: (key: K, value: number) => void; preview: (key: K, value: number) => Promise<CurveSample[]>;
}) {
  const axisValue = (p: CurveParameter<K>) => (p.offset??0) + (p.factor??1)*p.value;
  const extent = (axis: 'x' | 'y', base: number) => {
    const needed = Math.max(0,...controls.filter(p=>p.axis===axis).map(axisValue));
    return needed>base ? needed*1.15 : base;
  };
  const width = extent('x', Math.max(minimum[0],samples?.at(-1)?.[0]??0));
  const height = extent('y', Math.max(minimum[1],...(samples??[]).map(p=>Math.max(p[1],p[2]))));
  // Separate guides remain independently reachable even when both thresholds are zero.
  const points: ResponsePoint[] = controls.map((p,i)=>p.axis==='x'
    ? [axisValue(p), p.guideHeight ?? (i===0 ? 0 : height*0.88)]
    : [width, axisValue(p)]);
  const valueAt = (p: ResponsePoint, i: number) => {
    const c = controls[i];
    return Number((((c.axis==='x'?p[0]:p[1])-(c.offset??0))/(c.factor??1)).toPrecision(12));
  };
  function move(items: ResponsePoint[], i: number, x: number, y: number) {
    const c = controls[i], axis = c.axis==='x'?0:1;
    const pos = axis===0 ? x : y;
    if (pos===items[i][axis]) return items;
    const raw = (pos-(c.offset??0))/(c.factor??1);
    const value = Math.min(c.max,Math.max(c.min,Number((Math.round(raw/c.step)*c.step).toPrecision(12))));
    const next: ResponsePoint = [...items[i]];
    next[axis] = (c.offset??0)+(c.factor??1)*value;
    return next[axis]===items[i][axis] ? items : items.map((p,j)=>i===j?next:p);
  }
  const plot = useCurveHandles({points, range:[width,height], move, contextKey:controls.map(c=>c.key).join(','),
    keyStep:(i, fine)=>{
      const step = Math.abs(controls[i].factor??1)*controls[i].step*(fine?1:5);
      return [step,step];
    },
    onChange:next=>{
      const i = next.findIndex((p,j)=>p[0]!==points[j][0] || p[1]!==points[j][1]);
      if (i>=0) onChange(controls[i].key,valueAt(next[i],i));
    },
  });
  const {visible, selected, range:[maxX,maxY]} = plot;
  const control = controls[selected], value = valueAt(visible[selected], selected);
  const changed = plot.dragging && value!==valueAt(points[selected],selected);
  const prefix = JSON.stringify([plot.dragId,control.key])+'|';
  const key = changed ? prefix+JSON.stringify([control.key,value]) : undefined;
  const reply = useCurvePreview(key, ()=>preview(control.key,value));
  const failed = !!key && reply?.key===key && !reply.data;
  const displayed = changed && reply?.key.startsWith(prefix) && !failed ? (reply.data??samples) : samples;
  const line = (axis: 1 | 2) => displayed?.map(p=>[p[0]/maxX*100,(1-p[axis]/maxY)*100].join(',')).join(' ');
  const label = (i: number) => t(controls[i].label)+' '+plotNumber(valueAt(visible[i],i))+controls[i].unit;
  return <div className={styles.parameterChart}>
    <CurvePlot plot={plot} parameters title={title} xLabel={xLabel} yLabel={yLabel}
      label={label} valueText={label} hint="拖动图上的标记调整参数，选中后可用方向键微调。">
      <polyline className={styles.curveReference} data-series="reference" points={line(1)}/>
      <polyline className={styles.curveProcessed} data-series="processed" points={line(2)}/>
      {visible.map(([x,y],i)=><path key={controls[i].key} className={styles.parameterGuide} data-selected={i===selected}
        d={controls[i].axis==='x' ? 'M'+(x/maxX*100)+' 0V100' : 'M0 '+((1-y/maxY)*100)+'H100'}/>)}
    </CurvePlot>
    <div className={styles.parameterLegend}>{controls.map((c,i)=><button type="button" key={c.key}
      aria-pressed={i===selected} onClick={()=>plot.selectAndFocus(i)}>{label(i)}</button>)}</div>
    {failed && <p role="status" className={styles.hint}>曲线暂未更新，请检查当前参数。</p>}
  </div>;
}
