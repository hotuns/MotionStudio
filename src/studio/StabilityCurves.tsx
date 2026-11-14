import { invoke } from '@tauri-apps/api/core';
import { memo, useEffect, useId, useState } from 'react';
import { useLanguage } from '../i18n';
import type { Gyro } from '../types';
import styles from './MotionControls.module.css';

type Point = [x: number, reference: number, processed: number];
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

export function useStabilityPreview(gyro: Gyro, deviceNoise?: number) {
  const m = gyro.motion;
  // Send only the controls that change these curves, including when editing a layer.
  const request = JSON.stringify({
    gyro: {
      smoothing_ms: gyro.smoothing_ms,
      noise_threshold: gyro.noise_threshold,
      motion: {
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
  });
  const [curves, setCurves] = useState<StabilityPreview>();
  const [failedRequest, setFailedRequest] = useState<string>();

  useEffect(() => {
    let current = true;
    const timer = window.setTimeout(async () => {
      try {
        const result = await invoke<StabilityPreview>('preview_motion_stability', JSON.parse(request));
        if (current) {
          setCurves(result);
          setFailedRequest(undefined);
        }
      } catch {
        if (current) setFailedRequest(request);
      }
    }, 45);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [request]);

  return { curves, failed: failedRequest === request };
}

export function FlickReleaseCurve({strength}: {strength: number}) {
  const [points, setPoints] = useState<Point[]>();
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let current = true;
    const timer = window.setTimeout(() => {
      void invoke<Point[]>('preview_flick_release', {strength}).then(result => {
        if (current) {
          setPoints(result);
          setFailed(false);
        }
      }).catch(() => {
        if (current) setFailed(true);
      });
    }, 45);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [strength]);

  return <>
    <div className={styles.curveLegend}><span>匀速参考</span><span>当前释放曲线</span></div>
    <ResponseCurve title="Flick 转角释放" points={points} xLabel="时间进度（%）" yLabel="已完成转角（%）"/>
    {failed && <p role="status" className="studio-muted">曲线暂未更新，请检查当前参数。</p>}
  </>;
}

export function AccelerationCurve({acceleration}: {acceleration: Gyro['acceleration']}) {
  const request = JSON.stringify({acceleration});
  const [points, setPoints] = useState<Point[]>();
  const [failedRequest, setFailedRequest] = useState<string>();

  useEffect(() => {
    let current = true;
    const timer = window.setTimeout(async () => {
      try {
        const result = await invoke<Point[]>('preview_gyro_acceleration', JSON.parse(request));
        if (current) {
          setPoints(result);
          setFailedRequest(undefined);
        }
      } catch {
        if (current) setFailedRequest(request);
      }
    }, 45);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [request]);

  const failed = failedRequest === request;
  return <>
    <div className={styles.curveLegend}><span>参考线</span><span>当前设置</span></div>
    <ResponseCurve title="加速倍率预览" points={failed ? undefined : points} xLabel="输入转速（°/s）" yLabel="倍率"/>
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
