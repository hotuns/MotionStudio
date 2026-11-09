import { lazy, Suspense, useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { StudioShell, type StudioPage } from './studio/StudioShell';
import { StudioOverview } from './studio/StudioOverview';
import { GameProfilesPage } from './studio/GameProfilesPage';
import type { Capture, CaptureKind } from './studio/ActionMappings';
import type { GameTab } from './studio/game-tabs';
import { MotionSettings } from './studio/MotionSettings';
import { DeviceCalibration } from './studio/DeviceCalibration';
import { ProfileLibrary } from './studio/ProfileLibrary';
import { ProfileShareDialog } from './studio/ProfileShareDialog';
import type { CommunityProfile, SharedSource } from './profile-sharing';
import { SystemPage, type ComponentAction } from './studio/SystemPage';
import { SwitchSettings } from './studio/SwitchSettings';
import {shortcutLabel} from './shortcuts';
import { Dialog } from './studio/Controls';
import {ActionEditor} from './studio/ActionEditor';
import {keyboardKeyFromEvent,syncRecordedModifiers,modifierOutputs} from './keyboard';
import {ImportJsm} from './studio/ImportJsm';
import {ImportFiles} from './studio/ImportFiles';
import {GamepadNavigation} from './studio/GamepadNavigation';
import {LayerToolbar} from './studio/LayerToolbar';
import {editingProfile,updateLayer} from './layers';
import {useLanguage,setLanguage,getLanguage,t} from './i18n';
import { chordParts,inputLabel,outputLabel,keyNames, type Action, type ComponentStatus, type Entry, type Profile, type Settings, type Snapshot, type Workspace } from './types';
import { padBindingCandidates, gyroInputNeutral, heldPadInputs, type GyroBindingField, type GyroBindingRecording } from './pad-capture';
import dialogStyles from './studio/CreateProfileDialog.module.css';

const DevicePage = lazy(() => import('./studio/DevicePage'));

const templates = [
  ['blank', '空白配置', '从头添加自己的动作'],
  ['keyboard', '键鼠与体感', '左摇杆 WASD，扳机瞄准开火'],
  ['gyro', '只使用体感', '按住左扳机控制鼠标视角'],
  ['gamepad', '虚拟手柄', '标准按键和摇杆自动映射'],
  ['gyro-gamepad', '手柄与体感', '按住左扳机，将体感输出到右摇杆'],
  ['cursor', '光标操作', '左摇杆移动，A 确认，B 右键'],
] as const;
type Template = (typeof templates)[number][0];
type Editor = { index: number | null; action: Action };
type SaveResult = { entry: Entry; applied: boolean; warning: string | null };
type GyroCapture = GyroBindingRecording & {kind:'pad'; index:null; deviceId:number; restartCount:number; split:boolean};
type ActiveCapture = Capture | GyroCapture;
type CaptureRequest = Capture | Pick<GyroCapture,'kind'|'index'|'gyroField'>;

const uniqueGyroPath = (devices: Snapshot['devices']) => {
  const candidates = devices.filter(device => device.origin === 'physical' && device.path && device.gamepad && device.gyro && !device.open_error);
  return candidates.length === 1 ? candidates[0].path : '';
};

export default function App() {
  useLanguage();
  const [workspace, setWorkspace] = useState<Workspace | null>(null);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [draft, setDraft] = useState<Entry | null>(null);
  const [page, setPage] = useState<StudioPage>('overview');
  const [revealDeviceCalibration, setRevealDeviceCalibration] = useState(false);
  const [gameTab, setGameTab] = useState<GameTab>('actions');
  const [editingLayer,setEditingLayer]=useState('');
  useEffect(()=>{setEditingLayer('');},[draft?.id]);
  useEffect(()=>{if(editingLayer&&!draft?.profile.layers?.some(layer=>layer.id===editingLayer))setEditingLayer('');},[draft?.profile.layers,editingLayer]);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<{ text: string; error: boolean } | null>(null);
  const [devicePath, setDevicePath] = useState('');
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState('');
  const [template, setTemplate] = useState<Template>('blank');
  const [editor, setEditor] = useState<Editor | null>(null);
  const [capture, setCapture] = useState<ActiveCapture | null>(null);
  const [components, setComponents] = useState<ComponentStatus | null>(null);
  const [deleting, setDeleting] = useState<Entry | null>(null);
  const [confirmDiscard, setConfirmDiscard] = useState<Entry | null>(null);
  const [help, setHelp] = useState(false);
  const [failedApplyId, setFailedApplyId] = useState<string | null>(null);
  const [importingJsm,setImportingJsm]=useState(false),[importingToml,setImportingToml]=useState(false);
  const [libraryView, setLibraryView] = useState<'local' | 'community'>('local');
  const [sharedImport, setSharedImport] = useState<{community?: CommunityProfile} | null>(null);
  const [gamepadPreference,setGamepadPreference]=useState(()=>localStorage.getItem('motion-studio-gamepad-ui')!=='off');
  const [windowFocused,setWindowFocused]=useState(document.hasFocus());
  const autoNavigation=useRef(false),gestureSerial=useRef(0),snapshotEpoch=useRef(0);
  const previousInput = useRef(0);
  const padPreview=useRef<string[]>([]);
  const captureGeneration = useRef(0);
  const cancelCapture = useCallback(() => {
    captureGeneration.current++;
    setCapture(null);
  }, []);
  const captureRequests=useRef<Promise<void>>(Promise.resolve());
  const bindingCapture=useCallback((active:boolean)=>{
    const next=captureRequests.current.catch(()=>{}).then(()=>invoke<void>('set_binding_capture',{active}));
    captureRequests.current=next;return next;
  },[]);
  const draftRef = useRef(draft);
  draftRef.current = draft;
  const inFlight = useRef(false);

  const native = '__TAURI_INTERNALS__' in window;
  const entries = workspace?.profiles ?? [];
  const saved = entries.find(entry => entry.id === draft?.id);
  const dirty = !!draft && JSON.stringify(draft) !== JSON.stringify(saved);
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  const status = snapshot?.status;
  const gyroRecording = capture && 'gyroField' in capture ? capture : undefined;
  const actionRecording = capture && !('gyroField' in capture) ? capture : null;
  const captureActive = !!capture || !!status?.binding_capture;
  const measuringRef=useRef(false);
  measuringRef.current=!!status?.motion?.measuring;
  const active = !!status && !status.ui_navigation && ['running', 'observing', 'waiting', 'preparing_output', 'calibrating', 'capturing', 'capture_release'].includes(status.state);
  const waiting = !status?.ui_navigation && ['waiting','preparing_output'].includes(status?.state ?? '');
  const calibrating = status?.state === 'calibrating';
  const profile = draft?editingProfile(draft.profile,editingLayer):undefined;
  const device = snapshot?.devices.find(entry => entry.path === devicePath) ??
    (status?.device?.path === devicePath ? status.device : null);
  const canRecordGyro = !!snapshot?.engine_ready && !!device?.gamepad && device.origin !== 'virtual' && !device.open_error && !waiting;
  const canCalibrate = !!status?.device?.gyro && status.device.path===devicePath &&
    ['running','observing'].includes(status.state) && !captureActive && !status.motion?.measuring;
  const appliedId = active ? status?.profile_id : workspace?.settings.selected_profile;
  const pending = dirty || !!(draft && (draft.id !== appliedId || (active && draft.id === failedApplyId)));
  const canApply = !!draft && pending;
  const canObserve = !!(draft && devicePath && snapshot?.engine_ready);
  const deviceCount = snapshot?.devices.filter(device => device.gamepad && device.origin !== 'virtual' && !device.open_error).length ?? 0;

  const notify = useCallback((text: string, error = false) => setNotice({ text, error }), []);
  const refresh = useCallback(async (selectId?: string) => {
    const data = await invoke<Workspace>('workspace');
    if(data.settings.language)setLanguage(data.settings.language);
    setWorkspace(data);
    setSnapshot(data.snapshot);
    if (selectId || !draftRef.current) {
      const entry = data.profiles.find(item => item.id === (selectId ?? data.settings.selected_profile)) ?? data.profiles[0];
      setDraft(entry ? structuredClone(entry) : null);
    }
    const remembered = data.settings.device_path;
    const unusable = remembered.toLowerCase().startsWith('xinput#') ||
      data.snapshot.devices.some(device => device.path === remembered && device.origin === 'virtual');
    setDevicePath(previous => previous || (!unusable && remembered) || uniqueGyroPath(data.snapshot.devices));
  }, []);

  async function run(task: () => Promise<unknown>, message?: string) {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    try {
      await task();
      if (message) notify(message);
    } catch (error) {
      notify(String(error), true);
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }

  useEffect(() => {
    if (native) void refresh().catch(error => notify(String(error), true));
  }, [native, notify, refresh]);

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), notice.error ? 8000 : 3500);
    return () => window.clearTimeout(timer);
  }, [notice]);

  useEffect(() => {
    if (!native) return;
    let disposed = false;
    let fetching = false;
    const timer = window.setInterval(async () => {
      if (fetching) return;
      fetching = true;
      const epoch=snapshotEpoch.current;
      try {
        const data = await invoke<Snapshot>('snapshot');
        if (!disposed&&epoch===snapshotEpoch.current) {
          setSnapshot(data);
          setDevicePath(path => path || uniqueGyroPath(data.devices));
        }
      } catch {
        // The Tauri supervisor reports connection state when it becomes available.
      } finally {
        fetching = false;
      }
    }, status?.ui_navigation?33:150);
    return () => { disposed = true; window.clearInterval(timer); };
  }, [native,status?.ui_navigation]);

  useEffect(()=>{
    let disposed=false;
    const change=(event:Event)=>{
      const epoch=++snapshotEpoch.current;setWindowFocused(false);
      if(event.type==='blur'||!document.hasFocus()||document.hidden){cancelCapture();return;}
      if(!native){setWindowFocused(true);return;}
      // A background WebView can lag behind the engine. Fetch a fresh baseline
      // before accepting UI commands, and discard older in-flight polls.
      void invoke<Snapshot>('snapshot').then(current=>{
        if(disposed||epoch!==snapshotEpoch.current||!document.hasFocus()||document.hidden)return;
        gestureSerial.current=current.status.input_serial;
        setSnapshot(current);setWindowFocused(true);
      }).catch(()=>{});
    };
    window.addEventListener('focus',change);window.addEventListener('blur',change);document.addEventListener('visibilitychange',change);
    return()=>{disposed=true;window.removeEventListener('focus',change);window.removeEventListener('blur',change);document.removeEventListener('visibilitychange',change);};
  },[native,cancelCapture]);
  useEffect(()=>{
    if(autoNavigation.current||!gamepadPreference||!windowFocused||!devicePath||!snapshot?.engine_ready||busy||!status||!('ui_navigation' in status))return;
    if(status.state==='idle'){autoNavigation.current=true;enableGamepad();}
  },[gamepadPreference,windowFocused,devicePath,snapshot?.engine_ready,busy,status?.state]);
  useEffect(()=>{gestureSerial.current=status?.input_serial??0;},[snapshot?.restart_count]);
  useEffect(()=>{
    const serial=status?.input_serial??0;if(serial<=gestureSerial.current)return;gestureSerial.current=serial;
    if(!windowFocused||busy||calibrating||status?.motion?.measuring)return;
    const chord=status?.input_chord??[];
    if(chord.includes('back')&&chord.includes('start')){
      if(capture&&status?.ui_navigation){cancelCapture();notify('录入已取消');}
      else if(!capture&&!status?.binding_capture&&!status?.ui_navigation)enableGamepad();
    }
  },[status?.input_serial,windowFocused,busy,capture?.kind]);

  const revision = status?.revision;
  const runningProfile = status?.profile_id;
  useEffect(() => {
    if (!native || !runningProfile || !revision) return;
    let disposed = false;
    void invoke<Workspace>('workspace').then(data => {
      if (disposed) return;
      const current = draftRef.current;
      const entry = data.profiles.find(item => item.id === current?.id);
      setWorkspace(data);
      if (entry && !dirtyRef.current) setDraft(structuredClone(entry));
    }).catch(error => notify(String(error), true));
    return () => { disposed = true; };
  }, [native, notify, revision, runningProfile]);

  useEffect(() => {
    if (page !== 'settings' || !native) return;
    let disposed = false;
    void invoke<ComponentStatus>('component_status').then(data => {
      if (!disposed) setComponents(data);
    }).catch(error => { if (!disposed) notify(String(error), true); });
    return () => { disposed = true; };
  }, [native, notify, page]);

  const update = useCallback((change: (profile: Profile) => void) => {
    if(measuringRef.current)return;
    setDraft(current => {
      if (!current) return current;
      const entry = structuredClone(current);
      updateLayer(entry.profile,editingLayer,change);
      return entry;
    });
  }, [editingLayer]);

  const updateEditor = (patch: Partial<Action>) => {
    setEditor(current => current && { ...current, action: { ...current.action, ...patch } });
  };

  const finishCapture = useCallback((target: ActiveCapture, value: string) => {
    if ('gyroField' in target) {
      update(profile => { profile.gyro[target.gyroField] = value; });
    } else {
      const patch = target.kind === 'key' ? { output: value,...(!chordParts(value).some(v=>v.startsWith('wheel:'))?{repeat_ms:null}:{}) } : { input: value };
      const index = target.index;
      if (index === null) setEditor(current => current && { ...current, action: { ...current.action, ...patch } });
      else update(profile => {
        const action = profile.actions[index];
        if (action) Object.assign(action, patch);
      });
    }
    cancelCapture();
    notify(target.kind === 'key' ? '已录入键鼠操作' : '已识别手柄输入');
  }, [notify, update, cancelCapture]);

  const assignEscapeCapture=()=>{
    if(capture?.kind!=='key')return;
    const modifiers=(capture.preview??[]).filter(value=>modifierOutputs.includes(value));
    if(modifiers.length<4)finishCapture(capture,[...modifiers,'key:ESC'].join('+'));
  };

  useEffect(() => {
    const serial = status?.input_serial ?? 0;
    const edge = status?.last_input && serial > previousInput.current ?
      (status.input_chord?.length ? status.input_chord : [status.last_input]) : null;
    previousInput.current = Math.max(previousInput.current,serial);
    if (capture?.kind !== 'pad' || !status) return;
    const lostGyroCapture = 'gyroField' in capture && (!status.binding_capture ||
      status.device?.id !== capture.deviceId || snapshot?.restart_count !== capture.restartCount);
    if (lostGyroCapture || ['faulted','calibrating'].includes(status.state) || padPreview.current.length && !status.device) {
      padPreview.current = [];
      cancelCapture();
      notify('手柄连接或观察状态已改变，录入已取消。', true);
      return;
    }
    if ('gyroField' in capture) {
      const neutral = gyroInputNeutral(status);
      if (capture.waitingForNeutral) {
        if (neutral) setCapture({...capture, waitingForNeutral:false});
        return;
      }
      if (edge) padPreview.current = [...new Set([...padPreview.current, ...edge])];
      const candidates = padBindingCandidates(padPreview.current, capture.split);
      if (candidates.length && neutral) {
        padPreview.current = [];
        const blocked = capture.gyroField === 'activation' ? profile?.gyro.pause_input : profile?.gyro.activation;
        if (candidates.length > 1 || candidates[0] === blocked) {
          setCapture({...capture, preview:[], issue:candidates.length > 1 ? 'multiple' : 'duplicate'});
        } else finishCapture(capture, candidates[0]);
      } else if (edge) setCapture({...capture, preview:candidates, issue:undefined});
      return;
    }
    if (edge) {
      if (edge.length <= 4) { padPreview.current = edge; setCapture({...capture, preview:edge}); }
      else { padPreview.current = []; notify('组合键最多支持 4 个手柄输入，请松开后重新录入。', true); }
    }
    const pressed = heldPadInputs(status);
    if (padPreview.current.length && padPreview.current.every(value => !pressed.has(value))) {
      const value = padPreview.current.join('+');
      padPreview.current = [];
      finishCapture(capture, value);
    }
  }, [capture,status,snapshot?.restart_count,profile?.gyro.activation,profile?.gyro.pause_input,finishCapture,notify,cancelCapture]);

  const capturing=!!capture;
  useEffect(()=>{
    if(!capturing)return;
    const generation = captureGeneration.current;
    const timer=window.setInterval(()=>{void Promise.all([invoke('set_shortcut_capture',{active:true}),bindingCapture(true)]).catch(error=>{if(generation===captureGeneration.current){cancelCapture();notify(String(error),true);}});},1000);
    return()=>{window.clearInterval(timer);void bindingCapture(false).catch(()=>{});void invoke('set_shortcut_capture',{active:false}).catch(()=>{});};
  },[capturing,notify,bindingCapture,cancelCapture]);

  useEffect(() => {
    if (!capture) return;
    const target=capture;const held=new Set<string>();let best:string[]=[];let invalid=false;let finished=false;let timer:number|undefined;
    const commit=(value:string,delay=0)=>{
      if(finished)return;finished=true;
      if(delay)timer=window.setTimeout(()=>finishCapture(target,value),delay);else finishCapture(target,value);
    };
    const mods=(event:KeyboardEvent|MouseEvent)=>syncRecordedModifiers(held,event);
    const preview=()=>{
      if(held.size>4){invalid=true;notify('组合输出最多支持 4 个按键或鼠标操作，请松开后重试。',true);return;}
      if(held.size>=best.length)best=[...held];
      setCapture(c=>c?.kind==='key'?{...c,preview:best}:c);
    };
    const settle=()=>{
      if(held.size)return;
      if(invalid){invalid=false;best=[];setCapture(c=>c?.kind==='key'?{...c,preview:[]}:c);return;}
      if(best.length)commit(best.join('+'),best.some(v=>v.startsWith('mouse:'))?40:0);
    };
    const keyValue=keyboardKeyFromEvent;
    const keyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        finished=true;
        cancelCapture();
        return;
      }
      if (capture.kind !== 'key' || event.repeat) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      if(finished)return;
      const key=keyValue(event);
      if(keyNames.includes(key)&&!event.isComposing){held.add('key:'+key);mods(event);preview();}
      else {invalid=true;notify('此按键暂不支持。F12 保留用于紧急停止，Esc 可取消录入。',true);}
    };
    const keyUp=(event:KeyboardEvent)=>{
      if(target.kind!=='key')return;event.preventDefault();event.stopImmediatePropagation();
      if(finished)return;
      held.delete('key:'+keyValue(event));mods(event);settle();
    };
    const mouseNames=['mouse:left','mouse:middle','mouse:right','mouse:x1','mouse:x2'];
    const mouse=(event:MouseEvent)=>{
      if(target.kind!=='key'||(event.target instanceof Element&&event.target.closest('[data-cancel-capture],button[data-dialog-close]')))return;
      event.preventDefault();event.stopImmediatePropagation();if(finished)return;mods(event);
      const name=mouseNames[event.button];if(!name)return;
      if(event.type==='mousedown'){held.add(name);preview();}else{held.delete(name);settle();}
    };
    const wheel=(event:WheelEvent)=>{
      if(target.kind!=='key')return;event.preventDefault();event.stopImmediatePropagation();if(finished)return;mods(event);
      if(!event.deltaX&&!event.deltaY)return;
      const part=Math.abs(event.deltaX)>Math.abs(event.deltaY)?(event.deltaX>0?'wheel:right':'wheel:left'):(event.deltaY>0?'wheel:down':'wheel:up');
      const parts=[...held,part];if(parts.length>4||invalid){notify('组合输出最多支持 4 个操作，请松开后重试。',true);return;}
      commit(parts.join('+'));
    };
    const preventDefault=(event:Event)=>{if(target.kind==='key'){event.preventDefault();event.stopImmediatePropagation();}};
    window.addEventListener('keydown', keyDown, true);
    window.addEventListener('keyup',keyUp,true);window.addEventListener('mousedown',mouse,true);window.addEventListener('mouseup',mouse,true);
    window.addEventListener('wheel',wheel,{capture:true,passive:false});window.addEventListener('auxclick',preventDefault,true);window.addEventListener('contextmenu',preventDefault,true);
    return () => {window.removeEventListener('keydown',keyDown,true);window.removeEventListener('keyup',keyUp,true);window.removeEventListener('mousedown',mouse,true);window.removeEventListener('mouseup',mouse,true);window.removeEventListener('wheel',wheel,true);window.removeEventListener('auxclick',preventDefault,true);window.removeEventListener('contextmenu',preventDefault,true);if(timer!==undefined)window.clearTimeout(timer);};
  }, [capture?.kind,capture?.index,finishCapture,notify,cancelCapture]);

  const navigate = (target: StudioPage, tab: GameTab = 'actions', showCalibration = false) => {
    if (measuringRef.current && target !== 'games') {
      notify('请先结束或取消当前转角测量', true);
      return;
    }
    cancelCapture();
    if (target === 'games') setGameTab(measuringRef.current ? 'motion' : tab);
    setRevealDeviceCalibration(showCalibration);
    setPage(target);
  };
  function enableGamepad() {
    if(!native||busy||calibrating)return;
    if(!devicePath){navigate('controllerStatus');notify('请先选择一个手柄接口',true);return;}
    autoNavigation.current=true;setGamepadPreference(true);localStorage.setItem('motion-studio-gamepad-ui','on');
    void run(async()=>{await invoke('set_ui_navigation',{active:true,devicePath});setSnapshot(await invoke<Snapshot>('snapshot'));},'手柄操作已启用，游戏映射输出已暂停');
  }
  function disableGamepad() {
    autoNavigation.current=true;setGamepadPreference(false);localStorage.setItem('motion-studio-gamepad-ui','off');
    void run(async()=>{await invoke('set_ui_navigation',{active:false,devicePath});setSnapshot(await invoke<Snapshot>('snapshot'));});
  }
  function selectDevice(path:string) {
    setDevicePath(path);
    if(status?.ui_navigation&&path&&path!==devicePath)void run(async()=>{await invoke('set_ui_navigation',{active:true,devicePath:path});setSnapshot(await invoke<Snapshot>('snapshot'));});
  }
  function openJsmImport() {
    if(busy||calibrating||status?.motion?.measuring)return;
    if(dirty){notify('请先保存或撤销当前配置的修改。',true);return;}
    cancelCapture();setImportingJsm(true);
  }
  const selectProfile = (entry: Entry) => {
    if (entry.id === draft?.id || measuringRef.current) return;
    cancelCapture();
    if (dirty) setConfirmDiscard(entry);
    else setDraft(structuredClone(entry));
  };

  async function saveDraft() {
    if(measuringRef.current)throw new Error('请先结束或取消转角测量，再保存配置');
    if (!draft) throw new Error('请先选择配置');
    const result = await invoke<SaveResult>('save_profile', { entry: draft });
    setWorkspace(data => data && { ...data, profiles: data.profiles.map(entry => entry.id === result.entry.id ? result.entry : entry) });
    setDraft(current => current === draft ? structuredClone(result.entry) : current);
    setFailedApplyId(current => result.warning ? result.entry.id : current === result.entry.id ? null : current);
    notify(result.warning ? '配置已保存，应用失败：' + result.warning : result.applied ? '配置已保存并应用' : '配置已保存', !!result.warning);
    return result;
  }

  async function applyProfile() {
    if(measuringRef.current)throw new Error('请先结束或取消转角测量，再应用配置');
    if (!draft || !canApply) return;
    if (dirty) {
      const result = await saveDraft();
      if (result.applied || result.warning) return;
    }
    await invoke('activate_profile', { id: draft.id });
    setFailedApplyId(null);
    await refresh();
    notify(active ? '已应用当前配置' : '已设为下次启用输出的配置');
  }

  async function syncEngineSnapshot() {
    const epoch = ++snapshotEpoch.current;
    const current = await invoke<Snapshot>('snapshot');
    if (epoch === snapshotEpoch.current) setSnapshot(current);
  }

  async function startMapping(emit: boolean) {
    if (!draft) return false;
    if (!devicePath) {
      navigate('controllerStatus');
      notify('请先选择一个手柄接口', true);
      return false;
    }
    if (device?.origin === 'virtual') throw new Error('请选择实体手柄作为输入');
    const configured = emit ? draft.profile : saved?.profile ?? draft.profile;
    if (configured.gyro.enabled && device && !device.gyro) {
      throw new Error('当前接口没有体感，请在「设备与输入」选择支持体感的接口，或关闭配置的体感。');
    }
    if (emit && dirty) await saveDraft();
    await invoke('start_mapping', { id: draft.id, devicePath, emit });
    await syncEngineSnapshot();
    return true;
  }

  function startCapture(kind: CaptureKind, index: number | null) {
    beginCapture({kind, index});
  }

  function beginCapture(target: CaptureRequest) {
    if (busy || calibrating || status?.motion?.measuring) return;
    if (capture) { cancelCapture(); return; }
    const gyro = 'gyroField' in target;
    if (gyro && (!canRecordGyro || status?.binding_capture)) return;
    if (target.kind === 'pad' && !devicePath) {
      if (target.index === null) setEditor(null);
      navigate('controllerStatus');
      notify('请先选择一个手柄接口', true);
      return;
    }
    const generation = ++captureGeneration.current;
    void run(async () => {
      let leased = false, listening = false;
      try {
        await invoke('set_shortcut_capture', {active:true});
        if (generation !== captureGeneration.current) return;
        if (target.kind === 'pad' && !active && !status?.ui_navigation && !(await startMapping(false))) return;
        if (generation !== captureGeneration.current) return;
        leased = true;
        await bindingCapture(true);
        if (generation !== captureGeneration.current) return;
        // Reject older polls taken before capture paused the engine.
        snapshotEpoch.current++;
        const current = await invoke<Snapshot>('snapshot');
        if (generation !== captureGeneration.current) return;
        const liveDevice = current.status.device;
        if ('gyroField' in target && (!current.status.binding_capture || !liveDevice || liveDevice.path.toLowerCase() !== devicePath.toLowerCase())) {
          throw new Error('所选手柄未连接，无法录入体感输入。');
        }
        setSnapshot(current);
        previousInput.current = current.status.input_serial;
        padPreview.current = [];
        if ('gyroField' in target && liveDevice) {
          setCapture({...target, waitingForNeutral:!gyroInputNeutral(current.status), deviceId:liveDevice.id,
            restartCount:current.restart_count, split:(liveDevice.touchpads ?? 0) >= 2});
        } else if (!('gyroField' in target)) setCapture(target);
        listening = true;
      } finally {
        if (!listening) await Promise.allSettled([
          ...(leased ? [bindingCapture(false)] : []), invoke('set_shortcut_capture', {active:false}),
        ]);
      }
    });
  }

  function openCreate(initial: Template = 'blank') {
    if (!workspace || busy || calibrating || measuringRef.current) return;
    if (dirty) { notify('请先保存或撤销当前配置的修改。', true); return; }
    cancelCapture();
    setNotice(null);
    setTemplate(initial);
    setNewName(status?.ui_navigation?'新游戏配置 '+(entries.length+1):'');
    setCreating(true);
  }

  function openEditor(index: number | null, preset?: Partial<Action>) {
    cancelCapture();
    setNotice(null);
    const action = index === null
      ? { name: t('新动作'), input: 'south', output: '', threshold: 0.5, ...preset }
      : profile?.actions[index];
    if (action) setEditor({ index, action: { ...action } });
  }

  function closeEditor() { cancelCapture(); setEditor(null); }

  function finishEditing() {
    if (!editor || !editor.action.name.trim() || !editor.action.input || !editor.action.output) return;
    const { action, index } = editor;
    if(action.repeat_ms!=null&&(!Number.isInteger(action.repeat_ms)||action.repeat_ms<20||action.repeat_ms>2000)){
      notify('滚动间隔需为 20–2000 ms 的整数。',true);return;
    }
    update(profile => {
      if (index === null) profile.actions.push(action);
      else profile.actions[index] = action;
    });
    closeEditor();
  }

  function calibrate() {
    if (!canCalibrate) {
      navigate('controllerStatus');
      notify('请先在「设备与输入」开始观察体感，再静止校准。');
      return;
    }
    void run(() => invoke('engine_action', { action: 'calibrate' }));
  }

  function motionAction(action: string, axis?: number, positive?: boolean) {
    void run(() => invoke('motion_action', {action, axis:axis??null, positive:positive??null}));
  }

  async function changeSettings(patch: Partial<Settings>) {
    if (!workspace) return;
    const settings = await invoke<Settings>('save_settings', { settings: { ...workspace.settings, ...patch } });
    setWorkspace(current => current && { ...current, settings });
  }

  async function exportProfile(entry: Entry) {
    if (entry.id === draft?.id && dirty) await saveDraft();
    const defaultPath = entry.profile.name.replace(/[<>:"/\\|?*]/g, '-') + '.toml';
    const path = await save({ title: t('导出配置'), defaultPath, filters: [{ name: t('Motion Studio 配置'), extensions: ['toml'] }] });
    if (path) await invoke('export_profile', { id: entry.id, path });
  }
  async function importSharedProfile(source: SharedSource) {
    const entry = await invoke<Entry>('import_shared_profile', {source});
    setWorkspace(current => current && {...current, profiles: [...current.profiles, entry]});
    if (!dirtyRef.current && !measuringRef.current) setDraft(structuredClone(entry));
    setLibraryView('local');
    notify('已添加到本地配置');
  }

  async function exportDiagnostics() {
    const path = await save({ title: t('导出诊断'), defaultPath: 'motion-studio-diagnostics.json', filters: [{ name: 'JSON', extensions: ['json'] }] });
    if (path) await invoke('export_diagnostics', { path });
  }

  async function refreshComponents() {
    setComponents(await invoke<ComponentStatus>('component_status'));
  }

  function openLocation(kind: string) { void run(() => invoke('open_location', { kind })); }
  function stop() {
    void run(async () => {
      await invoke('engine_action', { action: 'stop' });
      await syncEngineSnapshot();
    });
  }
  function componentAction(action: ComponentAction, deviceId = '', xinput = false) {
    void run(async () => {
      await invoke('component_action', { action, deviceId, xinput });
      await refreshComponents();
    });
  }

  const locked = busy || calibrating;
  const dialogError = notice?.error ? notice.text : undefined;
  const shellStatus = dirty ? '保存到文件，或应用为当前配置。' :
    pending ? '点击应用修改可切换当前配置。' : status?.message || '连接设备后可开始测试。';

  return <>
    <StudioShell page={page} gameTab={gameTab} onNavigate={navigate} onGameTab={tab=>navigate('games',tab)}
      profiles={entries.map(entry => ({ id: entry.id, name: entry.id === draft?.id ? draft.profile.name : entry.profile.name }))}
      profileId={draft?.id ?? ''} onSelectProfile={id => { const entry = entries.find(item => item.id === id); if (entry) selectProfile(entry); }}
      layerControls={draft&&<LayerToolbar key={draft.id} profile={draft.profile} profileId={draft.id} selected={editingLayer} onSelect={setEditingLayer}
        blocked={locked||!!capture||!!editor||!!status?.motion?.measuring} status={status} onUpdate={change=>setDraft(current=>{if(!current)return current;const entry=structuredClone(current);change(entry.profile);return entry;})}/>}
      onNewProfile={() => openCreate()} onOpenFolder={() => openLocation('profiles')} onHelp={() => setHelp(true)}
      onImportJsm={openJsmImport} onGamepad={()=>status?.ui_navigation?disableGamepad():enableGamepad()}
      gamepadOn={!!status?.ui_navigation} gamepadAvailable={!!devicePath&&!!snapshot?.engine_ready}
      outputOn={!!status?.emitting} waiting={waiting} preparing={status?.state==='preparing_output'}
      outputDisabled={!snapshot?.engine_ready || (!draft && !status?.emitting && !waiting)}
      onStartMapping={() => void run(() => startMapping(true))} onStopMapping={stop}
      calibrating={calibrating} busy={busy} dirty={dirty} pending={pending}
      canApply={canApply} deviceCount={deviceCount} status={shellStatus} capturing={captureActive}
      language={workspace?.settings.language??getLanguage()} languageDisabled={busy||!workspace||!!capture}
      onLanguage={language=>void run(async()=>{await changeSettings({language});setLanguage(language);})}
      onSave={() => { if (dirty && !locked && !captureActive) void run(saveDraft); }}
      onApply={() => { if (!locked && !captureActive && canApply) void run(applyProfile); }}
      onDiscard={() => { if (saved) { setDraft(structuredClone(saved)); cancelCapture(); } }}>
      {notice && !(notice.error && (creating || editor || deleting)) &&
        <div className={'studio-toast' + (notice.error ? ' studio-toast-error' : '')} role={notice.error ? 'alert' : 'status'}>{notice.text}</div>}
      {workspace?.warnings.map((warning, index) => <div className="studio-note studio-error" key={index}>{warning}</div>)}
      {(status?.reload_error || snapshot?.policy_error) && <div className="studio-note studio-error" role="alert">{status?.reload_error || snapshot?.policy_error}</div>}

      {page === 'overview' && <StudioOverview profile={profile} status={status} device={device} deviceCount={deviceCount} onNavigate={navigate} />}
      {!native && <p className="studio-note">请在 Motion Studio 桌面应用中连接引擎和设备。</p>}
      {native && !workspace && page !== 'overview' && <p className="studio-note">正在打开工作区… <button type="button" className="secondary-btn" onClick={() => void run(() => refresh())}>重试</button></p>}

      {page === 'games' && workspace && (profile && draft
        ? <GameProfilesPage key={draft.id+':'+editingLayer} layerMode={!!editingLayer} tab={gameTab} profile={profile} device={device} status={status}
            blocked={locked} capture={actionRecording} onCapture={startCapture} onCancelCapture={cancelCapture} onAssignEscape={assignEscapeCapture}
            onUpdate={update} onEdit={openEditor} onTab={tab=>navigate('games',tab)}
            onEditBase={()=>{cancelCapture();setEditingLayer('');}} onImportJsm={openJsmImport}
            motion={<MotionSettings profile={profile} profileId={draft.id} update={update} device={device}
              deviceNoise={workspace.settings.motion_calibrations[devicePath]?.gyro_noise_dps}
              recording={gyroRecording} canRecord={canRecordGyro}
              onRecord={gyroField=>beginCapture({kind:'pad',index:null,gyroField})} onCancelRecording={cancelCapture}
              onOpenDevice={()=>navigate('controllerStatus','actions',true)} onPreviewOutput={()=>navigate('controllerStatus')} status={status}
              canMeasure={!!status?.emitting&&!pending&&!captureActive&&runningProfile===draft.id}
              onMotionAction={motionAction}/>}
            onExport={() => void run(() => exportProfile(draft))} onOpenFolder={() => openLocation('profiles')} onNavigate={navigate} />
        : <section className="app-card"><h2>创建你的第一份游戏配置</h2>
            <p className="studio-muted">选择合适的模板后，就可以按游戏动作配置手柄。</p>
            <div className="studio-actions"><button type="button" className="primary-btn" onClick={() => openCreate('keyboard')}>新建游戏配置</button>
            <button type="button" className="secondary-btn" onClick={openJsmImport}>导入 JSM 配置</button>
            <button type="button" className="secondary-btn" onClick={() => navigate('profiles')}>配置库</button></div>
          </section>)}
      {page === 'controllerStatus' && <Suspense fallback={<p className="studio-muted">正在载入设备状态…</p>}>
        <DevicePage snapshot={snapshot} devicePath={devicePath} device={device} active={active} revealCalibration={revealDeviceCalibration}
        busy={busy} canObserve={canObserve} onSelect={selectDevice} onObserve={() => void run(() => startMapping(false))}
        calibration={device?.gyro && workspace && <DeviceCalibration key={device.path} device={device} status={status}
          sensor={workspace.settings.motion_calibrations[devicePath]} canCalibrate={canCalibrate}
          calibrating={calibrating&&status?.device?.path===devicePath} blocked={locked||captureActive||!!status?.motion?.measuring}
          onCalibrate={calibrate} onMotionAction={motionAction}
          onSaveSensor={calibration=>void run(async()=>{await invoke('save_sensor_calibration',{devicePath,calibration});await refresh();},'设备校准已保存')}/>}
        onStop={stop} onScan={() => void run(() => invoke('engine_action', { action: 'scan' }))} onSettings={() => navigate('settings')} />
      </Suspense>}
      {page === 'profiles' && workspace && <ProfileLibrary entries={entries} selected={draft} activeId={active ? runningProfile : null}
        blocked={locked} dirty={dirty} view={libraryView} onView={setLibraryView}
        onChooseCommunity={community => setSharedImport({community})} onImportCode={() => setSharedImport({})}
        onContribute={() => openLocation('community')}
        onCopyCode={entry => void run(() => invoke('copy_profile_code', {profile: entry.profile}), '配置码已复制')}
        onSelect={selectProfile} onRename={name => update(profile => { profile.name = name; })}
        onCreate={() => openCreate()} onImport={() => {
          if (dirty) notify('请先保存或撤销当前配置的修改。', true);
          else setImportingToml(true);
        }} onImportJsm={openJsmImport} onExport={entry => void run(() => exportProfile(entry))} onDelete={entry => { setNotice(null); setDeleting(entry); }}
        onRestore={entry => void run(async () => { await invoke('restore_backup', { id: entry.id }); await refresh(entry.id); }, '已恢复上一份有效配置')}
        onSwitching={() => navigate('switching')} onFolder={() => openLocation('profiles')} />}
      {page === 'switching' && workspace && <SwitchSettings entries={entries} settings={workspace.settings} busy={busy}
        onChange={patch => void run(() => changeSettings(patch))} status={status} onPadCapture={bindingCapture}
        onPreparePad={async()=>{
          if(calibrating||status?.motion?.measuring)throw new Error('请先结束校准或转角测量');
          if(!active&&!status?.ui_navigation&&!(await startMapping(false)))throw new Error('请先选择一个手柄接口');
          await bindingCapture(true);
          snapshotEpoch.current++;
          const current=await invoke<Snapshot>('snapshot');setSnapshot(current);return current.status;
        }} />}
      {page === 'settings' && workspace && <SystemPage components={components} snapshot={snapshot} dataDir={workspace.data_dir}
        backend={workspace.settings.output_backend ?? 'hid_maestro'} outputActive={active}
        gamepadBackend={workspace.settings.gamepad_backend??'vigem'}
        onGamepadBackend={backend=>void run(()=>changeSettings({gamepad_backend:backend}),'虚拟手柄后端已保存，下次开始映射时生效')}
        autoHide={workspace.settings.auto_hide !== false}
        onAutoHide={enabled => void run(() => changeSettings({auto_hide: enabled}), '自动隐藏设置已保存，下次开始映射时生效')}
        onBackend={backend => void run(() => changeSettings({output_backend: backend}), '输出后端已保存，下次开始映射时生效')}
        busy={busy} onRefresh={() => void run(refreshComponents)} onOpen={openLocation} onComponent={componentAction}
        onInstall={kind => void run(async () => {
          const result = await invoke<{ cancelled: boolean; rebootRequired: boolean }>('install_component', { kind });
          await refreshComponents();
          notify(result.cancelled ? '已取消安装' : result.rebootRequired ? '安装完成，请重启 Windows 后使用' : kind === 'hidmaestro' ? 'HIDMaestro 驱动已安装' : '安装向导已完成');
        })}
        onGameConfig={()=>navigate('games')}
        onRecover={() => void run(() => invoke('engine_action', { action: 'recover' }), '引擎已重启，请手动启用输出')}
        onRestore={() => void run(async () => { await invoke('restore_backup', { id: 'settings' }); await refresh(); }, '已恢复设置备份')}
        onDiagnostics={() => void run(exportDiagnostics)} onDevices={() => navigate('controllerStatus')} />}
    </StudioShell>

    <GamepadNavigation status={status} foreground={windowFocused} blocked={locked||captureActive||!!status?.motion?.measuring} canSave={dirty} canPlay={canObserve&&!locked}
      onSave={()=>{if(dirty&&!locked)void run(saveDraft);}} onPlay={()=>void run(()=>startMapping(true))} onDisable={disableGamepad}/>
    {importingJsm&&<ImportJsm onClose={()=>setImportingJsm(false)} onImported={async entry=>{
      await refresh(entry.id);
      setImportingJsm(false);
      navigate('games');
      notify('已导入为新的游戏配置，请核验转换结果后启用输出');
    }}/>}
    {importingToml&&<ImportFiles kind="toml" onClose={()=>setImportingToml(false)} onSelect={content=>{setImportingToml(false);void run(async()=>{const entry=await invoke<Entry>('import_profile',{content});await refresh(entry.id);},'已导入为一份新配置');}}/>}
    {sharedImport && <ProfileShareDialog community={sharedImport.community} onClose={() => setSharedImport(null)}
      onImport={importSharedProfile}/>}
    {creating && <Dialog title="新建配置" description="选择一个起点，为游戏建立独立配置。" onClose={() => setCreating(false)} error={dialogError}
      onSubmit={() => void run(async () => {
        const entry = await invoke<Entry>('create_profile', { name: newName.trim(), template });
        await refresh(entry.id);
        setCreating(false);
        navigate('games');
      }, '配置已创建')}
      actions={<>
        <button type="button" className="secondary-btn" onClick={() => setCreating(false)}>取消</button>
        <button type="submit" className="primary-btn" disabled={busy || !newName.trim()}>创建配置</button>
      </>}>
      <label className={dialogStyles.field}>游戏或配置名称
        <input autoFocus required maxLength={100} value={newName} onChange={event => setNewName(event.target.value)} />
      </label>
      <fieldset className={dialogStyles.templates}>
        <legend>配置模板</legend>
        {templates.map(([id, title, description]) => <label key={id} className={dialogStyles.template}>
          <input type="radio" name="template" value={id} checked={template === id} onChange={() => setTemplate(id)} />
          <span><strong>{title}</strong><small>{description}</small></span>
        </label>)}
      </fieldset>
    </Dialog>}
    {editor && <ActionEditor key={editingLayer+':'+String(editor.index)} action={editor.action} isNew={editor.index===null}
      onChange={updateEditor} onClose={closeEditor} onSubmit={finishEditing} error={dialogError} busy={busy}
      capture={actionRecording} onCapture={kind=>startCapture(kind,null)} onCancelCapture={cancelCapture} onAssignEscape={assignEscapeCapture}
      layerName={draft?.profile.layers?.find(layer=>layer.id===editingLayer)?.name} navigation={!!status?.ui_navigation}/>}
    {deleting && <Dialog title="删除这份配置？" onClose={() => setDeleting(null)} error={dialogError} actions={<>
      <button type="button" className="secondary-btn" onClick={() => setDeleting(null)}>取消</button>
      <button type="button" className="danger-btn" disabled={busy} onClick={() => void run(async () => {
        await invoke('delete_profile', { id: deleting.id });
        if (draftRef.current?.id === deleting.id) {
          draftRef.current = null;
          setDraft(null);
        }
        setDeleting(null);
        await refresh();
      }, '配置已删除')}>删除配置</button>
    </>}>
      <p>将删除「{deleting.profile.name}」、保存备份及引用它的自动切换规则。</p>
    </Dialog>}
    {confirmDiscard && <Dialog title="有未保存的修改" onClose={() => setConfirmDiscard(null)} actions={<>
      <button type="button" className="secondary-btn" onClick={() => setConfirmDiscard(null)}>返回编辑</button>
      <button type="button" className="primary-btn" onClick={() => { setDraft(structuredClone(confirmDiscard)); setConfirmDiscard(null); }}>丢弃并切换</button>
    </>}>
      <p>切换到「{confirmDiscard.profile.name}」将丢弃当前编辑。</p>
    </Dialog>}
    {help && <Dialog title="帮助与快捷键" onClose={() => setHelp(false)}
      actions={<button type="button" className="secondary-btn" onClick={() => setHelp(false)}>关闭</button>}>
      <dl className="studio-shortcuts">
        <div><dt><kbd>Ctrl K</kbd></dt><dd>搜索页面和操作</dd></div>
        <div><dt><kbd>Ctrl S</kbd></dt><dd>保存当前配置</dd></div>
        <div><dt><kbd>Ctrl ↵</kbd></dt><dd>应用当前配置</dd></div>
        <div><dt><kbd>{shortcutLabel(workspace?.settings.shortcut_key)}</kbd></dt><dd>切换到指定配置或返回，可在「自动切换」中修改</dd></div>
        <div><dt><kbd>F12</kbd></dt><dd>停止映射并释放输出</dd></div>
        <div><dt><kbd>方向 / 左摇杆</kbd></dt><dd>移动界面焦点；右摇杆滚动页面</dd></div>
        <div><dt><kbd>A / B</kbd></dt><dd>确认或编辑／返回或取消调整</dd></div>
        <div><dt><kbd>LB / RB</kbd></dt><dd>切换侧栏、内容、工具栏和底部操作区域</dd></div>
        <div><dt><kbd>X / Y / Menu</kbd></dt><dd>编辑当前控件／保存配置／手柄快捷操作</dd></div>
        <div><dt><kbd>View + Menu</kbd></dt><dd>窗口前台暂停映射并切回界面；手柄界面录入时取消录入</dd></div>
      </dl>
    </Dialog>}
  </>;
}
