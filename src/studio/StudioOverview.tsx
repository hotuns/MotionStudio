import { useRef, useState } from 'react';
import { buttonNames, labels, type Device, type Profile, type Status } from '../types';
import Controller from './Controller';
import { QuickStartGuide, type GuideStep } from './QuickStartGuide';
import { StudioIcon, type StudioIconName } from './StudioIcon';
import type { StudioPage } from './StudioShell';
import type { GameTab } from './game-tabs';
import styles from './StudioOverview.module.css';

type Props = {
  profile?: Profile;
  device?: Device | null;
  status?: Status;
  deviceCount: number;
  guideStep: GuideStep;
  onGuideStepChange: (step: GuideStep) => void;
  onNavigate: (page: StudioPage, tab?: GameTab) => void;
};

const tools: {page: StudioPage; tab?: GameTab; icon: StudioIconName; title: string; description: string}[] = [
  {page:'games', tab:'actions', icon:'keybinds', title:'游戏配置', description:'设置按键、摇杆和游戏动作'},
  {page:'games', tab:'motion', icon:'gyro', title:'调校体感', description:'调整方向、灵敏度和防抖'},
  {page:'controllerStatus', icon:'controllerStatus', title:'设备与输入', description:'查看输入与校准'},
  {page:'profiles', icon:'profiles', title:'配置库', description:'管理、导入游戏配置'},
  {page:'switching', icon:'refresh', title:'自动切换', description:'设置配置切换规则'},
  {page:'settings', icon:'settings', title:'设置与组件', description:'软件偏好、驱动与输出'},
];

export function StudioOverview({ profile, device, status, deviceCount, guideStep, onGuideStepChange, onNavigate }: Props) {
  const [guideOpen, setGuideOpen] = useState(false);
  const guideButton = useRef<HTMLButtonElement>(null);
  const usableDevice = device?.gamepad && device.origin !== 'virtual' && !device.open_error;
  const reading = !!status && ['running', 'observing', 'navigating', 'calibrating', 'capturing', 'capture_release'].includes(status.state);
  const live = usableDevice && reading && status?.device?.path === device?.path ? status : undefined;
  const sensorLive = live && device?.gyro && !live.ui_navigation ? live : undefined;
  const session = !!status && !status.ui_navigation &&
    ['running', 'observing', 'waiting', 'preparing_output', 'calibrating', 'capturing', 'capture_release'].includes(status.state);
  const pressed = buttonNames.filter((_, i) => live?.buttons[i]);
  const gyro = profile?.gyro;
  const output = profile?.virtual_gamepad === 'dualshock_4' ? 'DualShock 4'
    : profile?.virtual_gamepad === 'xbox_360' ? 'Xbox 360' : '键盘与鼠标';
  const gyroMode = !gyro ? '—' : !gyro.enabled ? '关闭'
    : gyro.motion.mode === 'steering' ? '姿态控制摇杆'
    : gyro.motion.mode === 'stick_rotation' ? '体感旋转摇杆方向'
    : gyro.output === 'right_stick' ? '右摇杆瞄准' : '鼠标瞄准';

  function closeGuide() {
    setGuideOpen(false);
    requestAnimationFrame(() => {
      if (!document.querySelector('dialog[open]')) guideButton.current?.focus();
    });
  }

  function openGuideDestination(page: StudioPage, tab?: GameTab) {
    closeGuide();
    onNavigate(page, tab);
  }

  return <div className={styles.overview}>
    <div className={styles.workspace}>
      <section className={styles.inputPanel} aria-label="手柄输入">
        <div className={styles.panelHeading}>
          <h2>手柄输入</h2>
          <button type="button" className={styles.textButton} onClick={() => onNavigate('controllerStatus')}>管理设备<StudioIcon name="arrow" size={14} /></button>
        </div>
        {device ? <>
          <div className={styles.deviceHeading}>
            <h3 translate="no">{device.name}</h3>
            <span className={styles.deviceState} data-live={!!live} data-error={!!device.open_error}>
              <i aria-hidden="true" />
              {device.open_error ? '接口不可用' : device.origin === 'virtual' ? '虚拟设备'
                : live?.ui_navigation ? '手柄操作中' : live ? '输入监测中' : '输入未开始'}
            </span>
          </div>
          {device.open_error && <p className={styles.error}>{device.open_error}</p>}
          <div className={styles.controllerStage}><Controller device={device} status={live} /></div>
          <div className={styles.pressedRow}>
            {pressed.length ? <>
              {pressed.slice(0, 8).map(button => <span key={button}>{labels[button] ?? button}</span>)}
              {pressed.length > 8 && <span>+{pressed.length - 8}</span>}
            </> : <p>{live ? '按下按键或转动摇杆，查看实时反馈。'
              : '在「设备与输入」开始观察，查看实时读数。'}</p>}
          </div>
          <details className={styles.telemetry}>
            <summary>查看采样数据</summary>
            {live?.ui_navigation ? <p className={styles.sensorHint}>手柄操作模式仅预览按键和摇杆。</p> : <dl className={styles.readout}>
              <div><dt>体感事件</dt><dd>{sensorLive?.stats.gyro_hz ? Math.round(sensorLive.stats.gyro_hz) : '—'}<small>Hz</small></dd></div>
              {['X', 'Y', 'Z'].map((axis, i) => <div key={axis}>
                <dt>陀螺仪 {axis}</dt><dd>{sensorLive?.gyro[i].toFixed(1) ?? '—'}<small>°/s</small></dd>
              </div>)}
            </dl>}
          </details>
        </> : <div className={styles.emptyState}>
          <div className={styles.emptyIcon}><StudioIcon name="controllerStatus" size={40} /></div>
          <h3>{deviceCount ? '选择映射使用的手柄' : '等待手柄连接'}</h3>
          <p>{deviceCount ? '在设备页选择手柄接口并检查输入。'
            : '连接手柄或 2.4G 接收器，然后检查设备与输入。'}</p>
          <button type="button" className="secondary-btn" onClick={() => onNavigate('controllerStatus')}>设备与输入</button>
        </div>}
      </section>

      <div className={styles.configArea}>
        <section className={styles.profileSummary} aria-label="编辑中的配置">
          <div className={styles.profileTopline}>
            <span className={styles.eyebrow}>当前配置</span>
            <button ref={guideButton} type="button" className="secondary-btn" aria-haspopup="dialog"
              aria-expanded={guideOpen} onClick={() => setGuideOpen(true)}>
              <StudioIcon name="help" size={16} />快速上手
            </button>
          </div>
          <h2 translate={profile ? 'no' : undefined}>{profile?.name ?? '选择游戏配置'}</h2>
          {profile ? <dl className={styles.profileValues}>
            <div><dt><StudioIcon name={profile.virtual_gamepad === 'none' ? 'keybinds' : 'games'} size={14} /><span>输出方式</span></dt><dd>{output}</dd></div>
            <div><dt><StudioIcon name="gyro" size={14} /><span>体感控制</span></dt><dd>{gyroMode}</dd></div>
          </dl> : <p className={styles.profileHint}>从配置库选择配置，或新建自己的游戏配置。</p>}
          {session && status?.profile_name && <p className={styles.sessionNote}>
            <span>会话配置</span><strong translate="no">{status.profile_name}</strong>
          </p>}
        </section>

        <nav aria-label="工作台" className={styles.toolsPanel}>
          <div className={styles.primaryTools}>
            {tools.slice(0, 2).map(({page, tab, icon, title, description}) => <button
              type="button" key={title} className={styles.primaryTool} onClick={() => onNavigate(page, tab)}>
              <span className={styles.primaryIcon}><StudioIcon name={icon} size={23} /></span>
              <StudioIcon name="arrow" size={17} />
              <span className={styles.toolCopy}><strong>{title}</strong><small>{description}</small></span>
            </button>)}
          </div>
          <div className={styles.secondaryTools}>
            {tools.slice(2).map(({page, tab, icon, title, description}) => <button
              type="button" key={title} className={styles.secondaryTool} onClick={() => onNavigate(page, tab)}>
              <span className={styles.secondaryIcon}><StudioIcon name={icon} size={20} /></span>
              <span className={styles.toolCopy}><strong>{title}</strong><small>{description}</small></span>
              <StudioIcon name="arrow" size={14} />
            </button>)}
          </div>
        </nav>

        {gyro?.enabled && <details className={styles.profileDetails}>
          <summary>调校参数</summary>
          <dl className={styles.tuningValues}>
            <div><dt>水平 / 垂直灵敏度</dt><dd>{gyro.sensitivity.toFixed(1)} / {(gyro.sensitivity * gyro.vertical_ratio).toFixed(1)}</dd></div>
            <div><dt>平滑时间</dt><dd>{gyro.smoothing_ms > 0 ? gyro.smoothing_ms + ' ms' : '关闭'}</dd></div>
          </dl>
        </details>}
      </div>
    </div>
    <p className={styles.purpose}>将手柄操作转换为键盘、鼠标或虚拟手柄输入，支持体感瞄准。</p>
    {guideOpen && <QuickStartGuide step={guideStep} onStepChange={onGuideStepChange}
      device={device} live={live} profile={profile} onClose={closeGuide}
      onFinish={() => { onGuideStepChange('input'); closeGuide(); }}
      onOpenDevices={() => openGuideDestination('controllerStatus')}
      onOpenProfiles={() => openGuideDestination('profiles')}
      onOpenActions={() => openGuideDestination('games', 'actions')} />}
  </div>;
}
