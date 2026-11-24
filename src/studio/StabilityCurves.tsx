import {invoke} from '@tauri-apps/api/core';
import {memo, useId} from 'react';
import {useLanguage} from '../i18n';
import type {Gyro} from '../types';
import {nativeSmoothing, usesJsmAim} from '../motion-presets';
import {ParameterCurve, type CurveParameter, type CurveSample as Point} from './ParameterCurve';
import {useCurvePreview} from './useCurvePreview';
import styles from './MotionControls.module.css';

type StabilityPreview = {
  smoothing: Point[];
  idle: Point[];
  recovery: Point[];
  snap: Point[];
  brake: Point[];
  drift: Point[];
  measured_noise: boolean;
  noise_floor: number;
};

function stabilityArgs(gyro: Gyro, deviceNoise?: number) {
  const m = gyro.motion;
  // Send only the controls that change these curves, including when editing a layer.
  return {
    gyro: {
      output: gyro.output,
      jsm_response: {
        cutoff_recovery_dps:gyro.jsm_response?.cutoff_recovery_dps,
        smooth_time_ms:gyro.jsm_response?.smooth_time_ms,
        smooth_threshold_dps:gyro.jsm_response?.smooth_threshold_dps,
        decay_smoothing:gyro.jsm_response?.decay_smoothing,
        one_euro:gyro.jsm_response?.one_euro,
      },
      smoothing_ms: gyro.smoothing_ms,
      noise_threshold: gyro.noise_threshold,
      motion: {
        mode: m.mode,
        smoothing: m.smoothing,
        decay_threshold: m.decay_threshold,
        one_euro_cutoff: m.one_euro_cutoff,
        one_euro_beta: m.one_euro_beta,
        noise_gate: m.noise_gate,
        cutoff_dps: m.cutoff_dps,
        recovery_ms: m.recovery_ms,
        angle_snap: m.angle_snap,
        angle_snap_ease: m.angle_snap_ease,
        brake_strength: m.brake_strength,
        brake_threshold: m.brake_threshold,
        drift_correction: m.drift_correction,
      },
    },
    sensorNoise: deviceNoise ?? null,
  };
}
export function useStabilityPreview(gyro: Gyro, deviceNoise?: number) {
  const args = stabilityArgs(gyro,deviceNoise);
  const key = JSON.stringify(args);
  const reply = useCurvePreview(key, ()=>invoke<StabilityPreview>('preview_motion_stability',args));
  return {curves:reply?.data, failed:reply?.key===key && !reply.data};
}

type StabilityKey = 'smooth_time_ms'|'smooth_threshold_dps'|'smoothing_ms'|'decay_threshold'|'cutoff_dps'|
  'cutoff_recovery_dps'|'noise_threshold'|'recovery_ms'|'angle_snap'|'brake_threshold'|'brake_strength';
export function withStabilityValue(gyro: Gyro, key: StabilityKey, value: number): Gyro {
  switch (key) {
    case 'smooth_time_ms': case 'smooth_threshold_dps': case 'cutoff_recovery_dps':
      return {...gyro, jsm_response:{...gyro.jsm_response,[key]:value}};
    case 'smoothing_ms': case 'noise_threshold':
      return {...gyro,[key]:value};
    default:
      return {...gyro,motion:{...gyro.motion,[key]:value}};
  }
}
function parameter<K extends string>(key: K, label: string, value: number, min: number, max: number,
  step: number, unit: string, axis: 'x'|'y'='x'): CurveParameter<K> {
  return {key,label,value,min,max,step,unit,axis};
}
const charts = {
  smoothing:['平滑响应曲线','时间（ms）','角速度（°/s）'],
  idle:['低速抑制曲线','输入转速（°/s）','稳态输出转速（°/s）'],
  recovery:['输出恢复曲线','时间（ms）','输出比例（%）'],
  snap:['角度吸附曲线','输入方向角（°）','输出方向角（°）'],
  brake:['减速刹车曲线','10 ms 内转速减少量（°/s）','速度保留（%）'],
} as const;
export function StabilityCurve({kind, curves, gyro, deviceNoise, onChange}: {
  kind: keyof typeof charts; curves?: StabilityPreview; gyro: Gyro; deviceNoise?: number; onChange: (key: StabilityKey, value: number)=>void;
}) {
  const m = gyro.motion;
  const native = usesJsmAim(gyro);
  const controls: CurveParameter<StabilityKey>[] = [];
  switch (kind) {
    case 'smoothing': {
      if (native) {
        const smooth = nativeSmoothing(gyro);
        controls.push(parameter('smooth_time_ms','平滑窗口',smooth.window,0,2000,1,' ms'),
          parameter('smooth_threshold_dps','平滑阈值',smooth.threshold,0,2000,0.1,' °/s','y'));
      } else if (m.smoothing!=='one_euro') {
        controls.push(parameter('smoothing_ms','平滑时间',gyro.smoothing_ms,0,50,1,' ms'));
        if (m.smoothing==='decay') controls.push(parameter('decay_threshold','退出平滑的转速',m.decay_threshold,0.1,2000,0.1,' °/s','y'));
      }
      break;
    }
    case 'idle':
      if (native) controls.push(parameter('cutoff_dps','低速截止',m.cutoff_dps,0,100,0.1,' °/s'),
        parameter('cutoff_recovery_dps','完全恢复转速',gyro.jsm_response?.cutoff_recovery_dps??0,0,2000,0.1,' °/s'));
      else controls.push(parameter('noise_threshold','静止抑制',gyro.noise_threshold,0,5,0.05,' °/s'));
      break;
    case 'recovery':
      controls.push({...parameter('recovery_ms','截止后的恢复时间',m.recovery_ms,0,1000,1,' ms'),guideHeight:100});
      break;
    case 'snap':
      controls.push(parameter('angle_snap','水平／垂直吸附角',m.angle_snap,0,45,0.5,'°'));
      break;
    case 'brake':
      controls.push({...parameter('brake_threshold','刹车触发减速量',m.brake_threshold,0.01,1000,0.1,' °/s'),guideHeight:100},
        {...parameter('brake_strength','减速刹车强度',m.brake_strength,0,1,0.01,'','y'),offset:100,factor:-100});
      break;
  }
  const [title,xLabel,yLabel] = charts[kind];
  if (!controls.length) return <ResponseCurve title={title} xLabel={xLabel} yLabel={yLabel} points={curves?.[kind]}/>;
  const domain = {smoothing:[400,110],idle:[5,5],recovery:[160,100],snap:[90,90],brake:[20,100]} as const;
  return <ParameterCurve title={title} xLabel={xLabel} yLabel={yLabel} controls={controls} samples={curves?.[kind]} minimum={domain[kind]}
    onChange={onChange} preview={async (key,v)=>{
      const args = stabilityArgs(withStabilityValue(gyro,key,v),deviceNoise);
      return (await invoke<StabilityPreview>('preview_motion_stability',args))[kind];
    }}/>;
}

export function FlickReleaseCurve({duration, onChange}: {duration: number; onChange: (value: number)=>void}) {
  const reply = useCurvePreview('release',()=>invoke<Point[]>('preview_flick_release',{strength:0.5}));
  const endTime = Math.max(100,Math.min(2000,duration*1.5));
  const timed = (ms: number): Point[] => {
    if (!reply?.data) return [];
    const points: Point[] = reply.data.map(([progress,ref,y])=>[ms*progress/100,ref,y]);
    if (ms<endTime) points.push([endTime,100,100]);
    return points;
  };
  return <>
    <div className={styles.curveLegend}><span>匀速参考</span><span>当前释放曲线</span></div>
    <ParameterCurve title="拨动转角释放" xLabel="时间（ms）" yLabel="已完成转角（%）"
      controls={[{...parameter('duration','180° 基准释放时间',duration,0,2000,1,' ms'),guideHeight:100}]}
      minimum={[endTime,100]} samples={timed(duration)}
      onChange={(_,v)=>onChange(v)} preview={async (_,v)=>timed(v)}/>
    {reply && !reply.data && <p role="status" className="studio-muted">曲线暂未更新，请检查当前参数。</p>}
  </>;
}

type Acceleration = Gyro['acceleration'];
type AccelKey = 'start_dps' | 'full_dps' | 'fast_multiplier' | 'natural_half' | 'power_reference' | 'sigmoid_mid';
export function AccelerationCurve({acceleration:a, jsm, onChange}: {
  acceleration: Acceleration; jsm: boolean; onChange:(key: AccelKey, value: number)=>void;
}) {
  const read = (acceleration: Acceleration) => invoke<Point[]>('preview_gyro_acceleration',{acceleration,jsm});
  const key = JSON.stringify([a,jsm]);
  const reply = useCurvePreview(key,()=>read(a));
  const failed = reply?.key===key && !reply.data;
  const controls: CurveParameter<AccelKey>[] = [
    {...parameter('start_dps','加速起点',a.start_dps,0,Math.min(1000,Math.ceil(a.full_dps)-1),1,' °/s'),guideHeight:1},
  ];
  const offset = a.start_dps;
  switch (a.curve) {
    case 'natural': controls.push({...parameter('natural_half','达到半程倍率的速度',a.natural_half,0.01,4000,1,' °/s'),offset,guideHeight:(1+a.fast_multiplier)/2}); break;
    case 'power': controls.push({...parameter('power_reference','参考转速',a.power_reference,0.01,4000,1,' °/s'),offset}); break;
    case 'sigmoid': controls.push({...parameter('sigmoid_mid','过渡中点',a.sigmoid_mid,0,4000,1,' °/s'),offset}); break;
    default:
      controls.push({...parameter('full_dps','达到最高倍率',a.full_dps,Math.floor(a.start_dps)+1,2000,1,' °/s'),
        offset:jsm && (a.curve==='quadratic' || a.curve==='jump') ? offset : 0,guideHeight:a.fast_multiplier});
  }
  controls.push(parameter('fast_multiplier','快速转动倍率',a.fast_multiplier,1,4,0.05,'×','y'));
  return <>
    <div className={styles.curveLegend}><span>参考线</span><span>当前设置</span></div>
    <ParameterCurve title="加速倍率预览" xLabel="输入转速（°/s）" yLabel="倍率" controls={controls}
      minimum={[Math.max(200,a.full_dps*1.2),4]} samples={failed?undefined:reply?.data} onChange={onChange}
      preview={(key,value)=>read({...a,[key]:value})}/>
    {failed && <p role="status" className="studio-muted">曲线暂未更新，请检查当前参数。</p>}
  </>;
}

export const ResponseCurve = memo(function ResponseCurve({ title, points, xLabel, yLabel }: {
  title: string;
  points?: Point[];
  xLabel: string;
  yLabel: string;
}) {
  useLanguage();
  const captionId = useId();
  const xMax = points?.at(-1)?.[0] ?? 1;
  const yMax = Math.max(1, ...(points ?? []).map(([, reference, processed]) => Math.max(reference, processed)));
  const line = (series: 1 | 2) => points?.map(point =>
    `${(2 + point[0] / xMax * 316).toFixed(2)},${(110 - point[series] / yMax * 108).toFixed(2)}`
  ).join(' ');
  const tick = (value: number) => points ? Number(value.toFixed(2)) : '—';

  return <figure className={styles.responseCurve}>
    <figcaption id={captionId}>{title}</figcaption>
    <div className={styles.curveAxis}><span>{yLabel}</span><span>{tick(yMax)}</span></div>
    <svg viewBox="0 0 320 112" preserveAspectRatio="none" role="img" aria-labelledby={captionId}>
      <path className={styles.curveGrid} d="M80 0V112 M160 0V112 M240 0V112 M0 28H320 M0 56H320 M0 84H320"/>
      {points && <>
        <polyline data-series="processed" className={styles.curveProcessed} points={line(2)} />
        <polyline data-series="reference" className={styles.curveReference} points={line(1)} />
      </>}
    </svg>
    <div className={styles.curveAxis}><span>0</span><span>{xLabel}</span><span>{tick(xMax)}</span></div>
  </figure>;
});
