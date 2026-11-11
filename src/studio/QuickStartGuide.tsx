import type { Device, Profile, Status } from '../types';
import Controller from './Controller';
import { Dialog, gamepadNames } from './Controls';
import { StudioIcon, type StudioIconName } from './StudioIcon';
import styles from './QuickStartGuide.module.css';

export type GuideStep = 'input' | 'profile' | 'mapping';

type Props = {
  step: GuideStep;
  onStepChange: (step: GuideStep) => void;
  device?: Device | null;
  live?: Status;
  profile?: Profile;
  onClose: () => void;
  onFinish: () => void;
  onOpenDevices: () => void;
  onOpenProfiles: () => void;
  onOpenActions: () => void;
};

const steps: ReadonlyArray<{ id: GuideStep; title: string; icon: StudioIconName }> = [
  { id: 'input', title: '检查手柄输入', icon: 'controllerStatus' },
  { id: 'profile', title: '选择游戏配置', icon: 'profiles' },
  { id: 'mapping', title: '启动映射', icon: 'play' },
];

export function QuickStartGuide({
  step, onStepChange, device, live, profile, onClose, onFinish,
  onOpenDevices, onOpenProfiles, onOpenActions,
}: Props) {
  const index = steps.findIndex(item => item.id === step);
  const previous = steps[index - 1];
  const next = steps[index + 1];
  const selected = device?.gamepad && device.origin !== 'virtual' && !device.open_error ? device : undefined;
  const input = selected && live?.device?.path === selected.path ? live : undefined;
  const canPractice = input && !input.emitting && (input.state === 'observing' || input.state === 'navigating');
  const observeHint = profile ? '在「设备与输入」开始观察，查看实时读数。'
    : '先选择游戏配置，再到「设备与输入」开始观察。';

  return <Dialog
    title="快速上手"
    className={styles.dialog}
    onClose={onClose}
    actions={<>
      {previous && <button type="button" className="secondary-btn" onClick={() => onStepChange(previous.id)}>上一步</button>}
      <button type="button" className="primary-btn" onClick={next ? () => onStepChange(next.id) : onFinish}>
        {next ? '下一步' : '完成'}
      </button>
    </>}
  >
    <ol className={styles.steps} aria-label="上手步骤">
      {steps.map(({ id, title, icon }) => <li key={id}>
        <button type="button" aria-current={id === step ? 'step' : undefined} onClick={() => onStepChange(id)}>
          <StudioIcon name={icon} size={17} />
          <span>{title}</span>
        </button>
      </li>)}
    </ol>

    {step === 'input' && <section className={styles.content} aria-label="检查手柄输入">
      <div className={styles.readiness} role="status">
        <StudioIcon name="controllerStatus" size={21} />
        <div>
          {selected ? <>
            <strong translate="no">{selected.name}</strong>
            <span data-live={!!input}>{input ? '输入监测中' : '输入未开始'}</span>
          </> : <strong>请选择可用的实体手柄</strong>}
        </div>
      </div>
      {input ? <>
        <div className={styles.preview}><Controller device={selected} status={input} /></div>
        {canPractice && <p>按下按键或转动摇杆，查看实时反馈。</p>}
      </> : <p>{selected ? observeHint : '连接手柄后，点击重新检测。'}</p>}
      <button type="button" className="secondary-btn" onClick={onOpenDevices}>
        设备与输入<StudioIcon name="arrow" size={14} />
      </button>
    </section>}

    {step === 'profile' && <section className={styles.content} aria-label="选择游戏配置">
      <div className={styles.readiness} role="status">
        <StudioIcon name="profiles" size={21} />
        <div>
          {profile ? <>
            <strong translate="no">{profile.name}</strong>
            <span>{gamepadNames[profile.virtual_gamepad]}</span>
          </> : <strong>尚未选择配置</strong>}
        </div>
      </div>
      <p>从配置库选择配置，或新建自己的游戏配置。</p>
      <button type="button" className="secondary-btn" onClick={onOpenProfiles}>
        配置库<StudioIcon name="arrow" size={14} />
      </button>
    </section>}

    {step === 'mapping' && <section className={styles.content} aria-label="启动映射">
      <p>确认手柄和配置后，点击顶栏的「启动映射」，再进入游戏。</p>
      <div className={styles.shortcut}>
        <kbd>F12</kbd><span>停止映射</span>
      </div>
      <p>需要调整按键或摇杆时，打开游戏配置。</p>
      <button type="button" className="secondary-btn" onClick={onOpenActions}>
        游戏配置<StudioIcon name="arrow" size={14} />
      </button>
    </section>}
  </Dialog>;
}
