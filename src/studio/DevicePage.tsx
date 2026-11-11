import { useEffect, useRef, useState, type ReactNode } from 'react';
import { listen } from '@tauri-apps/api/event';
import { buttonNames, labels, type Device, type Snapshot, type Status } from '../types';
import { DeviceMonitor } from './DeviceMonitor';
import { StudioIcon } from './StudioIcon';
import styles from './DevicePage.module.css';
import motionStyles from './MotionControls.module.css';

export default function DevicePage({ snapshot, devicePath, device, active, busy, canObserve, calibration, revealCalibration, onSelect, onObserve, onStop, onScan, onSettings }: {
  snapshot: Snapshot | null;
  devicePath: string;
  device?: Device | null;
  active: boolean;
  busy: boolean;
  canObserve: boolean;
  calibration?: ReactNode;
  revealCalibration: boolean;
  onSelect: (path: string) => void;
  onObserve: () => void;
  onStop: () => void;
  onScan: () => void;
  onSettings: () => void;
}) {
  const [sample, setSample] = useState<{ status: Status; received: number; restartCount?: number }>();
  const restartCount = snapshot?.restart_count;
  const calibrationDetails = useRef<HTMLDetailsElement>(null);
  const hasCalibration = !!calibration;
  useEffect(() => {
    if (!('__TAURI_INTERNALS__' in window)) return;
    let disposed = false;
    const subscription = listen<Status>('engine-status', ({ payload }) => {
      if (!disposed) setSample({ status: payload, received: performance.now(), restartCount });
    });
    // Snapshot polling also covers engines that stop publishing or a missing event bridge.
    void subscription.catch(() => {});
    return () => {
      disposed = true;
      void subscription.then(unlisten => unlisten()).catch(() => {});
    };
  }, [restartCount]);
  useEffect(() => {
    const details = calibrationDetails.current;
    if (!revealCalibration || !details) return;
    details.open = true;
    const frame = requestAnimationFrame(() => details.scrollIntoView({ block: 'start' }));
    return () => cancelAnimationFrame(frame);
  }, [revealCalibration, hasCalibration]);

  const devices = snapshot?.devices ?? [];
  const latest = snapshot?.engine_ready && sample && sample.restartCount === restartCount
    && performance.now() - sample.received < 400
    ? sample.status : snapshot?.status;
  const status = latest?.device?.path === device?.path && latest?.state !== 'faulted' && latest?.state !== 'waiting' ? latest : undefined;
  const pressed = buttonNames.filter((_, index) => status?.buttons[index]);
  const extended = buttonNames.slice(15).filter(button => device?.buttons.includes(button));
  const stats = status?.stats;
  const canScan = snapshot?.engine_ready && !active && !busy;
  const processed = status?.processed_axes;
  const mapping = !!latest?.emitting || !!latest?.output_backend;
  const stopLabel = latest?.state === 'preparing_output' ? '取消启动' : latest?.state === 'waiting' ? '取消等待'
    : mapping ? '停止映射' : '停止观察';
  const physical = devices.filter(entry => entry.origin !== 'virtual' && entry.gamepad);
  const inaccessible = physical.filter(entry => entry.open_error);

  return <div className={styles.page}>
    <section className={styles.connection} aria-label="连接状态">
      <div className="studio-device-controls">
        <label className="studio-field">映射使用的手柄
          <select value={devicePath} disabled={active || busy} onChange={event => onSelect(event.target.value)}>
            <option value="">选择手柄接口</option>
            {devicePath && !devices.some(entry => entry.path === devicePath) &&
              <option value={devicePath}>上次选择的接口 · 等待连接</option>}
            {devices.filter(entry => entry.gamepad && entry.path).map(entry => (
              <option key={entry.id} value={entry.path} disabled={entry.origin === 'virtual' || !!entry.open_error}>
                {entry.name} · {entry.origin === 'virtual' ? '虚拟手柄' : entry.gyro ? '体感可用' : '无体感'} · {entry.buttons.length} 键
              </option>
            ))}
          </select>
        </label>
        <div className={styles.connectionActions}>
          {active
            ? <button type="button" className={mapping ? 'danger-btn' : 'secondary-btn'} disabled={busy} onClick={onStop}>
              <StudioIcon name="stop" size={16} />{stopLabel}
            </button>
            : <button type="button" className="primary-btn" disabled={!canObserve || busy} onClick={onObserve}>开始观察输入</button>}
          <button type="button" className="secondary-btn" disabled={!canScan} title={active ? '停止后可重新检测' : undefined} onClick={onScan}>
            <StudioIcon name="refresh" size={16} />重新检测
          </button>
        </div>
      </div>
      {!physical.length && <p className={styles.connectionHint}>连接手柄后，点击重新检测。</p>}
      {device && <p className={styles.connectionHint}>
        <span translate="no">{device.name}</span> · {device.gyro ? '体感可用' : '无体感'}
      </p>}
      {inaccessible.map(entry => <p className={styles.connectionError} role="alert" key={entry.id}>
        <strong translate="no">{entry.name}</strong> · {entry.open_error}
      </p>)}
      {!!inaccessible.length && <button type="button" className="secondary-btn" onClick={onSettings}>检查驱动与读取权限</button>}
      {(latest?.state === 'faulted' || latest?.state === 'waiting') && latest.message &&
        <p className={latest.state === 'faulted' ? styles.connectionError : styles.connectionHint} role={latest.state === 'faulted' ? 'alert' : 'status'}>{latest.message}</p>}
    </section>
    {device && <DeviceMonitor key={device.path + ':' + restartCount} device={device} status={status} />}
    {calibration && <details className={styles.details} ref={calibrationDetails}>
      <summary className={styles.foldHeading}><strong>设备校准</strong><span>静止校准 · 六面校准 · 设备方向与噪声</span></summary>
      {calibration}
    </details>}
    <details className={styles.details}>
      <summary className={styles.foldHeading}><strong>输入诊断</strong><span>原始和处理后输入 · 设备接口</span></summary>
      {device && <div className={styles.diagnostics}>
        <div className={styles.deviceMeta}>
          <strong translate="no">{device.name}</strong>
          <span>设备 #{device.id}</span><span>VID {device.vendor.toString(16)} / PID {device.product.toString(16)}</span>
        </div>
        {extended.length > 0 && <div className={styles.buttonList}>
          <strong>扩展按键</strong>
          {extended.map(button => <span key={button} className={styles.buttonChip}
            data-pressed={pressed.includes(button)}>{labels[button] || button}</span>)}
        </div>}
        <dl className={styles.metrics}>
          <div><dt>体感事件</dt><dd>{stats ? Math.round(stats.gyro_hz) + ' Hz' : '—'}</dd></div>
          <div><dt>处理耗时 P99</dt><dd>{stats ? stats.process_p99_us + ' μs' : '—'}</dd></div>
          <div><dt>事件等待 P99</dt><dd>{stats ? stats.queue_p99_us + ' μs' : '—'}</dd></div>
          <div><dt>过期事件 / 重连</dt><dd>{stats ? stats.stale_events + ' / ' + status?.reconnects : '—'}</dd></div>
        </dl>
      </div>}
      {status && processed && <details className="studio-details">
        <summary>已应用配置的输入预览</summary>
        <p className="studio-muted">运行配置：<strong translate="no">{status.profile_name}</strong></p>
        <table className={motionStyles.inputTable}>
          <thead><tr><th>输入轴</th><th>原始输入</th><th>处理后输入</th></tr></thead>
          <tbody>{['左摇杆水平','左摇杆垂直','右摇杆水平','右摇杆垂直','左扳机','右扳机'].map((label,i)=>
            <tr key={label}><th scope="row">{label}</th>
              <td translate="no">{status.axes[i]?.toFixed(3)??'—'}</td><td translate="no">{processed[i]?.toFixed(3)??'—'}</td>
            </tr>
          )}</tbody>
        </table>
      </details>}
      <h3 className={styles.diagnosticHeading}>全部设备接口与诊断信息</h3>
      {!devices.length && <p className="studio-muted">等待设备连接。</p>}
      {devices.map(entry => <div className="studio-data-row" key={entry.id}>
        <strong>{entry.name}</strong>
        <span className="studio-muted">{entry.origin === 'virtual' ? '虚拟手柄' : entry.gamepad ? '标准手柄' : '原始接口'} · {entry.gyro ? '支持体感' : '无体感'} · {entry.accelerometer ? '加速度可用' : '无加速度'}</span>
        <code>{entry.path}</code>
        {entry.open_error && <span className="studio-error">{entry.open_error}</span>}
      </div>)}
      <div className={styles.deviceHeader}>
        <div><h3>外部工具与 HidHide</h3><p className="studio-muted">在组件页检测驱动和管理实体手柄可见性。</p></div>
        <button type="button" className="secondary-btn" onClick={onSettings}>设置与组件</button>
      </div>
    </details>
  </div>;
}
