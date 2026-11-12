import { chordParts, targetFrom, targetValue, type Entry, type Settings, type Shortcut, type ShortcutRule, type Status, type Target } from '../types';
import {useCallback,useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {t} from '../i18n';
import {shortcutFromEvent, shortcutKeys, shortcutLabel, shortcutModifier, shortcutRules, shortcutTriggerIdentity} from '../shortcuts';
import styles from './CursorSwitchPanel.module.css';
import {PadShortcutSettings, validatePadShortcut} from './PadShortcutSettings';
import {OptionPicker, type PickerGroup} from './OptionPicker';
import {keyboardChoices, padInputLabel} from './binding-choices';
import {Dialog, Toggle} from './Controls';

const keyChoices = keyboardChoices(shortcutKeys);

export function SwitchSettings({ entries, settings, busy, onChange, onSaveRules, status, onPreparePad, onPadCapture }: {
  entries: Entry[];
  settings: Settings;
  busy: boolean;
  onChange: (patch: Partial<Settings>) => void;
  onSaveRules: (rules: ShortcutRule[]) => Promise<void>;
  status?:Status;
  onPreparePad:()=>Promise<Status>;
  onPadCapture:(active:boolean)=>Promise<void>;
}) {
  const [editing,setEditing]=useState<{index:number|null}|null>(null);
  const [removing,setRemoving]=useState(false);
  const [ruleError,setRuleError]=useState<string|null>(null);
  const [overlayError,setOverlayError]=useState<string|null>(null);
  const [delay,setDelay]=useState(String(settings.cursor_debounce_ms??250));
  const [delayError,setDelayError]=useState(false);
  useEffect(()=>{setDelay(String(settings.cursor_debounce_ms??250));setDelayError(false);},[settings.cursor_debounce_ms]);
  const saveDelay=()=>{
    const value=Number(delay);
    if(!delay.trim()||!Number.isInteger(value)||value<0||value>5000){setDelayError(true);return;}
    setDelayError(false);
    if(!busy&&value!==(settings.cursor_debounce_ms??250))onChange({cursor_debounce_ms:value});
  };
  const rules = shortcutRules(settings);
  const locked = busy || removing;
  const split = (status?.device?.touchpads ?? 0) >= 2;
  const editRule = (index:number|null) => { setRuleError(null);setEditing({index}); };
  const removeRule = async (index:number) => {
    setRemoving(true);
    setRuleError(null);
    try { await onSaveRules(rules.filter((_, i) => i !== index)); }
    catch (error) { setRuleError(String(error)); }
    finally { setRemoving(false); }
  };
  const profiles = {label:'游戏配置', options:entries.map(entry => ({value:entry.id, label:entry.profile.name, localize:false as const}))};
  const targetOptions = (label: string, value: Target, onSelect: (target: Target) => void) => (
    <div className={styles.field}>{label}
      <OptionPicker label={label} value={targetValue(value)} disabled={locked}
        groups={[{options:[{value:'none',label:'不切换'}, {value:'restore',label:'恢复切换前的配置'}]}, profiles]}
        onChange={next => onSelect(targetFrom(next))}/>
    </div>
  );

  return <div>
    <section className={styles.panel}>
      <div className={styles.heading}>
        <div><h4>光标显示状态</h4><p>分别选择光标显示和隐藏时要使用的配置。</p></div>
        <label className={styles.toggle}>
          <input type="checkbox" checked={settings.cursor_enabled} disabled={locked} onChange={event => onChange({ cursor_enabled: event.target.checked })} />
          启用光标自动切换
        </label>
      </div>
      <div className={styles.controls}>
        {targetOptions('光标显示时', settings.cursor_shown, target => onChange({ cursor_shown: target }))}
        {targetOptions('光标隐藏时', settings.cursor_hidden, target => onChange({ cursor_hidden: target }))}
        <label>状态确认时间<div className={styles.delayInput}>
          <input aria-label="光标状态确认时间" type="number" min={0} max={5000} step={10} inputMode="numeric"
            value={delay} disabled={locked} onChange={event=>{setDelay(event.target.value);setDelayError(false);}}
            onBlur={saveDelay} onKeyDown={event=>{if(event.key==='Enter'){event.preventDefault();event.currentTarget.blur();}}}/><span>ms</span>
        </div></label>
      </div>
      {delayError&&<p className={styles.error} role="alert">请输入 0–5000 毫秒之间的整数。</p>}
      <p className={styles.hint}>光标状态连续保持该时间后才切换。设为 0 可取消额外等待；检测仍受约 50 ms 检查周期和配置加载时间影响。输入后按回车或移开焦点保存。</p>
      <p className={styles.hint}>「恢复切换前的配置」会回到本轮自动切换前使用的配置。</p>
    </section>
    <section className={styles.panel}>
      <div className={styles.heading}>
        <div><h4>快捷键切换</h4><p>快捷键在映射或观察时生效。</p></div>
        <button type="button" className="primary-btn" disabled={locked || !entries.length || rules.length >= 64}
          onClick={() => editRule(null)}>添加切换规则</button>
      </div>
      {rules.length ? <ul className={styles.ruleList}>
        {rules.map((rule,index) => <li key={shortcutTriggerIdentity(rule.trigger)} className={styles.rule}>
          <div className={styles.ruleTrigger}>
            <span>{rule.trigger.kind === 'keyboard' ? '键盘快捷键' : '手柄快捷键'}</span>
            <kbd translate="no">{rule.trigger.kind === 'keyboard' ? shortcutLabel(rule.trigger.key) :
              chordParts(rule.trigger.input).map(input => t(padInputLabel(input,split))).join(' + ')}</kbd>
          </div>
          <div className={styles.ruleTarget}>
            <strong translate="no">{entries.find(entry => entry.id === rule.target)?.profile.name ?? rule.target}</strong>
            <span>{rule.return_on_repeat !== false ? '再次触发时返回' : '只切换到目标配置'}</span>
          </div>
          <div className={styles.shortcutActions}>
            <button type="button" className="secondary-btn" disabled={locked} onClick={() => editRule(index)}>编辑</button>
            <button type="button" className="ghost-btn" disabled={locked} onClick={() => void removeRule(index)}>删除</button>
          </div>
        </li>)}
      </ul> : <p className={styles.hint}>暂无快捷键规则。</p>}
      {ruleError&&<p className={styles.error} role="alert">{ruleError}</p>}
      <p className={styles.hint}><kbd>F12</kbd> 可停止映射并释放输出。</p>
    </section>
    {editing && <ShortcutRuleEditor initial={editing.index===null ? undefined : rules[editing.index]}
      otherRules={rules.filter((_,index) => index !== editing.index)} profiles={profiles} busy={locked} status={status}
      onPreparePad={onPreparePad} onPadCapture={onPadCapture} onClose={() => setEditing(null)}
      onSave={async rule => {
        const next = [...rules];
        if (editing.index === null) next.push(rule);
        else next[editing.index] = rule;
        await onSaveRules(next);
        setEditing(null);
      }}/>}
    <section className={styles.panel}>
      <div className={styles.heading}>
        <div><h4>游戏内配置提示</h4><p>切换成功后，在游戏所在屏幕的右下角显示配置名称。</p></div>
        <label className={styles.toggle}><input type="checkbox" aria-label="显示游戏内配置提示"
          checked={settings.overlay_enabled!==false} disabled={locked} onChange={event=>onChange({overlay_enabled:event.target.checked})}/>启用配置提示</label>
      </div>
      <div className={`${styles.controls} ${styles.overlayControls}`}>
        <label>显示时长<select aria-label="配置提示显示时长" value={settings.overlay_duration_ms??2500} disabled={locked||settings.overlay_enabled===false}
          onChange={event=>onChange({overlay_duration_ms:Number(event.target.value)})}>
          {Array.from(new Set([1000,2500,4000,6000,8000,settings.overlay_duration_ms??2500])).sort((a,b)=>a-b).map(ms=><option key={ms} value={ms}>{ms/1000} 秒</option>)}
        </select></label>
        <button type="button" className="secondary-btn" disabled={locked||settings.overlay_enabled===false} onClick={()=>{setOverlayError(null);void invoke('preview_overlay').catch(error=>setOverlayError(String(error)));}}>预览配置提示</button>
      </div>
      {overlayError&&<p className={styles.error} role="alert">{overlayError}</p>}
      <div className={styles.overlayPreview} aria-label="配置提示样式示例">
        <strong translate="no">{entries.find(entry=>entry.id===settings.selected_profile)?.profile.name??'体感瞄准'}</strong>
      </div>
      <p className={styles.hint}>提示会自动消失，不抢焦点、不接收鼠标点击。连续切换会显示最新配置。支持窗口化和无边框全屏；独占全屏可能无法显示，可尝试改为无边框全屏。</p>
    </section>
  </div>;
}

function ShortcutRuleEditor({initial, otherRules, profiles, busy, status, onPreparePad, onPadCapture, onClose, onSave}: {
  initial?: ShortcutRule;
  otherRules: ShortcutRule[];
  profiles: PickerGroup;
  busy: boolean;
  status?: Status;
  onPreparePad: () => Promise<Status>;
  onPadCapture: (active: boolean) => Promise<void>;
  onClose: () => void;
  onSave: (rule: ShortcutRule) => Promise<void>;
}) {
  const [kind, setKind] = useState<ShortcutRule['trigger']['kind']>(initial?.trigger.kind ?? 'keyboard');
  const [key, setKey] = useState<Shortcut|null>(initial?.trigger.kind === 'keyboard' ? initial.trigger.key : null);
  const [pad, setPad] = useState(initial?.trigger.kind === 'gamepad' ? initial.trigger.input : '');
  const [target, setTarget] = useState(initial?.target ?? '');
  const [returnOnRepeat, setReturnOnRepeat] = useState(initial?.return_on_repeat ?? true);
  const [recording, setRecording] = useState(false);
  const [starting, setStarting] = useState(false);
  const [padCapturing, setPadCapturing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string|null>(null);
  const changePad = useCallback((input: string) => { setPad(input);setError(null); }, []);
  const mounted = useRef(true);
  const keyboardLease = useRef(false);
  const capture = useRef(onPadCapture);
  capture.current = onPadCapture;

  const releaseKeyboard = () => {
    if (!keyboardLease.current) return;
    keyboardLease.current = false;
    void capture.current(false).catch(() => {});
    void invoke('set_shortcut_capture', {active:false}).catch(() => {});
  };
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; releaseKeyboard(); };
  }, []);
  useEffect(() => {
    if (!recording) return;
    let modifier: Shortcut | undefined;
    const finish = (shortcut:Shortcut) => {
      setKey(shortcut);
      setError(null);
      setRecording(false);
    };
    const keydown = (event:KeyboardEvent) => {
      event.preventDefault();
      event.stopImmediatePropagation();
      if (event.repeat) return;
      if (event.key === 'Escape') { setRecording(false);setError(null);return; }
      modifier = undefined;
      const result = shortcutFromEvent(event);
      if (result.error) { setError(result.error);return; }
      if (!result.shortcut) return;
      if (shortcutModifier(result.shortcut.key)) { modifier = result.shortcut;setError(null); }
      else finish(result.shortcut);
    };
    const keyup = (event:KeyboardEvent) => {
      if (modifier && !event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey) {
        event.preventDefault();
        event.stopImmediatePropagation();
        finish(modifier);
        modifier = undefined;
      }
    };
    const cancel = () => setRecording(false);
    const visibility = () => { if (document.hidden) cancel(); };
    window.addEventListener('keydown',keydown,true);
    window.addEventListener('keyup',keyup,true);
    window.addEventListener('blur',cancel);
    document.addEventListener('visibilitychange',visibility);
    const timer = window.setInterval(() => {
      void Promise.all([invoke('set_shortcut_capture',{active:true}),capture.current(true)]).catch(error => {
        setRecording(false);
        setError(String(error));
      });
    }, 1000);
    return () => {
      window.removeEventListener('keydown',keydown,true);
      window.removeEventListener('keyup',keyup,true);
      window.removeEventListener('blur',cancel);
      document.removeEventListener('visibilitychange',visibility);
      window.clearInterval(timer);
      releaseKeyboard();
    };
  }, [recording]);

  async function recordKeyboard() {
    setStarting(true);
    setError(null);
    keyboardLease.current = true;
    try {
      await invoke('set_shortcut_capture', {active:true});
      if (!mounted.current) { keyboardLease.current = true;releaseKeyboard();return; }
      await capture.current(true);
      if (mounted.current && document.hasFocus() && !document.hidden) setRecording(true);
      else { keyboardLease.current = true;releaseKeyboard(); }
    } catch (error) {
      keyboardLease.current = true;
      releaseKeyboard();
      if (mounted.current) setError(String(error));
    } finally {
      if (mounted.current) setStarting(false);
    }
  }

  const locked = busy || starting || recording || padCapturing || saving;
  const trigger: ShortcutRule['trigger'] | null = kind === 'keyboard'
    ? key && {kind,key} : pad ? {kind,input:pad} : null;
  const canSave = !!trigger && profiles.options.some(profile => profile.value === target);
  async function save() {
    if (locked || !canSave || !trigger) return;
    const failure = trigger.kind === 'gamepad' ? validatePadShortcut(trigger.input) : null;
    if (failure) { setError(failure);return; }
    const identity = shortcutTriggerIdentity(trigger);
    if (otherRules.some(rule => shortcutTriggerIdentity(rule.trigger) === identity)) {
      setError(trigger.kind === 'keyboard' ? '同一个键盘快捷键只能用于一条切换规则' : '同一个手柄快捷键只能用于一条切换规则');
      return;
    }
    setSaving(true);
    setError(null);
    try { await onSave({trigger,target,return_on_repeat:returnOnRepeat}); }
    catch (error) { if (mounted.current) setError(String(error)); }
    finally { if (mounted.current) setSaving(false); }
  }

  return <Dialog title={initial ? '编辑切换规则' : '添加切换规则'} error={error??undefined}
    onClose={() => { if (!saving) onClose(); }} onSubmit={() => void save()}
    actions={<>
      <button type="button" className="secondary-btn" disabled={saving} onClick={onClose}>取消</button>
      <button type="submit" className="primary-btn" disabled={locked || !canSave}>保存</button>
    </>}>
    <div className={styles.ruleEditor}>
      <div className={styles.field}>
        <span>触发条件</span>
        <div className={styles.recordMode} role="group" aria-label="触发条件">
          <button type="button" className="secondary-btn" aria-pressed={kind==='keyboard'} disabled={locked}
            onClick={() => { setKind('keyboard');setError(null); }}>键盘快捷键</button>
          <button type="button" className="secondary-btn" aria-pressed={kind==='gamepad'} disabled={locked}
            onClick={() => { setKind('gamepad');setError(null); }}>手柄快捷键</button>
        </div>
      </div>
      {kind === 'keyboard' ? <section className={styles.card}>
        <div className={styles.heading}><h5>键盘快捷键</h5>
          {key ? <kbd translate="no">{shortcutLabel(key)}</kbd> : <span>未绑定</span>}
        </div>
        <p className={styles.hint}>可选择单键，或录入单键和 Ctrl、Alt、Shift 组合键。</p>
        <OptionPicker label="选择键盘按键" placeholder="选择键盘按键" value={key?.key ?? null} groups={keyChoices} disabled={locked}
          onChange={next => { setError(null);setKey({key:next,ctrl:false,alt:false,shift:false}); }}/>
        <div className={styles.shortcutActions}>
          <button type="button" className="secondary-btn" aria-label="录入切换快捷键" disabled={locked}
            onClick={() => void recordKeyboard()}>{starting ? '正在准备…' : recording ? '请按下快捷键…' : '录入快捷键'}</button>
          {recording && <button type="button" className="ghost-btn" onClick={() => { setRecording(false);setError(null); }}>取消录入</button>}
        </div>
        {recording && <p className={styles.hint} role="status">请按单键或组合键。Esc 取消录入，录入期间暂停游戏输出和自动切换。</p>}
      </section> : <PadShortcutSettings input={pad} status={status} busy={busy||saving}
        onChange={changePad} onPrepare={onPreparePad} onCapture={onPadCapture} onRecordingChange={setPadCapturing}/>}
      <div className={styles.field}>目标配置
        <OptionPicker label="快捷键目标配置" placeholder="选择配置" value={target||null} groups={[profiles]} disabled={locked}
          onChange={next => { setTarget(next);setError(null); }}/>
      </div>
      <Toggle label="再次触发时返回" value={returnOnRepeat} onChange={setReturnOnRepeat} disabled={locked}
        help="开启后，在目标配置下再次触发此快捷键，会回到本轮自动切换前的配置。"/>
    </div>
  </Dialog>;
}
