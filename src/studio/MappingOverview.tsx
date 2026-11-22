import {isSensorOutput,sensorOutputLabel} from '../sensor-passthrough';
import {useLayoutEffect,useRef,useState} from 'react';
import {t} from '../i18n';
import {chordParts,outputLabel,type Device,type Profile,type Status} from '../types';
import {isAnalogGamepadBinding} from '../mapping-triggers';
import {triggerNames} from './BindingFields';
import {padInputLabel} from './binding-choices';
import {gamepadControlLabel} from './GamepadBindingEditor';
import {motionActionLabels} from './MotionActionEditor';
import Controller from './Controller';
import {Dialog} from './Controls';
import {entriesForGroup,overviewGroups,type OverviewEntry,type OverviewGroup} from './controller-mapping-overview';
import styles from './MappingOverview.module.css';

function labels(entry:OverviewEntry,profile:Profile,split:boolean) {
  const inputLabel=(input:string)=>chordParts(input).map(part=>t(padInputLabel(part,split))).join(' + ');
  if(entry.kind==='setting') {
    const output=profile.gyro.output;
    const passthrough=isSensorOutput(output);
    const gyroOutput=t(passthrough?sensorOutputLabel(output):output==='mouse'?'鼠标':'虚拟右摇杆');
    if(entry.setting==='stick_mouse')return {input:t(entry.input==='left_stick'?'左摇杆':'右摇杆'),output:t('移动鼠标'),name:t('摇杆鼠标'),tags:[t('持续移动')]};
    if(entry.setting==='flick')return {input:t(entry.input==='left_stick'?'左摇杆':'右摇杆'),output:gyroOutput,name:t('快速转向'),tags:[t(profile.gyro.motion.flick.standalone?'独立启用':'随体感启用')]};
    return {input:t('体感'),output:gyroOutput,name:t(passthrough?sensorOutputLabel(output):'体感瞄准'),tags:[profile.gyro.activation==='always'?t('始终开启'):inputLabel(profile.gyro.activation)]};
  }
  if(entry.kind==='motion'||entry.kind==='turn') {
    const value=motionActionLabels(entry);
    return {...value,name:entry.value.name,tags:[value.tag,...(entry.kind==='turn'?[entry.value.degrees+'°']:[])]};
  }
  const value=entry.value,trigger=value.trigger??'down',analog=entry.kind==='gamepad'&&isAnalogGamepadBinding(value);
  return {input:inputLabel(value.input)||t('待绑定手柄'),
    output:entry.kind==='gamepad'?t('虚拟手柄')+' · '+gamepadControlLabel(value.output,profile.virtual_gamepad):outputLabel(value.output),
    name:entry.kind==='gamepad'?t('虚拟手柄映射'):entry.value.name,
    tags:[t(analog?'模拟量':triggerNames[trigger])+(!analog&&trigger!=='down'?' '+(value.trigger_ms??250)+' ms':''),
      ...(value.toggle?[t('输出保持')]:[]),...(value.turbo_ms!=null?[t('连发')+' '+value.turbo_ms+' ms']:[]),
      ...('repeat_ms' in value&&value.repeat_ms!=null?[t('连续滚动')+' '+value.repeat_ms+' ms']:[])],
  };
}

type Props={profile:Profile;layerName?:string;layerId:string;entries:OverviewEntry[];device?:Device|null;status?:Status;blocked:boolean;onClose:()=>void;onOpen:(entry:OverviewEntry)=>void};
type Leader={id:string;points:string;end:[number,number]};
export function MappingOverview({profile,layerName,layerId,entries,device,status,blocked,onClose,onOpen}:Props) {
  const [selected,setSelected]=useState<string|null>(null);
  const [leaders,setLeaders]=useState<Leader[]>([]);
  const board=useRef<HTMLDivElement>(null),figure=useRef<HTMLDivElement>(null);
  const counts=new Map<string,number>();
  for(const entry of entries)for(const control of entry.controls)counts.set(control,(counts.get(control)??0)+1);
  const split=(device?.touchpads??0)>=2;
  // Only layout changes measure geometry. Live controller samples do not.
  useLayoutEffect(()=>{
    const container=board.current,preview=figure.current;
    if(!container||!preview)return;
    let frame=0;
    const measure=()=>{
      const svg=preview.querySelector('svg');if(!svg)return;
      const bounds=container.getBoundingClientRect(),rect=svg.getBoundingClientRect(),view=svg.viewBox.baseVal;
      if(!rect.width||!bounds.width)return;
      const next:Leader[]=[];
      for(const group of overviewGroups) {
        if(!group.anchor)continue;
        const card=container.querySelector<HTMLElement>('[data-overview-group="'+group.id+'"]');if(!card)continue;
        const box=card.getBoundingClientRect(),left=group.side==='left';
        const start=[(left?box.right:box.left)-bounds.left,box.top-bounds.top+30];
        const end:[number,number]=[rect.left-bounds.left+group.anchor[0]/view.width*rect.width,rect.top-bounds.top+group.anchor[1]/view.height*rect.height];
        next.push({id:group.id,points:[start,[start[0]+(left?16:-16),start[1]],end].map(p=>p.join(',')).join(' '),end});
      }
      setLeaders(next);
    };
    const resize=new ResizeObserver(()=>{cancelAnimationFrame(frame);frame=requestAnimationFrame(measure);});
    resize.observe(container);resize.observe(preview);
    container.querySelectorAll('[data-overview-group]').forEach(el=>resize.observe(el));
    frame=requestAnimationFrame(measure);return()=>{resize.disconnect();cancelAnimationFrame(frame);};
  },[entries]);
  function selectControl(control:string) {
    setSelected(control);
    const row=board.current?.querySelector<HTMLElement>('[data-overview-row][data-controls~="'+control+'"]');
    row?.scrollIntoView({block:'nearest',inline:'nearest',behavior:'smooth'});
    row?.focus({preventScroll:true});
  }
  function groupCard(group:OverviewGroup) {
    const items=entriesForGroup(entries,group);
    const highlighted=selected!==null&&(group.controls as readonly string[]).includes(selected);
    return <section key={group.id} className={styles.card} data-overview-group={group.id} data-highlighted={highlighted}>
      <h3>{t(group.name)}<span>{items.length}</span></h3>
      {items.length?<div className={styles.rows}>{items.map(entry=>{
        const value=labels(entry,profile,split),inherited=!!layerId&&!entry.layerId;
        const target=entry.controls.find(control=>(group.controls as readonly string[]).includes(control))??entry.controls[0];
        return <button type="button" key={entry.id} className={styles.mapping} data-overview-row data-entry={entry.id} data-controls={entry.controls.join(' ')}
          data-selected={!!selected&&entry.controls.includes(selected)} disabled={blocked}
          aria-label={t(inherited&&entry.kind!=='setting'?'前往基础层编辑：{0}':'编辑映射：{0}').replace('{0}',value.name+' · '+value.input+' → '+value.output)}
          onMouseEnter={()=>setSelected(target)} onFocus={()=>setSelected(target)} onClick={()=>onOpen(entry)}>
          <span className={styles.entryHeading}><strong className={styles.name} translate="no">{value.name}</strong>
            <span className={styles.tags}>{value.tags.map((tag,i)=><span key={i}>{tag}</span>)}{layerId&&<span data-inherited={inherited}>{inherited?'继承基础层':'本层配置'}</span>}</span>
          </span>
          <span className={styles.route}><span>{value.input}</span><span aria-hidden="true">→</span><kbd>{value.output||t('未设置输出')}</kbd></span>
        </button>;
      })}</div>:<p className={styles.empty}>未设置映射</p>}
    </section>;
  }
  return <Dialog title="手柄映射详图" className={styles.dialog} onClose={onClose}
    actions={<button type="button" className="primary-btn" onClick={onClose}>关闭详图</button>}>
    <div className={styles.scope}>
      <div><span>当前草稿</span><strong translate="no">{profile.name}</strong><span className={styles.layer}>{layerName?<><span>基础层</span> + <span translate="no">{layerName}</span></>:'基础层'}</span></div>
      <p>{layerId?'预览基础层与当前编辑层叠加，未叠加其他操作层。点击本层条目编辑；继承条目可前往基础层。':'查看各控件的动作名称和输出按键。点击映射条目编辑，点击手柄控件定位对应条目。'}</p>
    </div>
    <div ref={board} className={styles.board}>
      <svg className={styles.leaders} aria-hidden="true">{leaders.map(line=><g key={line.id} data-highlighted={!!selected&&(overviewGroups.find(g=>g.id===line.id)!.controls as readonly string[]).includes(selected)}>
        <polyline points={line.points}/><circle cx={line.end[0]} cy={line.end[1]} r={3}/>
      </g>)}</svg>
      <div className={styles.left}>{overviewGroups.filter(g=>g.side==='left').map(groupCard)}</div>
      <div ref={figure} className={styles.figure}>
        <Controller device={device} status={status} bindingCounts={counts} showBindingCounts={false} selectedControl={selected} onSelectControl={selectControl}/>
        <p>组合映射会同时出现在各成员控件旁。</p>
      </div>
      <div className={styles.right}>{overviewGroups.filter(g=>g.side==='right').map(groupCard)}</div>
      <div className={styles.extras}>{overviewGroups.filter(g=>g.side==='extra').map(groupCard)}</div>
    </div>
  </Dialog>;
}
