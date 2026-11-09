import { useEffect, useState } from 'react';
import type { MouseTracePoint, Status, VirtualPadPreview } from '../types';
import styles from './OutputMonitor.module.css';

const TRACE_MS = 3000;
const MAX_POINTS = 385;
const PLOT_WIDTH = 320;
const PLOT_HEIGHT = 150;
const PAD = 12;

type Trail = { points: MouseTracePoint[]; firstSequence: number };

function appendTrail(trail: Trail, incoming: MouseTracePoint[]) {
  const next = incoming.at(-1);
  const last = trail.points.at(-1);
  if (!next || next.sequence < trail.firstSequence || (last && next.sequence < last.sequence)) return trail;
  if (last?.sequence === next.sequence && last.position.every((value, axis) => value === next.position[axis])) return trail;

  const recent = incoming.filter(point => point.sequence >= trail.firstSequence);
  const first = recent[0].sequence;
  const before = last && first <= last.sequence + 1 ? trail.points.filter(point => point.sequence < first) : [];
  const merged = before.concat(recent);
  const start = merged.findIndex(point => point.time_ms >= next.time_ms - TRACE_MS);
  return { ...trail, points: merged.slice(Math.max(0, start - 1)).slice(-MAX_POINTS) };
}

function signed(value: number) {
  return value > 0 ? '+' + value : String(value);
}

function MousePath({ points }: { points: MouseTracePoint[] }) {
  const first = points[0]?.position ?? [0, 0];
  const last = points.at(-1)?.position ?? first;
  const xs = points.map(point => point.position[0]);
  const ys = points.map(point => point.position[1]);
  const minX = Math.min(first[0], ...xs), maxX = Math.max(first[0], ...xs);
  const minY = Math.min(first[1], ...ys), maxY = Math.max(first[1], ...ys);
  const scale = Math.min((PLOT_WIDTH - 2 * PAD) / Math.max(80, maxX - minX),
    (PLOT_HEIGHT - 2 * PAD) / Math.max(36, maxY - minY));
  const centerX = (minX + maxX) / 2, centerY = (minY + maxY) / 2;
  const x = (value: number) => PLOT_WIDTH / 2 + (value - centerX) * scale;
  const y = (value: number) => PLOT_HEIGHT / 2 + (value - centerY) * scale;
  return <figure className={styles.trajectory}>
    <div className={styles.plot}>
      <svg viewBox={`0 0 ${PLOT_WIDTH} ${PLOT_HEIGHT}`} role="img" aria-label="鼠标相对位移轨迹">
        <path className={styles.gridLines} d="M40 0V150 M80 0V150 M120 0V150 M160 0V150 M200 0V150 M240 0V150 M280 0V150 M0 30H320 M0 60H320 M0 90H320 M0 120H320"/>
        {points.length > 1 && <>
          <polyline className={styles.trace} points={points.map(point=>`${x(point.position[0]).toFixed(1)},${y(point.position[1]).toFixed(1)}`).join(' ')} />
          <circle className={styles.start} cx={x(first[0])} cy={y(first[1])} r={3} />
          <circle className={styles.end} cx={x(last[0])} cy={y(last[1])} r={4} />
        </>}
      </svg>
      {points.length < 2 && <span className={styles.empty}>等待鼠标位移</span>}
    </div>
    <figcaption className={styles.caption}>
      <span>相对位移（计数）</span>
      <strong>X {signed(last[0] - first[0])} · Y {signed(last[1] - first[1])}</strong>
    </figcaption>
  </figure>;
}

function StickOutput({ label, x, y }: { label: string; x: number; y: number }) {
  const horizontal = x / (x < 0 ? 32768 : 32767);
  const vertical = y / (y < 0 ? 32768 : 32767);
  return <div className={styles.stick}>
    <svg viewBox="0 0 54 54" role="img" aria-label={label}>
      <circle className={styles.stickRing} cx={27} cy={27} r={24} />
      <path className={styles.gridLines} d="M27 3V51 M3 27H51" />
      <circle className={styles.end} cx={27 + horizontal * 19} cy={27 - vertical * 19} r={4} />
    </svg>
    <div><strong>{label}</strong><span>X {signed(Math.round(horizontal * 100))}% · Y {signed(Math.round(vertical * 100))}%</span></div>
  </div>;
}

function VirtualOutput({ pad }: { pad: VirtualPadPreview }) {
  return <div className={styles.virtualOutput}>
    <h4>虚拟手柄输出</h4>
    <div className={styles.sticks}>
      <StickOutput label="左摇杆" x={pad.left_x} y={pad.left_y} />
      <StickOutput label="右摇杆" x={pad.right_x} y={pad.right_y} />
    </div>
    <p className={styles.caption}>
      <span>LT <strong>{Math.round(pad.left_trigger / 255 * 100)}%</strong></span>
      <span>RT <strong>{Math.round(pad.right_trigger / 255 * 100)}%</strong></span>
    </p>
  </div>;
}

export function OutputMonitor({ status, visible }: { status?: Status; visible: boolean }) {
  const preview = status?.output_preview;
  const mouse = preview?.mouse;
  const [trail, setTrail] = useState<Trail>({ points: [], firstSequence: 0 });
  useEffect(() => {
    if (mouse) setTrail(current=>appendTrail(current, mouse));
  }, [mouse]);
  if (!visible) return null;

  const mode = preview ? status?.emitting ? '实际输出' : '模拟输出' : '输出已暂停';
  const last = mouse?.at(-1);
  const clear = () => {
    if (last) setTrail({ points: [last], firstSequence: last.sequence + 1 });
  };

  return <section className={styles.output}>
    <div className={styles.heading}>
      <h3>鼠标轨迹</h3>
      <span className={styles.badge} data-live={!!preview && !!status?.emitting} role="status">{mode}</span>
    </div>
    {preview ? <>
      <MousePath points={trail.points} />
      <div className={styles.footer}>
        <span>近 3 秒轨迹 · 停手后保留</span>
        <button type="button" className="ghost-btn" disabled={trail.points.length < 2} onClick={clear}>清空轨迹</button>
      </div>
      {preview.pad && <VirtualOutput pad={preview.pad} />}
    </> : <p className="studio-muted">启用输出或开始观察输入后显示轨迹。</p>}
  </section>;
}
