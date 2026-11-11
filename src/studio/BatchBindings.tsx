import {useCallback,useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {t,type Language} from '../i18n';
import {modifierOutputs} from '../keyboard';
import {listenForBindingInput} from '../output-capture';
import {chordParts,outputLabel,type Action} from '../types';
import {OutputChordFields,triggerNames} from './BindingFields';
import {padInputLabel} from './binding-choices';
import {Dialog} from './Controls';
import styles from './BatchBindings.module.css';

export type BatchScope = {
  profileId: string;
  profileName: string;
  layerId: string;
  layerName?: string;
  actions: Action[];
  maxActions: number;
};
type RecordedAction = Pick<Action,'name'|'output'>;
export type BatchProgress = {cursor:number; values:(string|null)[]; added:RecordedAction[]};
export type BindingWindowState = {
  id:string;
  scope:BatchScope;
  progress:BatchProgress;
  split:boolean;
  language:Language;
  theme:'dark'|'light';
  outcome:'return'|'keep'|null;
};
export const emptyBatchProgress = (scope:BatchScope):BatchProgress => ({
  cursor:0, values:scope.actions.map(()=>null), added:[],
});

const suggestedName = (output:string) => t('新动作')+' · '+outputLabel(output);
export function addedDraftActions(progress:BatchProgress, originalCount:number):Action[] {
  return progress.added.map((record,index)=>{
    const output = progress.values[originalCount+index]??record.output;
    return {name:record.name.trim()||suggestedName(output),input:'',output,threshold:0.5};
  });
}

type CaptureLease = {id:string; pending:Promise<void>; stopping:boolean};
type CapturePhase = 'idle'|'starting'|'recording'|'stopping';

function useCaptureLease(onError:(message:string)=>void) {
  const [phase,setPhase] = useState<CapturePhase>('idle');
  const lease = useRef<CaptureLease>();
  const mounted = useRef(false);
  const report = useRef(onError);
  report.current = onError;

  const stop = useCallback(()=>{
    const active = lease.current;
    if (!active || active.stopping) return active?.pending;
    active.stopping = true;
    if (mounted.current) setPhase('stopping');
    active.pending = active.pending.catch(()=>{}).then(()=>
      invoke<void>('batch_capture',{id:active.id,action:'stop'})
    ).catch(error=>{
      if (mounted.current) report.current(String(error));
    }).finally(()=>{
      if (lease.current!==active) return;
      lease.current = undefined;
      if (mounted.current) setPhase('idle');
    });
    return active.pending;
  },[]);

  useEffect(()=>{
    mounted.current = true;
    const blur = ()=>{void stop();};
    const visibility = ()=>{if (document.hidden) blur();};
    window.addEventListener('blur',blur);
    document.addEventListener('visibilitychange',visibility);
    return ()=>{
      mounted.current = false;
      window.removeEventListener('blur',blur);
      document.removeEventListener('visibilitychange',visibility);
      void stop();
    };
  },[stop]);

  useEffect(()=>{
    if (phase!=='recording') return;
    let renewing = false;
    const timer = window.setInterval(()=>{
      const active = lease.current;
      if (!active || active.stopping || renewing) return;
      renewing = true;
      active.pending = invoke<void>('batch_capture',{id:active.id,action:'renew'});
      void active.pending.catch(error=>{
        if (lease.current===active && !active.stopping) {
          report.current(String(error));
          void stop();
        }
      }).finally(()=>{renewing=false;});
    },1000);
    return ()=>window.clearInterval(timer);
  },[phase,stop]);

  async function start() {
    if (lease.current) return;
    const id = crypto.randomUUID();
    const active = {id,pending:invoke<void>('batch_capture',{id,action:'start'}),stopping:false};
    lease.current = active;
    setPhase('starting');
    try {
      await active.pending;
      if (active.stopping) return;
      if (!document.hasFocus() || document.hidden) void stop();
      else setPhase('recording');
    } catch (error) {
      if (active.stopping) return;
      report.current(String(error));
      void stop();
    }
  }
  return {phase,start,stop,isRecording:()=>!!lease.current && !lease.current.stopping};
}

export function BatchBindings({scope,progress,onProgress,busy=false,captureLease=false,split,floating=false,error,onOpenWindow,onClose,onKeep}: {
  scope: BatchScope;
  progress: BatchProgress;
  onProgress: (progress:BatchProgress)=>void;
  busy?: boolean;
  captureLease?: boolean;
  split: boolean;
  floating?: boolean;
  error?: string;
  onOpenWindow?: (progress:BatchProgress)=>void;
  onClose: (progress:BatchProgress)=>void;
  onKeep: (progress:BatchProgress)=>void;
}) {
  const {cursor,values} = progress;
  const originalCount = scope.actions.length;
  const actions = [...scope.actions,...addedDraftActions(progress,originalCount)];
  const action = actions[cursor];
  const addedIndex = cursor-originalCount;
  const selectedAdded = progress.added[addedIndex];
  const latest = useRef(progress);
  latest.current = progress;
  const activeRow = useRef<HTMLTableRowElement>(null);
  const captureArea = useRef<HTMLDivElement>(null);
  const continuous = useRef(false);
  const wheelReady = useRef(0);
  const [preview,setPreview] = useState<string[]>([]);
  const [step,setStep] = useState(0);
  const inputGeneration = useRef(0);
  const [mode,setMode] = useState<'row'|'new'>('row');
  const [manual,setManual] = useState(false);
  const [localError,setLocalError] = useState<string>();
  const {phase,start,stop,isRecording} = useCaptureLease(setLocalError);
  const recording = phase==='recording';
  const adding = mode==='new' && phase!=='idle';
  const waitingForRelease = phase==='idle' && captureLease;
  const busyTransition = busy || phase==='starting' || phase==='stopping';
  const ready = !busyTransition && !waitingForRelease;
  const filled = values.slice(0,originalCount).filter(Boolean).length+progress.added.length;
  const inputFor = (item:Action) => item.input
    ? chordParts(item.input).map(part=>t(padInputLabel(part,split))).join(' + ')
    : t('待绑定手柄');
  const triggerFor = (item:Action) => t(triggerNames[item.trigger??'down']);
  const callbacks = useRef({onProgress,isRecording});
  callbacks.current = {onProgress,isRecording};

  function update(next:BatchProgress) {
    latest.current = next;
    callbacks.current.onProgress(next);
  }
  function resetInput() {
    setPreview([]);
    setStep(++inputGeneration.current);
  }
  function selectRow(index:number) {
    update({...latest.current,cursor:index});
    resetInput();
  }
  function recordValue(value:string) {
    if (!callbacks.current.isRecording() || !document.hasFocus() || document.hidden) return;
    const current = latest.current;
    if (mode==='new') {
      update({cursor:current.values.length,values:[...current.values,null],added:[...current.added,{name:'',output:value}]});
      setLocalError(undefined);
      resetInput();
      void stop();
      return;
    }
    const at = current.cursor;
    const next = continuous.current && at+1<actions.length;
    update({...current,cursor:next?at+1:at,values:current.values.map((entry,index)=>index===at?value:entry)});
    setLocalError(undefined);
    resetInput();
    if (!next) void stop();
  }
  const record = useRef(recordValue);
  record.current = recordValue;

  useEffect(()=>{
    if (!recording) return;
    return listenForBindingInput({
      output:true,
      pointerArea:captureArea.current,
      wheelReady,
      cancelOnEmergency:true,
      onValue:value=>{if (inputGeneration.current===step) record.current(value);},
      onPreview:setPreview,
      onCancel:()=>void stop(),
      onError:setLocalError,
    });
  },[recording,step,stop]);

  useEffect(()=>{
    activeRow.current?.scrollIntoView({block:'nearest',inline:'nearest'});
  },[cursor]);

  function begin(index:number,advance:boolean) {
    setMode('row');
    continuous.current = !floating && advance;
    wheelReady.current = 0;
    setManual(false);
    setLocalError(undefined);
    selectRow(index);
    if (!recording) void start();
  }
  function beginNew() {
    setMode('new');
    wheelReady.current = 0;
    setManual(false);
    setLocalError(undefined);
    resetInput();
    void start();
  }
  function move(delta:number) {
    const next = latest.current.cursor+delta;
    if (next===actions.length) void stop();
    else if (next>=0 && next<actions.length) selectRow(next);
  }
  function setValue(value:string|null) {
    const current = latest.current;
    update({...current,values:current.values.map((entry,index)=>index===current.cursor?value:entry)});
  }
  function renameAdded(name:string) {
    const current = latest.current;
    update({...current,added:current.added.map((entry,index)=>index===addedIndex?{...entry,name}:entry)});
  }
  function removeAdded() {
    const current = latest.current;
    const values = current.values.filter((_,index)=>index!==current.cursor);
    setManual(false);
    update({cursor:Math.max(0,Math.min(current.cursor,values.length-1)),
      values,added:current.added.filter((_,index)=>index!==addedIndex)});
  }
  function close() {
    if (phase!=='idle') void stop();
    else if (!busy) onClose(latest.current);
  }
  const assignEscape = () => recordValue([...preview.filter(value=>modifierOutputs.includes(value)),'key:ESC'].join('+'));

  const actionsBar = <>
    <button type="button" className="secondary-btn" data-cancel-capture onClick={close} disabled={busyTransition}>
      {recording?(floating?'取消录入':'暂停录入'):floating?'返回 Studio':'取消'}
    </button>
    <button type="button" className="primary-btn" disabled={!ready||recording||!filled}
      onClick={()=>onKeep(latest.current)}>写入配置草稿</button>
  </>;

  const body = <>
    <div className={styles.context}>
      <div><strong translate="no">{scope.profileName}</strong><span>{scope.layerName??t('基础层')}</span></div>
      <span>{t('已填 {0} / {1}').replace('{0}',String(filled)).replace('{1}',String(actions.length))}</span>
    </div>
    <section className={styles.prompt} aria-label="当前填键项目">
      <div className={styles.current} aria-live="polite">
        {adding ? <strong>新增动作</strong> : action && <>
          <span>{cursor+1} / {actions.length}</span>
          <strong translate="no">{action.name}</strong>
          <span>{inputFor(action)}{action.input && ' · '+triggerFor(action)}</span>
        </>}
      </div>
      {recording ? <div className={styles.captureArea} ref={captureArea} aria-label="鼠标录入区" role="region">
        <strong>按下并松开游戏键位</strong>
        <span>在这一区域点击鼠标按钮或滚动滚轮。</span>
        {!!preview.length&&<kbd>{outputLabel(preview.join('+'))}</kbd>}
      </div> : <p role={busyTransition?'status':undefined}>
        {phase==='starting'?'正在准备录入…'
          : phase==='stopping'?'正在结束录入…'
          : waitingForRelease?'松开手柄按键并让摇杆回中后可以继续。'
          : !action?'还没有动作，先录下第一个游戏键位。'
          : selectedAdded?'已录下游戏键位，可以填写动作名称。'
          : floating?'点击动作所在行修改游戏键位。'
          : '点动作旁的「录入」，或从选中的动作开始连续填写。'}
      </p>}
      <div className={styles.controls}>
        {recording ? <>
          {!floating&&<>
            <button type="button" className="secondary-btn" data-cancel-capture disabled={cursor===0} onClick={()=>move(-1)}>上一项</button>
            <button type="button" className="secondary-btn" data-cancel-capture onClick={()=>move(1)}>跳过</button>
          </>}
          <button type="button" className="secondary-btn" data-cancel-capture
            disabled={preview.filter(value=>modifierOutputs.includes(value)).length>=4} onClick={assignEscape}>映射 Esc</button>
        </> : <>
          {!floating&&action&&<button type="button" className="primary-btn"
            disabled={!ready} onClick={()=>begin(cursor,true)}>开始连续录入</button>}
          {onOpenWindow&&<button type="button" className="secondary-btn" disabled={!ready}
            onClick={()=>onOpenWindow(latest.current)}>打开录入悬浮窗</button>}
          {action&&<>
            <button type="button" className="ghost-btn" disabled={!ready} onClick={()=>setManual(open=>!open)}
              aria-expanded={manual}>手动选键</button>
            <button type="button" className="ghost-btn" disabled={!ready||!values[cursor]} onClick={()=>setValue(null)}>还原此项</button>
          </>}
          {selectedAdded&&<button type="button" className="ghost-btn" disabled={!ready} onClick={removeAdded}>撤销新增</button>}
        </>}
      </div>
      {selectedAdded&&action&&!adding&&!recording&&<label className={styles.actionName}>
        动作名称
        <input maxLength={100} value={selectedAdded.name} placeholder={suggestedName(action.output)} disabled={!ready}
          onChange={event=>renameAdded(event.target.value)}/>
      </label>}
      {manual&&action&&!recording&&<OutputChordFields showRepeat={false}
        action={{...action,output:values[cursor]??action.output}}
        onChange={({output})=>{if(output)setValue(output);}}/>}
    </section>
    {actions.length>0&&<div className={styles.rows}>
      <table>
        <thead><tr><th>游戏动作</th><th>手柄输入</th><th>游戏键位</th></tr></thead>
        <tbody>{actions.map((item,index)=><tr key={index} data-current={cursor===index} ref={cursor===index?activeRow:undefined}
          className={floating&&ready&&!adding?styles.clickableRow:undefined}
          onClick={floating&&ready&&!adding?()=>begin(index,false):undefined}>
          <td><button type="button" className={styles.actionButton} disabled={!ready||adding}
            onClick={floating?undefined:()=>selectRow(index)} data-cancel-capture
            aria-label={floating?t('录入 {0}').replace('{0}',item.name):undefined}
            aria-pressed={floating?undefined:cursor===index} translate="no">{item.name}</button></td>
          <td><div className={styles.binding}><span>{inputFor(item)}</span>{item.input&&<small>{triggerFor(item)}</small>}</div></td>
          <td><div className={styles.binding}>
            <kbd>{outputLabel(values[index]??item.output)}</kbd>
            {(index>=originalCount||values[index])&&<small data-filled="true">已填</small>}
            {!floating&&<button type="button" className={styles.rowRecord} disabled={!ready||adding} data-cancel-capture
              aria-label={t('录入 {0}').replace('{0}',item.name)} onClick={()=>begin(index,false)}>录入</button>}
          </div></td>
        </tr>)}</tbody>
      </table>
    </div>}
  </>;

  if (floating) return <main className={styles.surface} aria-label="悬浮窗录入">
    <header className={styles.surfaceHeading}>
      <div className={styles.surfaceTitle}>
        <h1>悬浮窗录入</h1>
        <button type="button" className="primary-btn" disabled={!ready||recording||actions.length>=scope.maxActions}
          title={actions.length>=scope.maxActions?'基础层与所有配置层合计最多 128 个动作':undefined}
          onClick={beginNew}>新增动作</button>
      </div>
      <p>点「新增动作」录下游戏键位，手柄输入可以稍后设置。</p>
    </header>
    <div className={styles.surfaceBody}>
      {(localError??error)&&<p className="studio-note studio-error" role="alert">{localError??error}</p>}
      {body}
    </div>
    <footer className={styles.surfaceFooter}>{actionsBar}</footer>
  </main>;

  return <Dialog title="批量填键" className={styles.dialog}
    description="按游戏里的键位依次填写。跳过的项目保留当前输出，录入结果先留在这张表。"
    error={localError??error} onClose={close} actions={actionsBar}>{body}</Dialog>;
}
