import {useCallback,useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {setLanguage,t,useLanguage,type Language} from '../i18n';
import styles from './FloatingCalibration.module.css';

type Axis = 'yaw' | 'pitch';
type Action = {kind:'start';axis:Axis} | {kind:'finish'|'cancel'|'apply'|'close'};
type CalibrationView = {
  sequence:number;
  profileName:string;
  output:'mouse'|'right_stick';
  stick:'left'|'right';
  language:Language;
  theme:string;
  blocked:string|null;
  measuring:Axis|null;
  degrees:number;
  measurementError:string|null;
  result:{axis:Axis;degrees:number;peak:number;error:string|null}|null;
  saved:{applied:boolean;warning:string|null}|null;
  hotkeys:{measure:boolean;apply:boolean};
  actionError:string|null;
};

export function FloatingCalibration() {
  useLanguage();
  const [view,setView] = useState<CalibrationView>();
  const [busy,setBusy] = useState(false);
  const [error,setError] = useState<string>();
  const [connectionError,setConnectionError] = useState<string>();
  const pending = useRef(false);
  const sequence = useRef(0);
  const accept = useCallback((next:CalibrationView)=>{
    if (next.sequence <= sequence.current) return;
    sequence.current = next.sequence;
    setView(next);
    setConnectionError(undefined);
  },[]);

  useEffect(()=>{
    let disposed = false;
    const subscription = listen<CalibrationView>('game-calibration-update',({payload})=>{
      if (disposed) return;
      accept(payload);
      setError(undefined);
    });
    void subscription.then(()=>invoke<CalibrationView>('calibration_window_state')).then(initial=>{
      if (!disposed) accept(initial);
    }).catch(reason=>{
      if (!disposed) setConnectionError(String(reason));
    });
    return ()=>{
      disposed = true;
      void subscription.then(unlisten=>unlisten()).catch(()=>{});
    };
  },[accept]);

  useEffect(()=>{
    if (view?.language) setLanguage(view.language);
    if (view?.theme) document.documentElement.dataset.theme = view.theme;
  },[view?.language,view?.theme]);

  async function act(action:Action) {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setError(undefined);
    try {
      await invoke('calibration_window_action',{action});
      if (action.kind !== 'close') {
        accept(await invoke<CalibrationView>('calibration_window_state'));
      }
    } catch (reason) {
      setError(String(reason));
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }

  const disabled = busy || !!connectionError;
  const {blocked,measuring,result,saved} = view??{};
  const resultError = result && (result.error || (!Number.isFinite(result.degrees) || Math.abs(result.degrees)<1
    ? '测得的手柄转角不足，请重新测量。'
    : view?.output==='right_stick' && result.peak>1
      ? '测量过程中已超过满杆速度，请更慢地转动后重新测量。'
      : null));
  const degrees = Math.abs(result?.degrees ?? view?.degrees ?? 0).toFixed(1);
  const displayError = view?.actionError ?? error ?? connectionError;

  return <main className={styles.surface}>
    <header className={styles.header}>
      <div>
        <h1>游戏内转角校准</h1>
        {view && <p><span translate="no">{view.profileName}</span> · {view.output==='mouse'?'鼠标':'虚拟右摇杆'}</p>}
      </div>
      <button type="button" className="secondary-btn" disabled={busy} onClick={()=>void act({kind:'close'})}>关闭悬浮窗</button>
    </header>
    {view && <div className={styles.hotkeys}>
      {view.hotkeys.measure && !blocked && !saved && <span><kbd>Ctrl + Shift + F8</kbd> {measuring?'结束测量':result?'重测水平 360°':'开始水平测量'}</span>}
      {view.hotkeys.apply && (saved || (!blocked && result && !resultError)) && <span><kbd>Ctrl + Shift + F9</kbd> {saved?'关闭悬浮窗':'保存并应用校准'}</span>}
    </div>}
    {displayError && <p className="studio-error" role="alert">{displayError}</p>}
    {!view && !connectionError && <p role="status">正在载入转角校准…</p>}
    {view && (saved ?
      <p className={saved.warning?'studio-error':styles.notice} role={saved.warning?'alert':'status'}>
        {saved.warning ? t('配置已保存，应用失败：{0}').replace('{0}',t(saved.warning)) : saved.applied ? '配置已保存并应用' : '配置已保存'}
      </p> : blocked ? <p className="studio-error" role="alert">{blocked}</p> : measuring ? <>
      <div className={styles.readout}>
        <p role="status">{measuring==='yaw'?'正在测量水平 360°':'正在测量垂直 180°'}</p>
        <strong>{degrees}°</strong><span>手柄净转角</span>
      </div>
      <p>{measuring==='yaw'?'让游戏镜头转一整圈并回到参照点。松开摇杆，等镜头停稳后结束测量。':'让游戏镜头从最低转到最高，停稳后结束测量。'}</p>
      {view.measurementError && <p className="studio-error" role="alert">{view.measurementError}</p>}
      <div className="studio-actions">
        <button type="button" className="primary-btn" disabled={disabled} onClick={()=>void act({kind:'finish'})}>结束测量</button>
        <button type="button" className="secondary-btn" disabled={disabled} onClick={()=>void act({kind:'cancel'})}>取消测量</button>
      </div>
    </> : result ? <>
      <div className={styles.readout}>
        <p role="status">{result.axis==='yaw'?'水平测量已完成':'垂直测量已完成'}</p>
        <strong>{degrees}°</strong><span>手柄净转角</span>
      </div>
      {resultError && <p className="studio-error" role="alert">{resultError}</p>}
      <div className="studio-actions">
        <button type="button" className="primary-btn" disabled={disabled||!!resultError} onClick={()=>void act({kind:'apply'})}>保存并应用校准</button>
        <button type="button" className="secondary-btn" disabled={disabled} onClick={()=>void act({kind:'cancel'})}>重新测量</button>
      </div>
    </> : <>
      <p>先在游戏内选一个清楚的参照点，再开始测量。</p>
      <p>{view.stick==='left'?'水平测量可沿左摇杆外圈转动，再用体感微调。':'水平测量可沿右摇杆外圈转动，再用体感微调。'}</p>
      {view.measurementError && <p className="studio-error" role="alert">{view.measurementError}</p>}
      <div className="studio-actions">
        <button type="button" className="primary-btn" disabled={disabled} onClick={()=>void act({kind:'start',axis:'yaw'})}>测量水平 360°</button>
        <button type="button" className="secondary-btn" disabled={disabled} onClick={()=>void act({kind:'start',axis:'pitch'})}>测量垂直 180°</button>
      </div>
      <p className="studio-muted">{view.hotkeys.measure
        ? '请使用窗口或无边框全屏。游戏锁定光标时，可以用上方的校准按键。'
        : '请使用窗口或无边框全屏。游戏锁定光标时，可通过游戏菜单调出光标。'}</p>
    </>)}
  </main>;
}
