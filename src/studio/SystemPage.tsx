import {useState} from 'react';
import type { ComponentStatus, OutputBackend, GamepadBackend, Snapshot } from '../types';
import { gamepadNames } from './Controls';
import {VirtualOutputStatus} from './VirtualOutputStatus';
import styles from './SettingsPage.module.css';

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

export function SystemPage(props: Props) {
  const { components, snapshot, busy } = props;
  const visibility = snapshot?.status.visibility;
  const [showOutput,setShowOutput]=useState(false);
  const automaticBusy = !!visibility?.requested || !!visibility?.active;
  return <div className={styles.page}>
    <section className={styles.section}>
      <div className={styles.heading}><div><h2>输出后端</h2><p>此设置保存在本机，所有游戏配置共用。</p></div></div>
      <label className={styles.field}>键鼠输出后端
        <select aria-label="输出后端" value={props.backend} disabled={busy || props.outputActive}
          onChange={event => props.onBackend(event.target.value as OutputBackend)}>
          <option value="hid_maestro">HIDMaestro · FlexInput 原生 HID 输出</option>
          <option value="legacy">SendInput / ViGEm · 兼容模式</option>
        </select>
      </label>
      <p>组合键、双向滚轮、鼠标侧键和体感鼠标使用键鼠后端。虚拟手柄可单独选择 FlexInput 原生输出实现。</p>
      <label className={styles.field}>虚拟手柄后端
        <select aria-label="虚拟手柄后端" value={props.backend==='legacy'?'vigem':props.gamepadBackend} disabled={busy||props.outputActive||props.backend==='legacy'} onChange={event=>props.onGamepadBackend(event.target.value as GamepadBackend)}>
          <option value="vigem">ViGEm · FlexInput 原生 Xbox／DS4（推荐）</option>
          <option value="hid_maestro">HIDMaestro · FlexInput 原生 HID 手柄</option>
        </select>
      </label>
      <p>ViGEm 提供标准 Xbox／DS4 虚拟手柄，需安装 ViGEmBus。可以同时使用 HIDMaestro 键鼠输出。</p>
      <p>先安装下方驱动。首次启用映射时，Windows 会请求管理员授权来创建虚拟设备；同一引擎会话中的暂停和配置切换会复用辅助进程。</p>
      {props.outputActive && <p className={styles.notice}>停止映射或观察后可以切换输出后端。</p>}
      <p>当前键鼠输出：{snapshot?.status.emitting ? snapshot.status.output_backend === 'hid_maestro' ? 'HIDMaestro' : snapshot.status.output_backend==='legacy'?'SendInput':'状态未知' : '未发送输入'}。所选后端不可用时会暂停并报告原因。</p>
      <p>当前虚拟手柄：{snapshot?.status.output_gamepad?<span translate="no">{gamepadNames[snapshot.status.output_gamepad.target]} · {snapshot.status.output_gamepad.backend==='vigem'?'ViGEm':'HIDMaestro'}</span>:'当前没有运行中的虚拟手柄输出'}</p>
      <button type="button" className="secondary-btn" onClick={props.onGameConfig}>设置当前游戏的输出</button>
      <p>游戏是否接受虚拟设备仍取决于游戏本身，HID 输出不能保证通过反作弊检查。</p>
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
          <div className={styles.heading}><h3>{kind === 'hidmaestro' ? 'HIDMaestro' : hidhide ? 'HidHide' : 'ViGEmBus'}</h3>
            <span className={styles.badge} data-ready={state?.status === 'running'}>{state ? driverNames[state.status] ?? state.status : '等待检测'}</span></div>
          <p>{kind === 'hidmaestro' ? 'FlexInput 使用的虚拟 HID 驱动。支持键盘、鼠标、Xbox 360 和 DualShock 4。安装时会在本机生成专用签名证书，并由 Windows 请求管理员授权。' : hidhide ? '向游戏隐藏实体手柄，减少重复输入。' : 'FlexInput 原生 ViGEm 手柄后端需要此驱动，可与 HIDMaestro 键鼠同时使用。'}</p>
          <div className={styles.actions}>
            <button type="button" className="primary-btn" disabled={busy || props.outputActive || state?.status !== 'missing'} onClick={() => props.onInstall(kind)}>
              {state?.status === 'missing' ? '安装内置组件' : '安装状态已检测'}
            </button>
            <button type="button" className="secondary-btn" onClick={() => hidhide?props.onOpen('hidhide'):setShowOutput(true)}>
              {hidhide ? '官方管理工具' : '检查虚拟手柄'}
            </button>
          </div>
          {state?.error && <p className={styles.error}>{state.error}</p>}
        </div>;
      })}</div>
    </section>
    <section className={styles.section}>
      <div className={styles.heading}>
        <div><h2>手柄隐藏</h2><p>管理由 Motion Studio 添加的 HidHide 规则。</p></div>
        <button type="button" className="secondary-btn" disabled={busy || automaticBusy || !components?.controlAvailable} onClick={() => props.onComponent('allow')}>
          {components?.engineAllowed ? '重新确认引擎权限' : '允许引擎读取手柄'}
        </button>
      </div>
      <label className={styles.field}>
        <span><input type="checkbox" aria-label="映射时自动隐藏实体手柄" checked={props.autoHide}
          disabled={busy || props.outputActive} onChange={event => props.onAutoHide(event.target.checked)} /> 映射时自动隐藏实体手柄</span>
      </label>
      <p>开始映射时隐藏所选实体手柄及可确认的 XInput 伴随接口，保留虚拟输出可见。断线等待和配置切换期间保持隐藏；停止、F12 或退出后恢复本次新增的规则。</p>
      {props.autoHide && components?.hidhide.status === 'missing' && <p className={styles.notice}>请先安装 HidHide；自动隐藏未就绪时不会开始输出。也可以停止后关闭此设置，自行管理设备隐藏。</p>}
      {visibility?.active && <p role="status">实体手柄已隐藏 · 映射会话管理中</p>}
      {visibility?.error && <p className={styles.error} role="alert">{visibility.error}</p>}
      {automaticBusy && <p className={styles.notice}>自动隐藏会话运行期间，请先停止映射，再手动修改隐藏规则。</p>}
      {components?.error && <p className={styles.error}>{components.error}</p>}
      {components?.inverse && <p className={styles.notice}>HidHide 正在使用反向应用列表，可在官方工具中调整。</p>}
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
        <button type="button" className="secondary-btn" onClick={props.onDevices}>查看设备与输入</button>
      </div>
    </section>
    <section className={styles.section}>
      <div className={styles.heading}>
        <div><h2>运行与恢复</h2><p>引擎异常退出后会恢复到待机。</p></div>
        <span className={styles.badge} data-ready={!!snapshot?.engine_ready}>{snapshot?.engine_ready ? '引擎已连接' : '引擎离线'}</span>
      </div>
      <div className={styles.actions}>
        <button type="button" className="secondary-btn" disabled={busy} onClick={props.onRecover}>重启引擎</button>
        <button type="button" className="secondary-btn" disabled={busy} onClick={props.onDiagnostics}>导出诊断</button>
        <button type="button" className="secondary-btn" onClick={() => props.onOpen('data')}>打开数据目录</button>
        <button type="button" className="secondary-btn" disabled={busy} onClick={props.onRestore}>恢复设置备份</button>
      </div>
      <p>自动恢复次数 {snapshot?.restart_count ?? 0}</p>
      <code className={styles.path}>{props.dataDir}</code>
      <details><summary>最近诊断记录</summary><pre>{snapshot?.logs.join('\n') || '暂无错误记录。'}</pre></details>
    </section>
    {showOutput&&<VirtualOutputStatus status={snapshot?.status} onClose={()=>setShowOutput(false)} onOpenLegacy={()=>props.onOpen('gamepads')}/>}
  </div>;
}
