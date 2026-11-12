import {useEffect,useRef,useState} from 'react';
import {chordParts,outputLabel,type Action,type Device,type GamepadBinding,type Profile,type Status} from '../types';
import {t} from '../i18n';
import Controller from './Controller';
import {triggerNames} from './BindingFields';
import {padInputLabel} from './binding-choices';
import {modifierOutputs} from '../keyboard';
import {isAnalogGamepadBinding} from '../mapping-triggers';
import type {GamepadBindingRecording} from '../pad-capture';
import {MotionActionEditor,motionActionLabels,newMotionAction,type MotionAction} from './MotionActionEditor';
import {
  fillStandardGamepadBindings, GamepadBindingEditor, gamepadControlLabel, isAnalogGamepadOutput,
  maxGamepadBindings, virtualGamepadControls,
} from './GamepadBindingEditor';
import styles from './GameProfilesPage.module.css';

export type CaptureKind = 'key' | 'pad';
export type Capture = {kind: CaptureKind; index: number | null; preview?: string[]};

export type ActionMappingsProps = {
  profile: Profile;
  device?: Device | null;
  status?: Status;
  blocked: boolean;
  capture: Capture | null;
  gamepadRecording?: GamepadBindingRecording;
  canRecordGamepad: boolean;
  touchOnly: boolean;
  onClearTouch: () => void;
  onCapture: (kind: CaptureKind, index: number) => void;
  onRecordGamepad: (wholeStick: boolean, onInput: (input: string) => void) => void;
  onCancelCapture: () => void;
  onAssignEscape: () => void;
  onBatch: (floating?:boolean) => void;
  onUpdate: (change: (profile: Profile) => void) => void;
  onEdit: (index: number | null, preset?: Partial<Action>) => void;
};

type MappingAction = {kind: 'binding'; value: Action} | {kind: 'gamepad'; value: GamepadBinding} | MotionAction;
type MappingRow = {action: MappingAction; index: number; input: string; output: string; tag: string};
type InputGroup = {input: string; rows: MappingRow[]};

const groupedControls = ['left_stick', 'right_stick', 'left_trigger', 'right_trigger'];
function physicalControl(input: string): string {
  if (input === 'touch1' || input === 'touch2') return 'touchpad';
  return groupedControls.find(control => input.startsWith(control + '_')) ?? input;
}
const actionCountLabel = (count: number) => count === 1
  ? t('1 个动作') : t('{0} 个动作').replace('{0}', String(count));

export function ActionMappings({profile, status, device, blocked, capture, gamepadRecording, canRecordGamepad,
  touchOnly, onClearTouch, onUpdate, onEdit, onCapture, onRecordGamepad, onCancelCapture, onAssignEscape, onBatch}: ActionMappingsProps) {
  const [query,setQuery] = useState('');
  const [controlFilter,setControlFilter] = useState<string | null>(null);
  const [selected,setSelected] = useState<number | null>(null);
  const [removing,setRemoving] = useState<string | null>(null);
  const [extra,setExtra] = useState<{action: MotionAction; index: number | null} | null>(null);
  const [gamepadEdit,setGamepadEdit] = useState<{value: GamepadBinding; index: number | null} | null>(null);
  const actionList = useRef<HTMLElement>(null);
  const clearFilter = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const list = actionList.current;
    const scrollArea = list?.closest('[data-pad-scroll]');
    if (!controlFilter || !list || !scrollArea) return;
    const start = list.getBoundingClientRect().top;
    const {top,bottom} = scrollArea.getBoundingClientRect();
    const headerOffset = parseFloat(getComputedStyle(list).scrollMarginBlockStart) || 0;
    if (start < top + headerOffset || start > bottom - 170) {
      clearFilter.current?.focus({preventScroll:true});
      list.scrollIntoView({block:'start', inline:'nearest'});
    }
  }, [controlFilter]);
  const selection = selected === null ? undefined : profile.actions[selected];
  const search = query.trim().toLowerCase();
  const matches = (...values: string[]) => values.some(value=>value.toLowerCase().includes(search));
  const split = (device?.touchpads ?? 0) >= 2;
  const controlLabel = (control: string) => t(control === 'left_stick' ? '左摇杆' :
    control === 'right_stick' ? '右摇杆' : control === 'touchpad' ? '触摸板' : padInputLabel(control, split));
  const inputLabel = (parts: string[]) => parts.map(part => part === 'left_stick' || part === 'right_stick'
    ? controlLabel(part) : t(padInputLabel(part, split))).join(' + ');
  const touches = (part: string) => physicalControl(part) === 'touchpad' || (split && part === 'misc2');
  const motion = profile.gyro.motion;
  const gamepadBindings = profile.gamepad_bindings ?? [];
  const gamepadOutputs = virtualGamepadControls(profile.virtual_gamepad);
  const canFillGamepad = fillStandardGamepadBindings(profile.virtual_gamepad, gamepadBindings).length
    > gamepadBindings.length;
  const actions: {action: MappingAction; index: number}[] = [
    ...gamepadBindings.flatMap((value,index)=>gamepadOutputs.includes(value.output)
      ? [{action:{kind:'gamepad' as const,value},index}] : []),
    ...profile.actions.map((value,index)=>({action:{kind:'binding' as const,value},index})),
    ...motion.turns.map((value,index)=>({action:{kind:'turn' as const,value},index})),
    ...motion.bindings.map((value,index)=>({action:{kind:'motion' as const,value},index})),
  ];
  const bindingCounts = new Map<string, number>();
  const groups = new Map<string, InputGroup>();
  for (const {action,index} of actions) {
    const parts = action.kind === 'motion' ? [] : chordParts(action.value.input);
    // A chord can use two inputs from the same stick. Count that action once per physical control.
    const controls = new Set(parts.map(physicalControl));
    for (const control of controls) bindingCounts.set(control, (bindingCounts.get(control) ?? 0) + 1);
    if ((touchOnly && !parts.some(touches)) || (controlFilter && !controls.has(controlFilter))) continue;

    const labels = action.kind === 'gamepad' ? {
      input: inputLabel(parts),
      output: gamepadControlLabel(action.value.output, profile.virtual_gamepad),
      tag: isAnalogGamepadBinding(action.value) ? '模拟量' : triggerNames[action.value.trigger??'down'],
    } : action.kind === 'binding'
      ? {input: parts.length ? inputLabel(parts) : t('待绑定手柄'), output: outputLabel(action.value.output), tag: triggerNames[action.value.trigger ?? 'down']}
      : motionActionLabels(action);
    const input = action.kind === 'turn' ? inputLabel(parts) : labels.input;
    const name = action.kind === 'gamepad' ? t('虚拟手柄映射') : action.value.name;
    if (!matches(name, input, action.value.input, labels.output, labels.tag, t(labels.tag))) continue;
    const key = action.kind === 'motion' ? 'motion:' + action.value.input : 'pad:' + parts.sort().join('+');
    const row = {action,index,input,output:labels.output,tag:labels.tag};
    const group = groups.get(key);
    if (group) group.rows.push(row);
    else groups.set(key, {input, rows:[row]});
  }
  const mappedControls = [...bindingCounts].sort((a,b) => b[1] - a[1]);
  const live = status?.device?.path === device?.path ? status : undefined;
  const virtual = profile.virtual_gamepad !== 'none';
  const locked = blocked || !!capture;
  const edit = (index: number) => { setSelected(index); onEdit(index); };
  const addMotion = (kind: MotionAction['kind']) => { setQuery(''); setExtra({action:newMotionAction(kind),index:null}); };
  function addGamepad() {
    const input = controlFilter ?? (touchOnly ? 'touchpad' : '');
    const reserved = isAnalogGamepadOutput(input) && gamepadBindings.some(binding => binding.output === input);
    const output = gamepadOutputs.includes(input) && !reserved ? input : '';
    setQuery('');
    setGamepadEdit({value:{input,output}, index:null});
  }
  function selectControl(control: string) {
    setControlFilter(current => current === control ? null : control);
    setQuery('');
    setSelected(null);
    setRemoving(null);
    if (touchOnly) onClearTouch();
  }
  const rowActions = (key: string, name: string, edit: ()=>void, remove: ()=>void) =>
    <div className={styles.rowActions}>{removing===key ? <>
      <button type="button" className={styles.deleteButton} disabled={blocked||!!capture} onClick={()=>{remove();setRemoving(null);}}>确认删除</button>
      <button type="button" onClick={()=>setRemoving(null)}>取消</button>
    </> : <>
      <button type="button" disabled={blocked||!!capture} aria-label={'设置 '+name} onClick={edit}>设置</button>
      <button type="button" className={styles.deleteButton} disabled={blocked||!!capture}
        aria-label={'删除 '+name} onClick={()=>setRemoving(key)}>删除</button>
    </>}</div>;

  function renderRow({action,index,input,output,tag}: MappingRow) {
    if (action.kind === 'gamepad') {
      const name = t('虚拟手柄映射');
      const open = () => setGamepadEdit({value:action.value,index});
      return <tr key={'gamepad:'+index}>
        <td><div className={styles.actionName}>
          <button type="button" disabled={locked} onClick={open}>{name}</button>
        </div></td>
        <td><button type="button" className={styles.bindingButton} disabled={locked} onClick={open}><kbd>{output}</kbd></button></td>
        <td><div className={styles.inputBinding}>
          <button type="button" className={styles.bindingButton} disabled={locked} onClick={open}>{input}</button>
          <span className={styles.triggerBadge}>{tag}</span>
          {action.value.turbo_ms!=null&&<span className={styles.triggerBadge}>连发</span>}
          {action.value.allow_simultaneous&&<span className={styles.triggerBadge}>同时触发</span>}
        </div></td>
        <td>{rowActions('gamepad:'+index, name, open, () => {
          onUpdate(draft => { draft.gamepad_bindings?.splice(index, 1); });
        })}</td>
      </tr>;
    }
    const {value,kind} = action;
    if (kind === 'binding') return <tr key={'binding:'+index} data-selected={selected === index}>
      <td><div className={styles.actionName}>
        <button type="button" translate="no" disabled={locked} onClick={() => edit(index)}>{value.name}</button>
        {value.turbo_ms!=null&&<span className={styles.triggerBadge}>连发</span>}
      </div></td>
      <td><button type="button" className={styles.bindingButton} disabled={locked} aria-label={'录入键鼠 ' + value.name}
        onClick={() => { setSelected(index); onCapture('key', index); }}><kbd>{output}</kbd>{value.repeat_ms!=null&&<small> · 按住每 {value.repeat_ms} ms</small>}</button></td>
      <td><div className={styles.inputBinding}>
        <button type="button" className={styles.bindingButton} disabled={locked} aria-label={'录入手柄 ' + value.name}
          onClick={() => { setSelected(index); onCapture('pad', index); }}>{input}</button>
        {value.input&&<span className={styles.triggerBadge} data-trigger={value.trigger??'down'}>{tag}</span>}
        {value.allow_simultaneous&&<span className={styles.triggerBadge}>同时触发</span>}
      </div></td>
      <td>{rowActions('binding:'+index, value.name, ()=>edit(index), ()=>{
        onUpdate(draft=>{draft.actions.splice(index,1);});
        onCancelCapture();
        setSelected(null);
      })}</td>
    </tr>;

    const open = () => setExtra({action,index});
    return <tr key={kind+':'+index}>
      <td><div className={styles.actionName}>
        <button type="button" translate="no" disabled={locked} onClick={open}>{value.name}</button>
        <span className={styles.triggerBadge}>{tag}</span>
      </div></td>
      <td><span>{output}</span>{action.kind==='turn'&&<small translate="no"> · {action.value.degrees}°</small>}</td>
      <td>{input}</td>
      <td>{rowActions(kind+':'+index,value.name,open,()=>{
        onUpdate(draft=>{
          const motion=draft.gyro.motion;
          if(kind==='motion') motion.bindings.splice(index,1);
          else motion.turns.splice(index,1);
        });
      })}</td>
    </tr>;
  }

  return <div className={styles.actionLayout}>
    <aside className={styles.preview} aria-label="手柄映射概览" data-pad-zone="mapping-overview">
      <h3>手柄映射概览</h3>
      <p className={styles.overviewHint}>数字表示当前编辑层中与该控件关联的自定义动作数。点控件可查看，包括组合键。</p>
      <div className={styles.overviewBody}>
        <Controller device={device} status={live} bindingCounts={bindingCounts}
          selectedControl={controlFilter ?? (selection && physicalControl(chordParts(selection.input)[0] ?? ''))}
          onSelectControl={locked ? undefined : selectControl} />
        {mappedControls.length > 0 ? <div className={styles.controlIndex} role="group" aria-label="有映射的手柄控件">
          {mappedControls.map(([control,count])=><button key={control} type="button" className={styles.controlChip}
            aria-label={controlLabel(control) + ' · ' + actionCountLabel(count)}
            disabled={locked} aria-pressed={controlFilter===control} onClick={()=>selectControl(control)}>
            <span>{controlLabel(control)}</span><span className={styles.chipCount}>{count}</span>
          </button>)}
        </div> : <p className={styles.hint}>添加手柄绑定后，这里会显示映射分布。</p>}
      </div>
    </aside>
    <section ref={actionList} className={styles.tableCard} aria-label="游戏动作" data-pad-zone="mapping-actions">
      {capture?.index != null && <div className={styles.quickCapture} role="status">
        <span>{capture.kind === 'key' ? '按下组合键或鼠标按钮，全部松开后完成；也可滚动滚轮。' : '同时按住手柄按键或摇杆方向，全部松开后完成。'}
          {!!capture.preview?.length&&<strong> {capture.kind==='key'?outputLabel(capture.preview.join('+')):inputLabel(capture.preview)}</strong>}</span>
        <div className={styles.captureActions}>
          {capture.kind==='key'&&<button type="button" className="secondary-btn" data-cancel-capture disabled={(capture.preview??[]).filter(value=>modifierOutputs.includes(value)).length>=4} onClick={onAssignEscape}>映射 Esc</button>}
          <button type="button" className="secondary-btn" data-cancel-capture onClick={onCancelCapture}>取消录入</button>
        </div>
      </div>}
      <div className={styles.filters}>
        <input type="search" aria-label="搜索动作" placeholder="搜索动作或按键…" value={query} onChange={event => setQuery(event.target.value)} />
        <div className={styles.actionTools}>
          {touchOnly && <button type="button" className="ghost-btn" onClick={onClearTouch}>清除触控筛选</button>}
          <button type="button" className="secondary-btn" disabled={locked||!profile.actions.length} onClick={()=>onBatch()}>批量填键</button>
          <button type="button" className="secondary-btn" disabled={locked} onClick={()=>onBatch(true)}>悬浮窗录入</button>
          <button type="button" className="secondary-btn" disabled={blocked||!!capture||profile.actions.length>=128}
            onClick={()=>{setQuery('');setControlFilter(null);onEdit(null,touchOnly?{input:'touchpad'}:undefined);}}>添加动作</button>
          {virtual&&<>
            <button type="button" className="secondary-btn" disabled={locked||gamepadBindings.length>=maxGamepadBindings}
              onClick={addGamepad}>添加手柄映射</button>
            {!touchOnly&&<button type="button" className="secondary-btn" disabled={locked||!canFillGamepad}
              onClick={()=>{
                onUpdate(draft=>{
                  draft.gamepad_bindings = fillStandardGamepadBindings(draft.virtual_gamepad, draft.gamepad_bindings ?? []);
                });
                setQuery('');
                setControlFilter(null);
              }}>填入标准布局</button>}
          </>}
          {!touchOnly&&<>
            <button type="button" className="secondary-btn" disabled={blocked||!!capture||profile.gyro.motion.bindings.length>=64}
              onClick={()=>addMotion('motion')}>添加体感动作</button>
            <button type="button" className="secondary-btn" disabled={blocked||!!capture||profile.gyro.motion.turns.length>=32}
              onClick={()=>addMotion('turn')}>添加定角转身</button>
          </>}
        </div>
      </div>
      {controlFilter && <div className={styles.controlFilter}>
        <span role="status">{t('正在查看 {0} 的映射').replace('{0}',controlLabel(controlFilter))}</span>
        <button ref={clearFilter} type="button" className="ghost-btn" onClick={()=>{setControlFilter(null);setSelected(null);}}>显示全部控件</button>
      </div>}
      <div className={styles.tableScroll}>
        <table>
          <thead><tr><th>游戏动作</th><th>输出</th><th>输入</th><th>操作</th></tr></thead>
          {[...groups].sort(([left],[right])=>Number(right==='pad:')-Number(left==='pad:')).map(([key,{input,rows}])=><tbody key={key}>
            <tr className={styles.inputGroup}><th scope="rowgroup" colSpan={4}>
              <strong>{input}</strong><span>{actionCountLabel(rows.length)}</span>
            </th></tr>
            {rows.map(renderRow)}
          </tbody>)}
        </table>
        {!groups.size && <p className={styles.hint + ' studio-table-empty'}>
          {search ? '没有匹配的动作。' : controlFilter ? '这个控件还没有自定义动作。' : touchOnly ? '可以添加触摸板点按动作。' :
            virtual ? '添加手柄映射，或填入标准布局。' : '添加第一个动作，例如“跳跃 → 空格 → A”。'}
        </p>}
      </div>
    </section>
    {gamepadEdit&&virtual&&<GamepadBindingEditor profile={profile}
      value={gamepadEdit.value} index={gamepadEdit.index} bindings={gamepadBindings}
      target={profile.virtual_gamepad} split={split} locked={locked}
      recording={gamepadRecording} canRecord={canRecordGamepad} onRecord={onRecordGamepad} onCancelRecording={onCancelCapture}
      onClose={()=>{onCancelCapture();setGamepadEdit(null);}}
      onSave={binding=>{
        onUpdate(draft=>{
          const bindings = draft.gamepad_bindings ??= [];
          if(gamepadEdit.index===null) bindings.push(binding);
          else bindings[gamepadEdit.index]=binding;
        });
        setGamepadEdit(null);
      }}/>}
    {extra&&<MotionActionEditor action={extra.action} blocked={blocked} onClose={()=>setExtra(null)}
      onSave={action=>{
        onUpdate(draft=>{
          const motion=draft.gyro.motion;
          if(action.kind==='motion') {
            if(extra.index===null) motion.bindings.push(action.value);
            else motion.bindings[extra.index]=action.value;
          } else {
            if(extra.index===null) motion.turns.push(action.value);
            else motion.turns[extra.index]=action.value;
          }
        });
        setExtra(null);
      }}/>}
  </div>;
}
