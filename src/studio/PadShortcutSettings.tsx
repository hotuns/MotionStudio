import {useEffect, useRef, useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {t} from '../i18n';
import {padInputNeutral, padBindingCandidates} from '../pad-capture';
import {chordParts, inputs, type Status} from '../types';
import {InputChordFields, inputOpposites} from './BindingFields';
import {padInputLabel} from './binding-choices';
import styles from './CursorSwitchPanel.module.css';

const modes = [['controls', '按键与方向'], ['regions', '摇杆内外圈']] as const;
type Mode = (typeof modes)[number][0];

const isNeutral = (status: Status, mode: Mode) => padInputNeutral(status)
  && (mode !== 'regions' || (status.stick_regions ?? []).every(region => region === 'center'));
const inputSerial = (status: Status, mode: Mode) =>
  mode === 'regions' ? status.pad_region_capture?.serial ?? 0 : status.input_serial;
const outerCount = (parts: string[]) => parts.filter(part => part.endsWith('_outer')).length;

export function validatePadShortcut(input: string) {
  const parts = chordParts(input);
  if (!parts.length || parts.length > 4 || new Set(parts).size !== parts.length ||
      parts.some(part => !inputs.includes(part) || parts.includes(inputOpposites[part]))) {
    return '请选择 1–4 个有效且不冲突的手柄输入。';
  }
  if (parts.includes('back') && parts.includes('start')) return 'View + Menu 已用于进入手柄界面操作，请选择其他组合。';
  return null;
}

export function PadShortcutSettings({input, status, busy, onChange, onPrepare, onCapture, onRecordingChange}: {
  input: string;
  status?: Status;
  busy: boolean;
  onChange: (input: string) => void;
  onPrepare: () => Promise<Status>;
  onCapture: (active: boolean) => Promise<void>;
  onRecordingChange: (active: boolean) => void;
}) {
  const [mode, setMode] = useState<Mode>('controls');
  const [recording, setRecording] = useState(false);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [candidate, setCandidate] = useState<string[]>([]);
  const mounted = useRef(true);
  const lease = useRef(false);
  const serial = useRef(0);
  const armed = useRef(false);
  const peak = useRef<string[]>([]);
  const deviceId = useRef<number>();
  const latest = useRef({onCapture, onRecordingChange});
  latest.current = {onCapture, onRecordingChange};
  const split = (status?.device?.touchpads ?? 0) >= 2;
  const label = (parts: string[]) => parts.map(part => t(padInputLabel(part, split))).join(' + ');

  const release = () => {
    if (!lease.current) return;
    lease.current = false;
    latest.current.onRecordingChange(false);
    void latest.current.onCapture(false).catch(() => {});
    void invoke('set_shortcut_capture', {active:false}).catch(() => {});
  };
  const cancel = () => { setRecording(false); setCandidate([]); };
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; release(); };
  }, []);
  useEffect(() => {
    if (!recording) return;
    const key = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { event.preventDefault(); event.stopImmediatePropagation(); cancel(); }
    };
    const visibility = () => { if (document.hidden) cancel(); };
    window.addEventListener('blur', cancel);
    window.addEventListener('keydown', key, true);
    document.addEventListener('visibilitychange', visibility);
    const timer = window.setInterval(() => {
      void Promise.all([invoke('set_shortcut_capture', {active:true}), latest.current.onCapture(true)])
        .catch(error => { setError(String(error)); cancel(); });
    }, 1000);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener('blur', cancel);
      window.removeEventListener('keydown', key, true);
      document.removeEventListener('visibilitychange', visibility);
      release();
    };
  }, [recording]);
  useEffect(() => {
    if (!recording || !status) return;
    if (!status.binding_capture || status.device?.id !== deviceId.current || status.state !== 'capturing') {
      cancel();
      setError('手柄连接或观察状态已改变，录入已取消。');
      return;
    }
    const nextSerial = inputSerial(status, mode);
    const neutral = isNeutral(status, mode);
    if (!armed.current) {
      serial.current = nextSerial;
      if (neutral) armed.current = true;
      return;
    }
    if (nextSerial > serial.current) {
      serial.current = nextSerial;
      const raw = mode === 'regions' ? status.pad_region_capture?.chord ?? [] :
        status.input_chord?.length ? status.input_chord : status.last_input ? [status.last_input] : [];
      const next = padBindingCandidates(raw, split);
      const failure = validatePadShortcut(next.join('+'));
      if (failure) {
        setError(failure);
        peak.current = [];
        setCandidate([]);
        armed.current = neutral;
        return;
      }
      if (next.length > peak.current.length ||
          (next.length === peak.current.length && outerCount(next) >= outerCount(peak.current))) {
        peak.current = next;
        setCandidate(next);
        setError(null);
      }
    }
    if (peak.current.length && neutral) {
      onChange(peak.current.join('+'));
      cancel();
    }
  }, [recording, status, mode, split, onChange]);

  async function record() {
    setStarting(true);
    setError(null);
    lease.current = true;
    onRecordingChange(true);
    try {
      await invoke('set_shortcut_capture', {active:true});
      if (!mounted.current) { lease.current = true; release(); return; }
      const current = await onPrepare();
      if (!mounted.current) { lease.current = true; release(); return; }
      if (!current.device || !current.binding_capture) throw new Error('所选手柄未连接，无法录入快捷键。');
      if (!document.hasFocus() || document.hidden) { release(); return; }
      deviceId.current = current.device.id;
      serial.current = inputSerial(current, mode);
      armed.current = isNeutral(current, mode);
      peak.current = [];
      setCandidate([]);
      setRecording(true);
    } catch (error) {
      lease.current = true;
      release();
      if (mounted.current) setError(String(error));
    } finally {
      if (mounted.current) setStarting(false);
    }
  }

  const locked = busy || recording || starting;
  const changeDraft = (value: string) => { onChange(value); setError(null); };
  return <section className={styles.card}>
    <div className={styles.heading}><h5>手柄快捷键</h5>
      {input ? <kbd translate="no">{label(chordParts(input))}</kbd> : <span>未绑定</span>}
    </div>
    <p className={styles.hint}>支持手柄单键、触摸、摇杆方向与内外圈，最多可组合 4 个输入。</p>
    <button type="button" className="secondary-btn" disabled={locked} onClick={() => changeDraft('left_stick_outer')}>使用左摇杆外圈</button>
    <fieldset disabled={locked} aria-label="手柄快捷键输入">
      <InputChordFields value={input} split={split} onChange={changeDraft} showHint={false}/>
    </fieldset>
    <div className={styles.recordMode} role="group" aria-label="手柄录入方式">
      {modes.map(([id, name]) => <button key={id} type="button" className="secondary-btn" disabled={locked}
        aria-pressed={mode === id} onClick={() => setMode(id)}>{name}</button>)}
      <button type="button" className="secondary-btn" aria-label="录入手柄切换快捷键" disabled={locked}
        onClick={() => void record()}>{starting ? '正在准备…' : recording ? '正在录入手柄…' : '监听手柄'}</button>
    </div>
    <div className={styles.shortcutActions}>
      {recording ? <button type="button" className="ghost-btn" onClick={() => { cancel(); setError(null); }}>取消录入</button> :
        <button type="button" className="ghost-btn" disabled={locked || !input}
          onClick={() => changeDraft('')}>清除手柄快捷键</button>}
    </div>
    {recording && <p className={styles.hint} role="status">
      {candidate.length ? <strong translate="no">{label(candidate)}</strong> : mode === 'regions' ?
        '请让摇杆回中，再推到要录入的区域并松开。' : '请先松开已有按键，再按住所需组合并全部松开。'}
      <br/>录入期间暂停游戏输出，手柄保持连接。Esc 取消，录入后点击保存。
    </p>}
    {error && <p className={styles.error} role="alert">{error}</p>}
    <p className={styles.hint}>摇杆内外圈使用当前运行配置基础层的死区和外圈设置。View + Menu 保留用于操作界面。</p>
  </section>;
}
