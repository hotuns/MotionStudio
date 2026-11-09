import {useState} from 'react';
import {chordParts,inputLabel,outputLabel,type Action,type Device,type Profile,type Status} from '../types';
import Controller from './Controller';
import {gamepadNames} from './Controls';
import {triggerNames} from './BindingFields';
import {modifierOutputs} from '../keyboard';
import {MotionActionEditor,motionActionLabels,newMotionAction,type MotionAction} from './MotionActionEditor';
import styles from './GameProfilesPage.module.css';

export type CaptureKind = 'key' | 'pad';
export type Capture = {kind: CaptureKind; index: number | null; preview?: string[]};

export type ActionMappingsProps = {
  profile: Profile;
  device?: Device | null;
  status?: Status;
  blocked: boolean;
  capture: Capture | null;
  touchOnly: boolean;
  onClearTouch: () => void;
  onCapture: (kind: CaptureKind, index: number) => void;
  onCancelCapture: () => void;
  onAssignEscape: () => void;
  onUpdate: (change: (profile: Profile) => void) => void;
  onEdit: (index: number | null, preset?: Partial<Action>) => void;
};

const labelFor = (action: Action) => inputLabel(action.input);

export function ActionMappings({profile, status, device, blocked, capture, touchOnly, onClearTouch,
  onUpdate, onEdit, onCapture, onCancelCapture, onAssignEscape}: ActionMappingsProps) {
  const [query,setQuery] = useState('');
  const [selected,setSelected] = useState<number | null>(null);
  const [removing,setRemoving] = useState<string | null>(null);
  const [extra,setExtra] = useState<{action: MotionAction; index: number | null} | null>(null);
  const selection = selected === null ? undefined : profile.actions[selected];
  const search = query.trim().toLowerCase();
  const matches = (...values: string[]) => values.some(value=>value.toLowerCase().includes(search));
  const touches = (input: string) => chordParts(input).some(part=>['touchpad','touch1','touch2'].includes(part) ||
    (part==='misc2' && (device?.touchpads ?? 0)>=2));
  const rows = profile.actions.map((action,index)=>({action,index})).filter(({action})=>
    (!touchOnly || touches(action.input)) && matches(action.name,labelFor(action),outputLabel(action.output)));
  const motion = profile.gyro.motion;
  const motionRows: {action: MotionAction; index: number}[] = [
    ...motion.bindings.map((value,index)=>({action:{kind:'motion' as const,value},index})),
    ...motion.turns.map((value,index)=>({action:{kind:'turn' as const,value},index})),
  ];
  const visibleMotionRows = motionRows
    .filter(({action})=>!touchOnly || (action.kind==='turn' && touches(action.value.input)))
    .map(row=>({...row,labels:motionActionLabels(row.action)}))
    .filter(({action,labels})=>matches(action.value.name,labels.input,labels.output,labels.tag));
  const live = status?.device?.path === device?.path ? status : undefined;
  const virtual = profile.virtual_gamepad !== 'none';
  const edit = (index: number) => { setSelected(index); onEdit(index); };
  const addMotion = (kind: MotionAction['kind']) => { setQuery(''); setExtra({action:newMotionAction(kind),index:null}); };
  const rowActions = (key: string, name: string, edit: ()=>void, remove: ()=>void) =>
    <div className={styles.rowActions}>{removing===key ? <>
      <button type="button" className={styles.deleteButton} disabled={blocked||!!capture} onClick={()=>{remove();setRemoving(null);}}>确认删除</button>
      <button type="button" onClick={()=>setRemoving(null)}>取消</button>
    </> : <>
      <button type="button" disabled={blocked||!!capture} aria-label={'设置 '+name} onClick={edit}>设置</button>
      <button type="button" className={styles.deleteButton} disabled={blocked||!!capture}
        aria-label={'删除 '+name} onClick={()=>setRemoving(key)}>删除</button>
    </>}</div>;

  return <div className={styles.actionLayout}>
    <section className={styles.tableCard} aria-label="游戏动作">
      {capture?.index != null && <div className={styles.quickCapture} role="status">
        <span>{capture.kind === 'key' ? '按下组合键或鼠标按钮，全部松开后完成；也可滚动滚轮。' : '同时按住手柄按键或摇杆方向，全部松开后完成。'}
          {!!capture.preview?.length&&<strong> {capture.kind==='key'?outputLabel(capture.preview.join('+')):inputLabel(capture.preview.join('+'))}</strong>}</span>
        <div className={styles.captureActions}>
          {capture.kind==='key'&&<button type="button" className="secondary-btn" data-cancel-capture disabled={(capture.preview??[]).filter(value=>modifierOutputs.includes(value)).length>=4} onClick={onAssignEscape}>映射 Esc</button>}
          <button type="button" className="secondary-btn" data-cancel-capture onClick={onCancelCapture}>取消录入</button>
        </div>
      </div>}
      <div className={styles.filters}>
        <input type="search" aria-label="搜索动作" placeholder="搜索动作或按键…" value={query} onChange={event => setQuery(event.target.value)} />
        <div className={styles.actionTools}>
          {touchOnly && <button type="button" className="ghost-btn" onClick={onClearTouch}>清除触控筛选</button>}
          <button type="button" className="secondary-btn" disabled={blocked||!!capture||profile.actions.length>=128}
            onClick={()=>{setQuery('');onEdit(null,touchOnly?{input:'touchpad'}:undefined);}}>添加动作</button>
          {!touchOnly&&<>
            <button type="button" className="secondary-btn" disabled={blocked||!!capture||profile.gyro.motion.bindings.length>=64}
              onClick={()=>addMotion('motion')}>添加体感动作</button>
            <button type="button" className="secondary-btn" disabled={blocked||!!capture||profile.gyro.motion.turns.length>=32}
              onClick={()=>addMotion('turn')}>添加定角转身</button>
          </>}
        </div>
      </div>
      <div className={styles.tableScroll}>
        <table>
          <thead><tr><th>游戏动作</th><th>输出</th><th>输入</th><th>操作</th></tr></thead>
          <tbody>{rows.map(({ action, index }) => (
            <tr key={index} data-selected={selected === index}>
              <td><div className={styles.actionName}>
                <button type="button" translate="no" disabled={blocked||!!capture} onClick={() => edit(index)}>{action.name}</button>
                <span className={styles.actionBadges}>
                  <span className={styles.triggerBadge} data-trigger={action.trigger??'down'}>{triggerNames[action.trigger??'down']}</span>
                  {action.turbo_ms!=null&&<span className={styles.triggerBadge}>连发</span>}
                </span>
              </div></td>
              <td><button type="button" className={styles.bindingButton} disabled={blocked||!!capture} aria-label={'录入键鼠 ' + action.name}
                onClick={() => { setSelected(index); onCapture('key', index); }}><kbd>{outputLabel(action.output)}</kbd>{action.repeat_ms!=null&&<small> · 按住每 {action.repeat_ms} ms</small>}</button></td>
              <td><button type="button" className={styles.bindingButton} disabled={blocked||!!capture} aria-label={'录入手柄 ' + action.name}
                onClick={() => { setSelected(index); onCapture('pad', index); }}>{labelFor(action)}</button></td>
              <td>{rowActions('button:'+index, action.name, ()=>edit(index), ()=>{
                onUpdate(draft=>{draft.actions.splice(index,1);});
                onCancelCapture();
                setSelected(null);
              })}</td>
            </tr>
          ))}
          {visibleMotionRows.map(({action,index,labels})=><tr key={action.kind+':'+index}>
            <td><div className={styles.actionName}>
              <button type="button" translate="no" disabled={blocked||!!capture} onClick={()=>setExtra({action,index})}>{action.value.name}</button>
              <span className={styles.triggerBadge}>{labels.tag}</span>
            </div></td>
            <td><span>{labels.output}</span>{action.kind==='turn'&&<small translate="no"> · {action.value.degrees}°</small>}</td>
            <td>{labels.input}</td>
            <td>{rowActions(action.kind+':'+index,action.value.name,()=>setExtra({action,index}),()=>{
              onUpdate(draft=>{
                const motion=draft.gyro.motion;
                if(action.kind==='motion') motion.bindings.splice(index,1);
                else motion.turns.splice(index,1);
              });
            })}</td>
          </tr>)}</tbody>
        </table>
        {!rows.length && !visibleMotionRows.length && <p className={styles.hint + ' studio-table-empty'}>
          {search ? '没有匹配的动作。' : touchOnly ? '可以添加触摸板点按动作。' :
            virtual ? gamepadNames[profile.virtual_gamepad] + ' 自动映射已就绪，可添加额外键鼠动作。' : '添加第一个动作，例如“跳跃 → 空格 → A”。'}
        </p>}
      </div>
    </section>
    <aside className={styles.preview}>
      <h3>手柄预览</h3>
      <p className={styles.hint}>点击已绑定的手柄控件可编辑动作。</p>
      <Controller device={device} status={live} boundCommands={new Set(rows.flatMap(({action})=>chordParts(action.input)))}
        selectedCommand={selection?chordParts(selection.input)[0]:undefined} onSelectCommand={blocked||capture ? undefined : input => {
          const matching = rows.filter(row=>chordParts(row.action.input).includes(input));
          const next = matching.find(row => row.index !== selected) ?? matching[0];
          if (next) edit(next.index);
        }} />
      {selection && <p><span translate="no">{selection.name}</span> → <kbd>{outputLabel(selection.output)}</kbd></p>}
      <p className={styles.hint}>也可以直接点击表格中的键鼠操作或手柄输入开始录入。</p>
    </aside>
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
