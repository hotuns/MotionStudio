import {useState, type ReactNode} from 'react';
import {ActionMappings, type ActionMappingsProps} from './ActionMappings';
import {FeedbackSettings, TouchpadSettings} from './PeripheralSettings';
import {Dialog} from './Controls';
import {OptionPicker} from './OptionPicker';
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
  motion: ReactNode;
  onTab: (tab: GameTab) => void;
  onEditBase: () => void;
  onImportJsm: () => void;
  onExport: () => void;
  onOpenFolder: () => void;
  onNavigate: (page: StudioPage) => void;
};

const outputChoices = [{options:[
  {value:'none', label:'键盘与鼠标'},
  {value:'xbox_360', label:'虚拟 Xbox 360'},
  {value:'dualshock_4', label:'虚拟 DualShock 4（PS4）'},
]}];

export function GameProfilesPage(props: Props) {
  const {profile, status, device, blocked, capture, onUpdate, tab, onTab, layerMode} = props;
  const [touchOnly, setTouchOnly] = useState(false);
  const [feedbackOpen, setFeedbackOpen] = useState(false);
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
        <div className={styles.gameMark}>{Array.from(profile.name.trim()).slice(0, 2).join('') || '＋'}</div>
        <div className="studio-game-info">
          <small>当前游戏配置</small>
          <input className="studio-game-name" aria-label="配置名称" maxLength={100} value={profile.name}
            disabled={blocked || layerMode || !!capture || measuring}
            onChange={event=>onUpdate(draft=>{draft.name=event.target.value;})}/>
          <p>{virtual ? '标准手柄按键和摇杆自动映射，可添加键鼠动作。' : '用手柄操作游戏中的键盘与鼠标动作。'}</p>
        </div>
        <div className="studio-game-output"><span>输出方式</span>
          <div className={styles.actionTools}>
            <OptionPicker label="输出方式" value={profile.virtual_gamepad} groups={outputChoices}
              disabled={blocked || layerMode || !!capture || measuring}
              onChange={target=>onUpdate(draft=>{draft.virtual_gamepad=target as GamepadTarget;})}/>
            <button type="button" className="secondary-btn" disabled={blocked || !!capture || measuring}
              onClick={()=>setFeedbackOpen(true)}>游戏震动与灯光</button>
          </div>
        </div>
      </div>
      <div className={styles.profileTools}>
        <button type="button" className="secondary-btn" disabled={blocked || measuring} onClick={props.onImportJsm}>导入 JSM 配置</button>
        <button type="button" className="secondary-btn" disabled={blocked || measuring} onClick={props.onExport}>导出配置</button>
        <button type="button" className="secondary-btn" disabled={measuring} onClick={()=>props.onNavigate('profiles')}>配置库</button>
        <button type="button" className="secondary-btn" onClick={props.onOpenFolder}>配置文件夹</button>
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
    <div data-pad-zone="game-panel" className={styles.tabPanel}>
      {tab==='actions' ? <ActionMappings profile={profile} device={device} status={status} blocked={blocked}
        capture={capture} onCapture={props.onCapture} onCancelCapture={props.onCancelCapture} onAssignEscape={props.onAssignEscape}
        onUpdate={onUpdate} onEdit={props.onEdit} touchOnly={touchOnly} onClearTouch={()=>setTouchOnly(false)}/> :
        <fieldset className="studio-content" disabled={blocked || !!capture}>
          {tab==='sticks' && <StickSettings profile={profile} update={onUpdate}/>}
          {tab==='triggers' && <TriggerSettings profile={profile} layerMode={layerMode} update={onUpdate} onEditBase={props.onEditBase}/>}
          {tab==='touch' && <section className="app-card">
            <h2>触控设置</h2>
            <TouchpadSettings profile={profile} device={device} status={status} update={onUpdate}/>
            <div className="studio-actions">
              <button type="button" className="secondary-btn" onClick={()=>{setTouchOnly(true);onTab('actions');}}>查看触控绑定</button>
            </div>
            {profile.virtual_gamepad==='dualshock_4' && <p className={styles.hint}>虚拟 DualShock 4 会自动映射实体手柄的触摸板按键。</p>}
          </section>}
          {tab==='motion' && props.motion}
        </fieldset>}
    </div>
  </div>;
}
