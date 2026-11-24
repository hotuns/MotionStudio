import {useId, type ReactNode} from 'react';
import {useLanguage} from '../i18n';
import type {useCurveHandles} from './useCurveHandles';
import styles from './MotionControls.module.css';

const ticks = [0, 0.25, 0.5, 0.75, 1];
export const plotNumber = (value: number) => String(Number(value.toPrecision(5)));
type Plot = ReturnType<typeof useCurveHandles>;

export function CurvePlot({plot, title, xLabel, yLabel, formatX=plotNumber, formatY=plotNumber,
  label, valueText, fixed, hint, legend, children, parameters=false}: {
  plot: Plot; title: string; xLabel: string; yLabel: string;
  formatX?: (value: number) => string; formatY?: (value: number) => string;
  label: (index: number) => string; valueText: (index: number) => string;
  fixed?: (index: number) => boolean; hint: string; legend?: ReactNode; children: ReactNode; parameters?: boolean;
}) {
  useLanguage();
  const id = useId();
  const {visible, selected, range:[maxX,maxY], surface, dragging, handlers, setSelection} = plot;
  const active = visible[selected];
  const left = (p: {0:number}) => p[0]/maxX*100;
  const top = (p: {1:number}) => (1-p[1]/maxY)*100;
  return <figure className={styles.stickPlot} data-parameters={parameters || undefined}>
    <figcaption id={id} className={styles.stickPlotCaption}><strong>{title}</strong>{legend}</figcaption>
    <div className={styles.stickAxisTitle}>{yLabel}</div>
    <div className={styles.stickPlotFrame}>
      <div className={styles.stickYAxis} aria-hidden="true">{ticks.map(v=><span key={v} style={{bottom:v*100+'%'}}>{formatY(v*maxY)}</span>)}</div>
      <div ref={surface} role="group" aria-labelledby={id} aria-describedby={id+'-hint'} className={styles.stickPlotSurface}
        data-dragging={dragging} {...handlers}>
        <svg viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
          <path className={styles.curveGrid} d="M25 0V100M50 0V100M75 0V100M0 25H100M0 50H100M0 75H100"/>
          {children}
        </svg>
        {visible.map((p,i)=><button key={i} type="button" className={styles.stickPlotPoint}
          style={{left:left(p)+'%', top:top(p)+'%'}} data-curve-point={i} data-fixed={fixed?.(i)}
          aria-label={label(i)} title={label(i)} aria-pressed={i===selected}
          aria-keyshortcuts={parameters?'ArrowUp ArrowDown ArrowLeft ArrowRight':'ArrowUp ArrowDown ArrowLeft ArrowRight Delete'}
          onClick={e=>{if (e.detail===0) setSelection(i);}} onFocus={()=>setSelection(i)}/>)}
        {dragging && active && <span aria-hidden="true" className={styles.stickPlotValue}
          data-edge={left(active)<15?'left':left(active)>85?'right':undefined} data-below={top(active)<18}
          style={{left:left(active)+'%', top:top(active)+'%'}}>{valueText(selected)}</span>}
      </div>
      <div className={styles.stickXAxis} aria-hidden="true">{ticks.map(v=><span key={v} style={{left:v*100+'%'}}>{formatX(v*maxX)}</span>)}</div>
    </div>
    <div className={styles.stickAxisTitle}>{xLabel}</div>
    <p id={id+'-hint'} className={styles.hint}>{hint}</p>
  </figure>;
}
