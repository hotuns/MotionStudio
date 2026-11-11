import { lazy, Suspense, useState } from 'react';
import { Quaternion, Vector3 } from 'three';
import { buttonNames, labels, type Device, type Status } from '../types';
import Controller from './Controller';
import { OutputMonitor } from './OutputMonitor';
import type { ControllerPreviewFailure } from './Controller3D';
import styles from './DevicePage.module.css';

const Controller3D = lazy(() => import('./Controller3D'));
const orientationLabels = ['水平朝向', '俯仰角', '横滚角'];
const accelerations = [
  { label: '向上加速度', index: 3 },
  { label: '向右加速度', index: 4 },
  { label: '向前加速度', index: 5 },
];
const tiltLabels = ['抬头倾角', '横滚倾角'];
const degrees = 180 / Math.PI;

function reading(value: number | undefined, precision: number, unit: string) {
  return typeof value === 'number' && Number.isFinite(value) ? value.toFixed(precision) + unit : '—';
}

function orientation(status?: Status) {
  const values = status?.motion.orientation_quat;
  if (!values || values.length !== 4 || !values.every(Number.isFinite)) return null;
  const pose = new Quaternion().fromArray(values);
  return pose.lengthSq() > 0.01 ? pose.normalize() : null;
}

// Same canonical-to-Y-up basis as flexinput_core::frames. The preview origin
// is local to this page. Both the numbers and mesh use the resulting rotation.
function viewPose(orientation: Quaternion, origin?: Quaternion) {
  const q = origin ? orientation.clone().premultiply(origin.clone().invert()) : orientation;
  return new Quaternion(-q.y, q.z, -q.x, q.w);
}

function anglesFor(pose: Quaternion) {
  const forward = new Vector3(0, 0, -1).applyQuaternion(pose);
  const right = new Vector3(1, 0, 0).applyQuaternion(pose);
  return [
    Math.atan2(forward.x, -forward.z) * degrees,
    Math.atan2(forward.y, Math.hypot(forward.x, forward.z)) * degrees,
    Math.asin(Math.max(-1, Math.min(1, -right.y))) * degrees,
  ].map(value => Math.abs(value) < 0.05 ? 0 : value);
}

export function DeviceMonitor({ device, status }: { device: Device; status?: Status }) {
  const [follow, setFollow] = useState(true);
  const [failure, setFailure] = useState<ControllerPreviewFailure>();
  const [origin, setOrigin] = useState<{ revision: number; pose: Quaternion }>();
  const [showOutput, setShowOutput] = useState(!device.gyro);
  const rawPose = device.gyro ? orientation(status) : null;
  const pose = rawPose && viewPose(rawPose, origin?.revision === status?.revision ? origin?.pose : undefined);
  const angles = pose ? anglesFor(pose) : undefined;
  const following = follow && !!pose && !failure;
  const pressed = buttonNames.filter((_, index) => status?.buttons[index]);
  const accel = status?.motion.accel_fresh ? status.motion.quantities : undefined;
  const planar = <Controller device={device} status={status} />;
  const mode = status?.state === 'capturing' || status?.state === 'capture_release' ? status.message || '正在录入按键'
    : status?.ui_navigation ? '手柄操作界面' : status?.state === 'calibrating' ? '正在校准…'
    : status?.emitting ? '映射运行中' : status?.state === 'idle' ? '手柄已连接 · 映射已停止'
    : status?.state === 'preparing_output' ? '正在准备输出'
    : status ? '正在观察输入' : '开始观察后显示实时输入。';

  return <section className={'app-card ' + styles.monitor}>
    <div className={styles.deviceHeader}>
      <div>
        <h2>实时输入</h2>
        <p className="studio-muted" role="status">{mode}</p>
      </div>
      {device.gyro && <div className={styles.viewModes} role="group" aria-label="手柄预览视角">
        <button type="button" className="ghost-btn" aria-pressed={following} disabled={!pose || !!failure}
          onClick={() => setFollow(true)}>跟随姿态</button>
        <button type="button" className="ghost-btn" aria-pressed={!following}
          onClick={() => setFollow(false)}>固定正视</button>
      </div>}
    </div>
    <div className={styles.statusLayout}>
      <div className={styles.previewPanel}>
        <div className={styles.previewStage}>
          {following ? <Suspense fallback={planar}>
            <Controller3D device={device} status={status} pose={pose} onUnavailable={setFailure} />
          </Suspense> : planar}
        </div>
        <div className={styles.inputStrip}>
          <div className={styles.buttonList}>
            {pressed.length ? pressed.map(input => (
              <span key={input} className={styles.buttonChip} data-pressed="true">{labels[input] || input}</span>
            )) : <span className="studio-muted">{status ? '当前无按键按下。' : '等待输入'}</span>}
          </div>
          <div className={styles.triggerReadouts}>
            <span>LT <strong>{status ? reading((status.axes[4] ?? 0) * 100, 0, '%') : '—'}</strong></span>
            <span>RT <strong>{status ? reading((status.axes[5] ?? 0) * 100, 0, '%') : '—'}</strong></span>
          </div>
        </div>
        {failure && <p className={styles.previewHint}>{failure === 'graphics'
          ? '三维图形功能暂时不可用，已切换到固定正视。'
          : '三维模型文件载入失败，已切换到固定正视。'}</p>}
      </div>
      <div className={styles.panel}>
        <div className={styles.readoutModes + ' ' + styles.viewModes} role="group" aria-label="实时信号视图">
          <button type="button" className="ghost-btn" aria-pressed={!showOutput} onClick={()=>setShowOutput(false)}>体感数据</button>
          <button type="button" className="ghost-btn" aria-pressed={showOutput} onClick={()=>setShowOutput(true)}>输出预览</button>
        </div>
        <OutputMonitor key={`${status?.revision}:${status?.output_preview?.generation}:${status?.emitting}`}
          status={status} visible={showOutput}/>
        {!showOutput && <>
          <div className={styles.deviceHeader}>
            <h3>实时体感</h3>
            {device.gyro && <button className="secondary-btn" type="button" disabled={!rawPose}
              title="只重设预览的姿态参考" onClick={() => {
                if (rawPose && status) setOrigin({ revision: status.revision, pose: rawPose });
              }}>预览归零</button>}
          </div>
          {device.gyro ? <>
            {!angles && <p className="studio-muted">等待体感采样</p>}
            <dl className={styles.readouts}>
              {orientationLabels.map((label, i) => <div key={label}>
                <dt>{label}</dt><dd>{reading(angles?.[i], 1, '°')}</dd>
              </div>)}
            </dl>
            {device.accelerometer && <div className={styles.acceleration}>
              <p className="studio-muted">{accel ? '动态加速度' : '等待加速度计采样'}</p>
              <dl className={styles.readouts}>
                {accelerations.map(({ label, index }) => <div key={label}>
                  <dt>{label}</dt><dd>{reading(accel?.[index], 2, ' g')}</dd>
                </div>)}
              </dl>
            </div>}
            <details className={styles.sensorDetails}>
              <summary>传感器读数</summary>
              <dl className={styles.readouts}>
                {['陀螺仪 X', '陀螺仪 Y', '陀螺仪 Z'].map((label, i) => <div key={label}>
                  <dt>{label}</dt><dd>{reading(rawPose ? status?.gyro[i] : undefined, 2, ' °/s')}</dd>
                </div>)}
                {device.accelerometer && tiltLabels.map((label, i) => <div key={label}>
                  <dt>{label}</dt><dd>{reading(accel?.[i + 6], 2, '°')}</dd>
                </div>)}
              </dl>
            </details>
          </> : <p className="studio-muted">此接口没有体感数据。</p>}
        </>}
      </div>
    </div>
  </section>;
}
