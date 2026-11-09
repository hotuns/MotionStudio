import { buttonNames, labels, type Device, type Profile, type Status } from '../types';
import Controller from './Controller';
import { StudioIcon, type StudioIconName } from './StudioIcon';
import type { StudioPage } from './StudioShell';
import type { GameTab } from './game-tabs';
import styles from './StudioOverview.module.css';

type Props = {
  profile?: Profile;
  device?: Device | null;
  status?: Status;
  deviceCount: number;
  onNavigate: (page: StudioPage, tab?: GameTab) => void;
};

const quickActions: {page: StudioPage; tab?: GameTab; icon: StudioIconName; title: string; description: string}[] = [
  {page:'games', tab:'actions', icon:'games', title:'为游戏配置操作', description:'按游戏动作配置手柄'},
  {page:'games', tab:'motion', icon:'gyro', title:'调校体感', description:'调整方向、灵敏度和防抖'},
  {page:'profiles', icon:'profiles', title:'管理配置', description:'导入、切换和恢复游戏配置'},
];

const guide: {page: StudioPage; tab?: GameTab; title: string; description: string}[] = [
  {page:'controllerStatus', title:'确认输入', description:'在设备页检查轴向和按键。'},
  {page:'games', tab:'motion', title:'调整响应', description:'先设置基础灵敏度，再增加防抖。'},
  {page:'profiles', title:'保存与测试', description:'应用修改，在游戏中验证手感。'},
];

export function StudioOverview({ profile, device, status, deviceCount, onNavigate }: Props) {
  const live = status?.device?.path === device?.path ? status : undefined;
  const pressed = buttonNames.filter((_, i) => live?.buttons[i]);
  const gyro = profile?.gyro;
  const output = profile?.virtual_gamepad === 'dualshock_4' ? 'DualShock 4' : profile?.virtual_gamepad === 'xbox_360' ? 'Xbox 360' : '键盘与鼠标';

  return (
    <div className={styles.overview}>
      <div className={styles.metrics}>
        <div><span>设备与输入</span><strong>{String(deviceCount).padStart(2, '0')}<small>{deviceCount ? '设备已连接' : '等待手柄连接'}</small></strong></div>
        <div title="收到的体感事件频率"><span>体感事件</span><strong>{live?.stats.gyro_hz ? Math.round(live.stats.gyro_hz) : '—'}<small>Hz</small></strong></div>
        <div><span>当前配置</span><strong className={styles.profileMetric} translate="no">{profile?.name ?? '—'}</strong></div>
        <div><span>映射输出</span><strong className={styles.stateMetric}><i className={status?.emitting ? styles.online : undefined} />{status?.emitting ? '输出已启用' : '输出已暂停'}</strong></div>
      </div>
      <section className={styles.quickActions} aria-label="快速开始">
        {quickActions.map(({page, tab, icon, title, description}) => (
          <button type="button" key={title} onClick={() => onNavigate(page, tab)}>
            <span className={styles.actionIcon}><StudioIcon name={icon} size={23} /></span>
            <span><strong>{title}</strong><small>{description}</small></span><StudioIcon name="arrow" size={18} />
          </button>
        ))}
      </section>
      <div className={styles.dashboard}>
        <section className={styles.liveCard}>
          <div className={styles.cardHeading}>
            <div><span className={styles.eyebrow}>INPUT MONITOR</span><h2>实时输入</h2></div>
            <button type="button" className={styles.textButton} onClick={() => onNavigate('controllerStatus')}>查看全部输入<StudioIcon name="arrow" size={16} /></button>
          </div>
          {device ? <>
            <div className={styles.deviceLabel}><span className={styles.onlineDot} /><strong translate="no">{device.name}</strong><span className={styles.deviceId}>#{device.id}</span></div>
            <div className={styles.controllerStage}><Controller device={device} status={live} /></div>
            <div className={styles.inputReadout}>
              {['X', 'Y', 'Z'].map((axis, i) => <div key={axis}><span>陀螺仪 {axis}</span><strong>{live?.gyro[i].toFixed(1) ?? '—'}</strong></div>)}
              <small>°/s</small>
            </div>
            <div className={styles.pressedRow}>{pressed.length
              ? pressed.slice(0, 10).map(button => <span key={button}>{labels[button] ?? button}</span>)
              : <p>{live ? '按下手柄按键或转动摇杆，预览会实时响应。' : '在「设备与输入」开始观察，查看实时读数。'}</p>}
              {pressed.length > 10 && <span>+{pressed.length - 10}</span>}
            </div>
          </> : <div className={styles.emptyState}>
            <span className={styles.emptyIcon}><StudioIcon name="controllerStatus" size={72} /></span>
            <h3>{deviceCount ? '选择映射使用的手柄' : '等待手柄连接'}</h3>
            <p>{deviceCount ? '在设备页选择手柄接口并检查输入。' : '连接手柄或 2.4G 接收器，然后检查设备与输入。'}</p>
            <button type="button" className="primary-btn" onClick={() => onNavigate('controllerStatus')}>设备与输入<StudioIcon name="arrow" size={16} /></button>
          </div>}
        </section>
        <div className={styles.rightColumn}>
          <section className={styles.sessionCard}>
            <span className={styles.eyebrow}>PROFILE SNAPSHOT</span><h2>当前工作区</h2>
            <div className={styles.sessionProfile}><StudioIcon name="profiles" /><span translate="no">{profile?.name ?? '选择配置'}</span></div>
            <dl>
              <div><dt>水平 / 垂直灵敏度</dt><dd>{gyro?.enabled ? gyro.sensitivity.toFixed(1) + ' / ' + (gyro.sensitivity * gyro.vertical_ratio).toFixed(1) : '—'}</dd></div>
              <div><dt>平滑时间</dt><dd>{gyro?.enabled && gyro.smoothing_ms > 0 ? gyro.smoothing_ms + ' ms' : '关闭'}</dd></div>
              <div><dt>输出方式</dt><dd>{profile ? output : '—'}</dd></div>
            </dl>
            <button type="button" className={styles.textButton} onClick={() => onNavigate('profiles')}>管理配置<StudioIcon name="arrow" size={16} /></button>
          </section>
          <section className={styles.guideCard}>
            <h2>调校顺序</h2>
            <ol>{guide.map(({page, tab, title, description}, i) => (
              <li key={title}><span>{i + 1}</span><button type="button" onClick={() => onNavigate(page, tab)}><strong>{title}</strong><small>{description}</small></button></li>
            ))}</ol>
          </section>
        </div>
      </div>
    </div>
  );
}
