import {useId, useState, type ReactNode} from 'react';
import type { ComponentStatus, OutputBackend, GamepadBackend, IoRates, Snapshot } from '../types';
import {t} from '../i18n';
import { gamepadNames } from './Controls';
import {VirtualOutputStatus} from './VirtualOutputStatus';
import styles from './SettingsPage.module.css';
import {SettingField, SettingLabel} from './SettingLayout';

const driverNames: Record<string, string> = {
  running: '已就绪',
  missing: '未安装',
  stopped: '已安装 · 待启动',
  unknown: '状态未知',
  unsupported: '不支持',
  partial: '安装不完整',
  incompatible: '版本不匹配',
};

export type ComponentAction = 'allow' | 'hide' | 'show' | 'restore';
type Props = {
  softwareUpdate: ReactNode;
  components: ComponentStatus | null;
  snapshot: Snapshot | null;
  dataDir: string;
  busy: boolean;
  onRefresh: () => void;
  onInstall: (kind: 'hidhide' | 'vigem' | 'hidmaestro') => void;
  backend: OutputBackend;
  outputActive: boolean;
  onBackend: (backend: OutputBackend) => void;
  gamepadBackend: GamepadBackend;
  onGamepadBackend: (backend:GamepadBackend)=>void;
  mouseDisplaySync: boolean;
  onMouseDisplaySync: (enabled: boolean) => void;
  ioRates: IoRates;
  onIoRates: (rates: IoRates) => void;
  autoHide: boolean;
  onAutoHide: (enabled: boolean) => void;
  onOpen: (location: string) => void;
  onComponent: (action: ComponentAction, id?: string, xinput?: boolean) => void;
  onGameConfig: () => void;
  onRecover: () => void;
  onRestore: () => void;
  onDiagnostics: () => void;
  onDevices: () => void;
};

function FrequencyField({label, value, disabled, onChange}: {
  label: string;
  value: number;
  disabled: boolean;
  onChange: (rate: number) => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const id=useId();
  return <SettingField label={label}><div className={styles.field}>
    <SettingLabel label={label} htmlFor={id} unit="Hz"/>
    <input id={id} aria-label={t(label)+' (Hz)'} type="number" min={60} max={4000} step={1} value={draft ?? value} disabled={disabled}
      onChange={event => setDraft(event.currentTarget.value)}
      onBlur={event => {
        const rate = event.currentTarget.valueAsNumber;
        if (draft !== null && event.currentTarget.validity.valid && Number.isInteger(rate) && rate !== value) {
          onChange(rate);
        }
        setDraft(null);
      }}
      onKeyDown={event => {
        if (event.key === 'Enter') {
          event.preventDefault();
          event.currentTarget.blur();
        } else if (event.key === 'Escape') {
          setDraft(null);
        }
      }}/>
  </div></SettingField>;
}

export function SystemPage(props: Props) {
  const { components, snapshot, busy } = props;
  const visibility = snapshot?.status.visibility;
  const displaySync = snapshot?.status.mouse_display_sync;
  const [showOutput,setShowOutput]=useState(false);
  const automaticBusy = !!visibility?.requested || !!visibility?.active;
  const {ioRates, onIoRates} = props;
  const outputFollowsPolling = ioRates.output_hz === 0;
  return <div className={styles.page}>
    {props.softwareUpdate}
    {snapshot?.status.state === 'faulted' && snapshot.status.message &&
      <p className={styles.error} role="alert">{snapshot.status.message}</p>}
    {snapshot && !snapshot.engine_ready && <div className={styles.notice} role="status">
      <span>引擎离线</span>
      <button type="button" className="secondary-btn" disabled={busy} onClick={props.onRecover}>重启引擎</button>
    </div>}
    <section className={styles.section}>
      <div className={styles.heading}><h2>日常设置</h2></div>
      <div className={styles.settingRow}>
        <div>
          <label className={styles.check}>
            <input type="checkbox" aria-label="映射时自动隐藏实体手柄" checked={props.autoHide}
              disabled={busy || props.outputActive} onChange={event => props.onAutoHide(event.target.checked)}/>
            映射时自动隐藏实体手柄
          </label>
          <p>避免重复输入。停止映射后恢复本次隐藏的手柄。</p>
        </div>
        <button type="button" className="secondary-btn" onClick={props.onDevices}>查看设备与输入</button>
      </div>
      {props.autoHide && components?.hidhide.status === 'missing' && <p className={styles.notice}>请先安装 HidHide；自动隐藏未就绪时不会开始输出。也可以停止后关闭此设置，自行管理设备隐藏。</p>}
      {visibility?.active && <p role="status">实体手柄已隐藏 · 映射会话管理中</p>}
      {visibility?.error && <p className={styles.error} role="alert">{visibility.error}</p>}
      {components?.error && <p className={styles.error} role="alert">{components.error}</p>}
      {components?.inverse && <p className={styles.notice}>HidHide 正在使用反向应用列表，可在官方工具中调整。</p>}
      <div className={styles.settingRow}>
        <div><strong>游戏输出方式</strong><p>在游戏配置中选择键鼠或虚拟手柄。</p></div>
        <button type="button" className="secondary-btn" onClick={props.onGameConfig}>设置当前游戏的输出</button>
      </div>
    </section>
    <section className={styles.section}>
      <div className={styles.heading}>
        <div><h2>系统组件</h2><p>检查 HID 输出、设备隐藏和兼容模式所需的驱动。</p></div>
        <button type="button" className="secondary-btn" disabled={busy} onClick={props.onRefresh}>重新检测</button>
      </div>
      <div className={styles.drivers}>{(['hidmaestro', 'hidhide', 'vigem'] as const).map(kind => {
        const state = components?.[kind];
        const hidhide = kind === 'hidhide';
        return <div className={styles.driver} key={kind}>
          <div className={styles.driverInfo}>
            <div className={styles.driverTitle}><h3>{kind === 'hidmaestro' ? 'HIDMaestro' : hidhide ? 'HidHide' : 'ViGEmBus'}</h3>
              <span className={styles.badge} data-ready={state?.status === 'running'}>{state ? driverNames[state.status] ?? state.status : '等待检测'}</span></div>
            <p>{kind === 'hidmaestro' ? '提供虚拟键盘、鼠标、Xbox 360 和 DualShock 4。安装时会在本机生成签名证书，Windows 会请求管理员授权。' : hidhide ? '向游戏隐藏实体手柄，减少重复输入。' : '安装此驱动后可使用 ViGEm 虚拟手柄。'}</p>
            {state?.error && <p className={styles.error} role="alert">{state.error}</p>}
          </div>
          <div className={styles.actions}>
            {state?.status === 'missing' && <button type="button" className="primary-btn" disabled={busy || props.outputActive} onClick={() => props.onInstall(kind)}>安装内置组件</button>}
            <button type="button" className="secondary-btn" onClick={() => hidhide?props.onOpen('hidhide'):setShowOutput(true)}>
              {hidhide ? '官方管理工具' : '检查虚拟手柄'}
            </button>
          </div>
        </div>;
      })}</div>
      {props.outputActive && <p className={styles.hint}>停止映射或观察后可安装组件。</p>}
    </section>
    <div className={styles.advancedHeading}><h2>高级设置</h2><p>按需调整输出实现、设备隐藏和诊断工具。</p></div>
    <details className={styles.advanced}>
      <summary><strong>输出后端</strong><span>键鼠与虚拟手柄 · 显示同步</span></summary>
      <div className={styles.detailsBody}>
        <p>此设置保存在本机，所有游戏配置共用。</p>
        <SettingField label="键鼠输出后端"><div className={styles.field}><SettingLabel label="键鼠输出后端"/>
          <select aria-label="输出后端" value={props.backend} disabled={busy || props.outputActive}
            onChange={event => props.onBackend(event.target.value as OutputBackend)}>
            <option value="hid_maestro">HIDMaestro · HID 键鼠输出</option>
            <option value="legacy">SendInput / ViGEm · 兼容模式</option>
          </select>
        </div></SettingField>
        <p>组合键、滚轮、鼠标侧键和体感鼠标共用此后端。</p>
        <SettingField label="虚拟手柄后端"><div className={styles.field}><SettingLabel label="虚拟手柄后端"/>
          <select aria-label="虚拟手柄后端" value={props.backend==='legacy'?'vigem':props.gamepadBackend} disabled={busy||props.outputActive||props.backend==='legacy'} onChange={event=>props.onGamepadBackend(event.target.value as GamepadBackend)}>
            <option value="vigem">ViGEm · 虚拟 Xbox／DS4（推荐）</option>
            <option value="hid_maestro">HIDMaestro · HID 虚拟手柄</option>
          </select>
        </div></SettingField>
        <p>ViGEm 提供标准 Xbox／DS4 虚拟手柄，需安装 ViGEmBus。可以同时使用 HIDMaestro 键鼠输出。</p>
        <FrequencyField label="输入轮询频率" value={ioRates.polling_hz} disabled={busy}
          onChange={polling_hz => onIoRates({...ioRates, polling_hz})}/>
        <FrequencyField label="固定输出频率" value={outputFollowsPolling ? ioRates.polling_hz : ioRates.output_hz}
          disabled={busy || props.mouseDisplaySync || outputFollowsPolling}
          onChange={output_hz => onIoRates({...ioRates, output_hz})}/>
        <label className={styles.check}>
          <input type="checkbox" checked={outputFollowsPolling} disabled={busy || props.mouseDisplaySync}
            onChange={event => onIoRates({...ioRates, output_hz: event.currentTarget.checked ? 0 : ioRates.polling_hz})}/>
          固定输出频率跟随输入轮询
        </label>
        <p>频率可设为 60–4000 Hz，下次开始映射或观察时生效。</p>
        <label className={styles.check}>
          <input type="checkbox" checked={props.mouseDisplaySync} disabled={busy || props.outputActive}
            onChange={event => props.onMouseDisplaySync(event.target.checked)}/>
          输出跟随显示刷新
        </label>
        <p>连续鼠标位移和虚拟手柄轴值跟随前台窗口所在的显示器，每帧安排 4 个输出时隙。等待显示时钟时，按显示器的标称刷新率安排固定节拍。</p>
        {props.mouseDisplaySync && snapshot?.status.emitting && <p role="status">
          {displaySync?.live
            ? <>{t('已同步到 {0} Hz · 每帧 {1} 次').replace('{0}', String(displaySync.frame_hz)).replace('{1}', String(displaySync.reports_per_frame))}
              {displaySync.vrr && <> · <span>跟随可变刷新率</span></>}</>
            : displaySync?.fallback_hz
              ? t('等待显示同步 · 当前固定 {0} Hz').replace('{0}', String(displaySync.fallback_hz))
              : '等待显示同步'}
        </p>}
        <p>首次启用映射时，Windows 会请求管理员授权来创建虚拟设备；同一引擎会话中的暂停和配置切换会复用辅助进程。</p>
        {props.outputActive && <p className={styles.notice}>停止映射或观察后可以切换输出后端。</p>}
        <p>当前键鼠输出：{snapshot?.status.emitting ? snapshot.status.output_backend === 'hid_maestro' ? 'HIDMaestro' : snapshot.status.output_backend==='legacy'?'SendInput':'状态未知' : '未发送输入'}。所选后端不可用时会暂停并报告原因。</p>
        <p>当前虚拟手柄：{snapshot?.status.output_gamepad?<span translate="no">{gamepadNames[snapshot.status.output_gamepad.target]} · {snapshot.status.output_gamepad.backend==='vigem'?'ViGEm':'HIDMaestro'}</span>:'当前没有运行中的虚拟手柄输出'}</p>
        <p>游戏是否接受虚拟设备仍取决于游戏本身，HID 输出不能保证通过反作弊检查。</p>
      </div>
    </details>
    <details className={styles.advanced}>
      <summary><strong>手动管理隐藏规则</strong><span>HidHide 权限与设备接口</span></summary>
      <div className={styles.detailsBody}>
        <div className={styles.heading}>
          <p>管理由 Motion Studio 添加的 HidHide 规则。</p>
          <button type="button" className="secondary-btn" disabled={busy || automaticBusy || !components?.controlAvailable} onClick={() => props.onComponent('allow')}>
            {components?.engineAllowed ? '重新确认引擎权限' : '允许引擎读取手柄'}
          </button>
        </div>
        <p>开始映射时隐藏所选实体手柄及可确认的 XInput 伴随接口，保留虚拟输出可见。断线等待和配置切换期间保持隐藏；停止、F12 或退出后恢复本次新增的规则。</p>
        {automaticBusy && <p className={styles.notice}>自动隐藏会话运行期间，请先停止映射，再手动修改隐藏规则。</p>}
        {components?.devices.map(device => <div className="studio-device-rule" key={device.id}>
          <div><strong>{device.name || device.description}</strong>
            <span className="studio-muted">{device.listed ? '已在隐藏列表' : '当前可见'}{visibility?.hidden_paths.some(path=>path.toUpperCase()===device.id.toUpperCase()) ? ' · 自动会话管理' : device.managed ? ' · 本应用管理' : ''}</span></div>
          {device.managed
            ? <button type="button" className="secondary-btn" disabled={busy || automaticBusy} onClick={() => props.onComponent('show', device.id)}>恢复可见</button>
            : <button type="button" className="secondary-btn" disabled={busy || automaticBusy || device.listed || components.inverse}
                onClick={() => props.onComponent('hide', device.id, device.xinputAvailable)}>
                {device.listed ? '其他规则管理' : '隐藏游戏接口'}
              </button>}
        </div>)}
        {!components?.devices.length && <p>安装 HidHide 并连接手柄后，可以选择要隐藏的接口。</p>}
        <div className={styles.actions}>
          <button type="button" className="secondary-btn" disabled={busy || props.outputActive || !!visibility?.requested || (!components?.managedCount && !visibility?.error && !visibility?.active)} onClick={() => props.onComponent('restore')}>恢复本应用的隐藏规则</button>
        </div>
      </div>
    </details>
    <details className={styles.advanced}>
      <summary><strong>运行与恢复</strong><span>{snapshot?.engine_ready ? '引擎已连接' : '引擎离线'}</span></summary>
      <div className={styles.detailsBody}>
        <p>引擎异常退出后会恢复到待机。</p>
        <div className={styles.actions}>
          <button type="button" className="secondary-btn" disabled={busy} onClick={props.onRecover}>重启引擎</button>
          <button type="button" className="secondary-btn" disabled={busy} onClick={props.onDiagnostics}>导出诊断</button>
          <button type="button" className="secondary-btn" onClick={() => props.onOpen('data')}>打开数据目录</button>
          <button type="button" className="secondary-btn" disabled={busy} onClick={props.onRestore}>恢复设置备份</button>
        </div>
        <p>自动恢复次数 {snapshot?.restart_count ?? 0}</p>
        <code className={styles.path}>{props.dataDir}</code>
        <details><summary>最近诊断记录</summary><pre>{snapshot?.logs.join('\n') || '暂无错误记录。'}</pre></details>
      </div>
    </details>
    {showOutput&&<VirtualOutputStatus status={snapshot?.status} onClose={()=>setShowOutput(false)} onOpenLegacy={()=>props.onOpen('gamepads')}/>}
  </div>;
}
