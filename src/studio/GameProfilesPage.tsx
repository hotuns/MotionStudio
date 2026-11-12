import {useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode} from 'react';
import {ActionMappings, type ActionMappingsProps} from './ActionMappings';
import {FeedbackSettings, TouchpadSettings} from './PeripheralSettings';
import {Dialog} from './Controls';
import type {GamepadTarget,ProfileLayer} from '../types';
import {StickSettings} from './StickSettings';
import {TriggerSettings} from './TriggerSettings';
import {gameTabs, type GameTab} from './game-tabs';
import type {StudioPage} from './StudioShell';
import styles from './GameProfilesPage.module.css';
import {StudioIcon} from './StudioIcon';
import {MotionOutputSettings} from './AdvancedMotion';
import {SettingContext,settingStyles} from './SettingLayout';
import type {SettingRequest} from './settings-catalog';
import {t} from '../i18n';

type Props = Omit<ActionMappingsProps, 'touchOnly' | 'onClearTouch'> & {
  tab: GameTab;
  layerMode: boolean;
  editingLayerKey: string;
  layerControls?: ReactNode;
  motion: ReactNode;
  editingLayer?: ProfileLayer;
  settingRequest?: SettingRequest;
  onRestoreSetting: (paths: string[]) => void;
  canMeasure: boolean;
  onOpenCalibration: () => void;
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
  const [searchHint,setSearchHint]=useState<string|null>(null);
  const pageRef = useRef<HTMLDivElement>(null);
  const navigationRef = useRef<HTMLDivElement>(null);
  const outputModeId = useId();
  const measuring = !!status?.motion?.measuring;
  const virtual = profile.virtual_gamepad !== 'none';
  const conflicts = !virtual && profile.gyro.enabled && profile.gyro.output === 'right_stick';

  useEffect(()=>{
    setSearchHint(null);
    const request=props.settingRequest;
    if(!request||request.tab!==tab)return;
    let nextFrame=0,focusFrame=0,timer=0;
    let highlighted:HTMLElement|undefined;
    const firstFrame=requestAnimationFrame(()=>{
      nextFrame=requestAnimationFrame(()=>{
        const page=pageRef.current;
        if(!page)return;
        const sections=Array.from(page.querySelectorAll<HTMLElement>('[data-setting-section]'));
        const section=sections.find(el=>el.dataset.settingSection===request.section);
        const candidates=Array.from(page.querySelectorAll<HTMLElement>('[data-setting-path]'))
          .filter(el=>el.dataset.settingPath?.split(' ').includes(request.path));
        const field=candidates.find(el=>section?.contains(el)&&el.dataset.settingPath===request.path)
          ??candidates.find(el=>section?.contains(el))
          ??candidates.find(el=>el.dataset.settingPath===request.path)??candidates[0];
        const fallback=tab==='motion'?'motion-main':tab==='sticks'?'flick-main':tab==='triggers'?'trigger-stages':'touch-general';
        const target=field??section??sections.find(el=>el.dataset.settingSection===fallback);
        if(!target)return;
        for(let parent:HTMLElement|null=target;parent&&parent!==page;parent=parent.parentElement){
          if(parent instanceof HTMLDetailsElement)parent.open=true;
        }
        if(!field)setSearchHint(t('已定位「{0}」。此参数随功能或模式显示，请先检查该分组的启用条件。').replace('{0}',t(request.label)));
        focusFrame=requestAnimationFrame(()=>{
          target.scrollIntoView({block:'start'});
          const focusTarget=field
            ? target.querySelector<HTMLElement>('input[type="number"]:not(:disabled),select:not(:disabled),textarea:not(:disabled)')
              ??target.querySelector<HTMLElement>('input:not(:disabled),button:not(:disabled)')
            : target.querySelector<HTMLElement>('summary,button:not(:disabled)');
          focusTarget?.focus({preventScroll:true});
          target.dataset.settingHighlight='true';highlighted=target;
          timer=window.setTimeout(()=>delete target.dataset.settingHighlight,2400);
        });
      });
    });
    return ()=>{cancelAnimationFrame(firstFrame);cancelAnimationFrame(nextFrame);cancelAnimationFrame(focusFrame);clearTimeout(timer);if(highlighted)delete highlighted.dataset.settingHighlight;};
  },[props.settingRequest,tab]);

  useLayoutEffect(() => {
    const page = pageRef.current;
    const navigation = navigationRef.current;
    if (!page || !navigation) return;
    const updateOffset = () => {
      page.style.setProperty('--profile-navigation-height', `${navigation.offsetHeight}px`);
    };
    const observer = new ResizeObserver(updateOffset);
    updateOffset();
    observer.observe(navigation);
    return () => observer.disconnect();
  }, []);

  const selectTab = (next: GameTab) => {
    setTouchOnly(false);
    onTab(next);
  };

  return <SettingContext.Provider value={{layer:props.editingLayer,blocked:blocked||!!capture||!!status?.binding_capture||measuring,restore:props.onRestoreSetting,request:props.settingRequest}}>
  <div className={styles.page} ref={pageRef}>
    <div className={styles.profileNavigation} ref={navigationRef}>
      <section className={styles.profileHeader} aria-label="游戏配置与输出">
        <label className={styles.gameInfo}>
          <span>配置名称</span>
          <input className={styles.gameName} aria-label="配置名称" maxLength={100} value={profile.name}
            disabled={blocked || layerMode || !!capture || measuring}
            onChange={event=>onUpdate(draft=>{draft.name=event.target.value;})}/>
        </label>
        <div className={styles.gameCommands}>
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
          {props.layerControls}
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
      </section>

      <nav className={styles.configTabs} aria-label="游戏配置分类" data-pad-zone="game-tabs">
        {gameTabs.map(item=><button key={item.id} type="button" aria-current={tab===item.id?'page':undefined}
          disabled={blocked || (measuring && tab!==item.id)} data-cancel-capture
          onClick={()=>selectTab(item.id)}>
          <StudioIcon name={item.icon} size={16}/><span>{item.label}</span>
        </button>)}
      </nav>
    </div>
    {tab==='actions'&&<p className={styles.profileContext}>{virtual ? '在动作映射中编辑虚拟手柄按键，可填入标准布局。' : '用手柄操作游戏中的键盘与鼠标动作。'}</p>}
    {searchHint&&<div className={settingStyles.shared}><p className={settingStyles.notice} role="status">{searchHint}</p>
      <button type="button" className="ghost-btn" onClick={()=>setSearchHint(null)}>关闭提示</button></div>}
    {measuring&&<div className={settingStyles.shared}><p className={settingStyles.notice}>正在进行转角校准，结束或取消后可继续编辑。</p>
      <button type="button" className="secondary-btn" onClick={props.onOpenCalibration}>返回校准悬浮窗</button></div>}
    {conflicts && <div className={styles.warning}>当前体感使用虚拟右摇杆。请为游戏配置选择虚拟手柄，或在「体感」改用鼠标输出。</div>}
    {feedbackOpen&&<Dialog title="游戏震动与灯光" onClose={()=>setFeedbackOpen(false)}
      actions={<button type="button" className="primary-btn" onClick={()=>setFeedbackOpen(false)}>完成</button>}>
      <fieldset disabled={blocked || !!capture || measuring} className="studio-content">
        <FeedbackSettings profile={profile} device={device} status={status} update={onUpdate}/>
      </fieldset>
    </Dialog>}
    <div key={props.editingLayerKey} data-pad-zone="game-panel" className={styles.tabPanel}>
      {tab==='actions' ? <ActionMappings profile={profile} device={device} status={status} blocked={blocked}
        gamepadRecording={props.gamepadRecording} canRecordGamepad={props.canRecordGamepad} onRecordGamepad={props.onRecordGamepad}
        capture={capture} onCapture={props.onCapture} onCancelCapture={props.onCancelCapture} onAssignEscape={props.onAssignEscape}
        onUpdate={onUpdate} onEdit={props.onEdit} onBatch={props.onBatch} touchOnly={touchOnly} onClearTouch={()=>setTouchOnly(false)}/> :
        <fieldset className="studio-content" disabled={blocked || !!capture || measuring}>
          {(tab==='motion'||tab==='sticks')&&<MotionOutputSettings profile={profile} update={onUpdate}
            canMeasure={props.canMeasure} onOpenCalibration={props.onOpenCalibration}/>}
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
  </div></SettingContext.Provider>;
}
