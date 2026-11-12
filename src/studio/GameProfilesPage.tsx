import {useId, useState, type ReactNode} from 'react';
import {ActionMappings, type ActionMappingsProps} from './ActionMappings';
import {FeedbackSettings, TouchpadSettings} from './PeripheralSettings';
import {Dialog} from './Controls';
import type {GamepadTarget} from '../types';
import {StickSettings} from './StickSettings';
import {TriggerSettings} from './TriggerSettings';
import {gameTabs, type GameTab} from './game-tabs';
import type {StudioPage} from './StudioShell';
import styles from './GameProfilesPage.module.css';
import {StudioIcon} from './StudioIcon';

type Props = Omit<ActionMappingsProps, 'touchOnly' | 'onClearTouch'> & {
  tab: GameTab;
  layerMode: boolean;
  editingLayerKey: string;
  layerControls?: ReactNode;
  motion: ReactNode;
  onTab: (tab: GameTab) => void;
  onEditBase: () => void;
  onImportJsm: () => void;
  onExport: () => void;
  onOpenFolder: () => void;
  onNavigate: (page: StudioPage) => void;
};

export function GameProfilesPage(props: Props) {
  const {profile, status, device, blocked, capture, onUpdate, tab, onTab, layerMode} = props;
  const [touchOnly, setTouchOnly] = useState(false);
  const [feedbackOpen, setFeedbackOpen] = useState(false);
  const outputModeId = useId();
  const measuring = !!status?.motion?.measuring;
  const virtual = profile.virtual_gamepad !== 'none';
  const conflicts = !virtual && profile.gyro.enabled && profile.gyro.output === 'right_stick';

  const selectTab = (next: GameTab) => {
    setTouchOnly(false);
    onTab(next);
  };

  return <div className={styles.page}>
    <section className={styles.profileHeader} aria-label="游戏配置与输出">
      <div className={styles.gameHeader}>
        <label className={styles.gameInfo}>
          <span>配置名称</span>
          <input className={styles.gameName} aria-label="配置名称" maxLength={100} value={profile.name}
            disabled={blocked || layerMode || !!capture || measuring}
            onChange={event=>onUpdate(draft=>{draft.name=event.target.value;})}/>
        </label>
        <div className={styles.gameOutput}>
          <label htmlFor={outputModeId}>输出方式</label>
          <select id={outputModeId} className={styles.outputSelect} value={profile.virtual_gamepad}
            disabled={blocked || layerMode || !!capture || measuring}
            onChange={event=>{
              const target = event.currentTarget.value as GamepadTarget;
              onUpdate(draft=>{
                draft.virtual_gamepad=target;
                if(target!=='none') draft.gamepad_bindings ??= [];
              });
            }}>
            <option value="none">键盘与鼠标</option>
            <option value="xbox_360">虚拟 Xbox 360</option>
            <option value="dualshock_4">虚拟 DualShock 4（PS4）</option>
          </select>
        </div>
        <details className={styles.profileTools}>
          <summary><StudioIcon name="more" size={16}/><span>配置工具</span></summary>
          <div className={styles.toolsMenu} onClick={event=>{
            if ((event.target as Element).closest('button')) event.currentTarget.closest('details')?.removeAttribute('open');
          }}>
            <button type="button" disabled={blocked || !!capture || measuring} onClick={()=>setFeedbackOpen(true)}>震动与灯光</button>
            <button type="button" disabled={blocked || measuring} onClick={props.onImportJsm}>导入 JSM 配置</button>
            <button type="button" disabled={blocked || measuring} onClick={props.onExport}>导出配置</button>
            <button type="button" disabled={measuring} onClick={()=>props.onNavigate('profiles')}>配置库</button>
            <button type="button" onClick={props.onOpenFolder}>配置文件夹</button>
          </div>
        </details>
      </div>
      <div className={styles.profileContext}>
        <p>{virtual ? '在动作映射中编辑虚拟手柄按键，可填入标准布局。' : '用手柄操作游戏中的键盘与鼠标动作。'}</p>
        {props.layerControls && <div className={styles.layerTools}>{props.layerControls}</div>}
      </div>
      {feedbackOpen&&<Dialog title="游戏震动与灯光" onClose={()=>setFeedbackOpen(false)}
        actions={<button type="button" className="primary-btn" onClick={()=>setFeedbackOpen(false)}>完成</button>}>
        <fieldset disabled={blocked || !!capture || measuring} className="studio-content">
          <FeedbackSettings profile={profile} device={device} status={status} update={onUpdate}/>
        </fieldset>
      </Dialog>}
      {conflicts && <div className={styles.warning}>当前体感使用虚拟右摇杆。请为游戏配置选择虚拟手柄，或在「体感」改用鼠标输出。</div>}
    </section>

    <nav className={styles.configTabs} aria-label="游戏配置分类" data-pad-zone="game-tabs">
      {gameTabs.map(item=><button key={item.id} type="button" aria-current={tab===item.id?'page':undefined}
        disabled={blocked || (measuring && tab!==item.id)} data-cancel-capture
        onClick={()=>selectTab(item.id)}>
        <StudioIcon name={item.icon} size={16}/><span>{item.label}</span>
      </button>)}
    </nav>
    <div key={props.editingLayerKey} data-pad-zone="game-panel" className={styles.tabPanel}>
      {tab==='actions' ? <ActionMappings profile={profile} device={device} status={status} blocked={blocked}
        gamepadRecording={props.gamepadRecording} canRecordGamepad={props.canRecordGamepad} onRecordGamepad={props.onRecordGamepad}
        capture={capture} onCapture={props.onCapture} onCancelCapture={props.onCancelCapture} onAssignEscape={props.onAssignEscape}
        onUpdate={onUpdate} onEdit={props.onEdit} onBatch={props.onBatch} touchOnly={touchOnly} onClearTouch={()=>setTouchOnly(false)}/> :
        <fieldset className="studio-content" disabled={blocked || !!capture}>
          {tab==='sticks' && <StickSettings profile={profile} update={onUpdate}/>}
          {tab==='triggers' && <TriggerSettings profile={profile} layerMode={layerMode} update={onUpdate} onEditBase={props.onEditBase}/>}
          {tab==='touch' && <section className="app-card">
            <h2>触控设置</h2>
            <TouchpadSettings profile={profile} device={device} status={status} update={onUpdate}/>
            <div className="studio-actions">
              <button type="button" className="secondary-btn" onClick={()=>{setTouchOnly(true);onTab('actions');}}>查看触控绑定</button>
            </div>
            {profile.virtual_gamepad==='dualshock_4' && <p className={styles.hint}>虚拟 DualShock 4 的触摸板按键可在动作映射中绑定。</p>}
          </section>}
          {tab==='motion' && props.motion}
        </fieldset>}
    </div>
  </div>;
}
