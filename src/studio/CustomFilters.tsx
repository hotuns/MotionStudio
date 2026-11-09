import {useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import type {CustomFilter,FilterPreview,FilterStage,FilterStatus,Profile} from '../types';
import {Dialog} from './Controls';
import {filterTemplates} from './filter-templates';
import styles from './CustomFilters.module.css';
import {t} from '../i18n';

const stages:[FilterStage,string][]=[['raw_gyro','原始角速度'],['before_builtin','内置滤波之前'],['after_builtin','内置滤波之后']];
type Draft={filter:CustomFilter;parameters:{key:string;value:string}[]};
const draftOf=(filter:CustomFilter):Draft=>({filter:structuredClone(filter),parameters:Object.entries(filter.params).map(([key,value])=>({key,value:String(value)}))});

export function CustomFilters({profile,update,statuses=[],running=false,onReset}:{profile:Profile;update:(change:(profile:Profile)=>void)=>void;statuses?:FilterStatus[];running?:boolean;onReset:()=>void}) {
  const filters=profile.gyro.motion.custom_filters??[];
  const [editing,setEditing]=useState<Draft|null>(null),[removing,setRemoving]=useState<string|null>(null);
  const [template,setTemplate]=useState('passthrough'),[signal,setSignal]=useState('noise'),[axis,setAxis]=useState(0);
  const [preview,setPreview]=useState<FilterPreview|null>(null),[error,setError]=useState<string|null>(null),[previewing,setPreviewing]=useState(false);
  const fileInput=useRef<HTMLInputElement>(null),request=useRef(0);
  const previewArea=useRef<HTMLDivElement>(null);
  const previewKey=JSON.stringify([editing,signal]);const latestKey=useRef(previewKey);latestKey.current=previewKey;
  useEffect(()=>()=>{request.current++;},[]);
  useEffect(()=>{if(preview)previewArea.current?.scrollIntoView({block:'nearest'});},[preview]);
  const mutate=(change:(filters:CustomFilter[])=>void)=>update(draft=>{draft.gyro.motion.custom_filters??=[];change(draft.gyro.motion.custom_filters);});
  const close=()=>{request.current++;setEditing(null);setPreview(null);setError(null);setPreviewing(false);};
  const open=(filter:CustomFilter)=>{setEditing(draftOf(filter));setTemplate('passthrough');setPreview(null);setError(null);};
  const patch=(change:Partial<CustomFilter>)=>{setEditing(value=>value&&{...value,filter:{...value.filter,...change}});setPreview(null);setError(null);};
  const config=():CustomFilter=>{
    if(!editing)throw Error('请先选择滤波器。');
    if(!editing.filter.name.trim())throw Error('请填写滤波器名称。');
    if(new TextEncoder().encode(editing.filter.source).length>32768)throw Error('脚本最多为 32 KiB。');
    const params:Record<string,number>={};
    for(const item of editing.parameters){
      const key=item.key.trim(),value=Number(item.value);
      if(!/^[A-Za-z0-9_]{1,48}$/.test(key)||Object.hasOwn(params,key)||!item.value.trim()||!Number.isFinite(value)||Math.abs(value)>1000000)throw Error('参数名需为不重复的字母、数字或下划线，参数值需为有效数值。');
      params[key]=value;
    }
    return {...editing.filter,params};
  };
  const submit=()=>{try{const value=config();mutate(filters=>{const index=filters.findIndex(filter=>filter.id===value.id);if(index<0)filters.push(value);else filters[index]=value;});close();}catch(error){setError(String(error));}};
  async function runPreview(){
    let filter:CustomFilter;try{filter=config();}catch(error){setError(String(error));return;}
    const id=++request.current,key=latestKey.current;setPreviewing(true);setError(null);
    try{const result=await invoke<FilterPreview>('preview_motion_filter',{filter,signal});if(request.current===id&&latestKey.current===key)setPreview(result);}
    catch(error){if(request.current===id&&latestKey.current===key)setError(String(error));}
    finally{if(request.current===id)setPreviewing(false);}
  }
  const reorder=(id:string,direction:number)=>mutate(filters=>{const index=filters.findIndex(filter=>filter.id===id),stage=filters[index]?.stage;const same=filters.map((filter,index)=>({filter,index})).filter(item=>item.filter.stage===stage);const next=same[same.findIndex(item=>item.index===index)+direction]?.index;if(next!==undefined)[filters[index],filters[next]]=[filters[next],filters[index]];});
  const shown=preview?.points??[],peak=Math.max(0.1,...shown.flatMap(point=>[Math.abs(point.input[axis]),Math.abs(point.output[axis])]));
  const line=(output:boolean)=>shown.map((point,index)=>`${20+index/Math.max(1,shown.length-1)*860},${90-(output?point.output[axis]:point.input[axis])/peak*70}`).join(' ');
  return <details className={styles.root} open={filters.length>0}>
    <summary>高级：自定义处理链（Rhai）</summary>
    <p>添加自己的防抖和滤波脚本。按传感器样本处理，状态与参数独立；修改随当前配置保存。</p>
    <div className={styles.toolbar}>
      <button type="button" className="secondary-btn" disabled={filters.length>=4} onClick={()=>open({api_version:1,id:crypto.randomUUID(),name:t('自定义滤波'),enabled:true,stage:'before_builtin',source:filterTemplates[0].source,params:{}})}>添加滤波器</button>
      <button type="button" className="ghost-btn" disabled={!running||!filters.length} onClick={onReset}>重置脚本状态</button>
    </div>
    <p>不同阶段按管线顺序执行，同阶段按列表顺序执行。最多添加 4 个。</p>
    {profile.gyro.motion.mode==='steering'&&<p>姿态控制模式只经过原始角速度阶段。</p>}
    {stages.map(([stage,label])=>{
      const items=filters.filter(filter=>filter.stage===stage);
      return <section key={stage} className={styles.stage}><h4>{label}</h4>{!items.length?<p>此阶段没有自定义滤波器。</p>:items.map((filter,index)=>{
        const live=running?statuses.find(status=>status.id===filter.id):undefined;
        return <article key={filter.id} className={styles.card}>
          <div className={styles.identity}><label><input type="checkbox" aria-label={'启用滤波器 '+filter.name} checked={filter.enabled} onChange={event=>mutate(filters=>{const item=filters.find(item=>item.id===filter.id);if(item)item.enabled=event.target.checked;})}/><strong translate="no">{filter.name}</strong></label>
            <span className={styles.status} data-fault={live?.bypassed} title={live?.error??undefined}>{!filter.enabled?'已关闭':profile.gyro.motion.mode==='steering'&&filter.stage!=='raw_gyro'?'此模式未执行':live?.bypassed?'已旁路':live?.calls?'运行中':live?'待采样':'保存后应用'}{live&&<> · {live.last_us} µs / {live.max_us} µs</>}</span>
          </div>
          <div className={styles.actions}>
            <button type="button" className="ghost-btn" aria-label={'上移滤波器 '+filter.name} disabled={index===0} onClick={()=>reorder(filter.id,-1)}>↑</button>
            <button type="button" className="ghost-btn" aria-label={'下移滤波器 '+filter.name} disabled={index===items.length-1} onClick={()=>reorder(filter.id,1)}>↓</button>
            <button type="button" className="secondary-btn" aria-label={'编辑滤波器 '+filter.name} onClick={()=>open(filter)}>编辑</button>
            {removing===filter.id?<><button type="button" className="ghost-btn" onClick={()=>{mutate(filters=>{const index=filters.findIndex(item=>item.id===filter.id);if(index>=0)filters.splice(index,1);});setRemoving(null);}}>确认删除</button><button type="button" className="ghost-btn" onClick={()=>setRemoving(null)}>取消</button></>:<button type="button" className="ghost-btn" aria-label={'删除滤波器 '+filter.name} onClick={()=>setRemoving(filter.id)}>删除</button>}
          </div>
          {live?.error&&<p className={styles.error} role="status" translate="no">{live.error}</p>}
        </article>;
      })}</section>;
    })}
    {!!statuses.length&&running&&<p>运行耗时显示上次与峰值，状态来自已应用配置。脚本出错或超过预算时自动旁路，重置状态或应用修改后可重试。</p>}
    {editing&&<Dialog title="编辑自定义滤波器" className={styles.editor} onClose={close} onSubmit={submit} error={error??undefined}
      actions={<><button type="button" className="secondary-btn" onClick={close}>取消</button><button type="submit" className="primary-btn" disabled={previewing}>完成</button></>}>
      <div className={styles.fields}><label>滤波器名称<input aria-label="滤波器名称" autoFocus value={editing.filter.name} maxLength={100} onChange={event=>patch({name:event.target.value})}/></label>
        <label>接入阶段<select aria-label="滤波器接入阶段" value={editing.filter.stage} onChange={event=>patch({stage:event.target.value as FilterStage})}>{stages.map(([value,label])=><option key={value} value={value}>{label}</option>)}</select></label></div>
      <label className="studio-toggle-row"><span>需要加速度计</span><input aria-label="滤波器需要加速度计" type="checkbox" checked={editing.filter.requires_accel??false} onChange={event=>patch({requires_accel:event.target.checked})}/></label>
      <div className={styles.editorTools}><div className={styles.actions}><select aria-label="滤波脚本模板" value={template} onChange={event=>setTemplate(event.target.value)}>{filterTemplates.map(template=><option value={template.id} key={template.id}>{template.name}</option>)}</select>
        <button type="button" className="secondary-btn" onClick={()=>{const value=filterTemplates.find(item=>item.id===template)!;setEditing(current=>current&&{...draftOf({...current.filter,source:value.source,params:value.params})});setPreview(null);setError(null);}}>载入模板</button></div>
        <button type="button" className="ghost-btn" onClick={()=>fileInput.current?.click()}>导入 .rhai 文件</button>
        <input ref={fileInput} type="file" accept=".rhai,.txt" hidden onChange={async event=>{const file=event.target.files?.[0];event.target.value='';if(!file)return;if(file.size>32768){setError('脚本最多为 32 KiB。');return;}const key=latestKey.current;try{const source=await file.text();if(latestKey.current===key)patch({source});}catch(error){if(latestKey.current===key)setError(String(error));}}}/>
      </div>
      <p className={styles.hint}>载入模板会替换当前代码与参数。脚本通过 process(sample, params) 返回 [x, y, z]，this 用于保存独立状态。</p>
      <textarea className={styles.code} aria-label="Rhai 滤波脚本" spellCheck={false} maxLength={32768} value={editing.filter.source} onChange={event=>patch({source:event.target.value})} onKeyDown={event=>{if(event.key==='Tab'&&!event.shiftKey&&!event.ctrlKey&&!event.altKey&&!event.metaKey){event.preventDefault();const input=event.currentTarget,start=input.selectionStart,end=input.selectionEnd;patch({source:input.value.slice(0,start)+'    '+input.value.slice(end)});requestAnimationFrame(()=>input.setSelectionRange(start+4,start+4));}}}/>
      <details><summary>脚本接口与执行规则</summary><p>角速度单位为 °/s，sample.dt 为秒；host_ns 与 sensor_ns 保留原始时间戳。原始阶段处理三轴，其余阶段的 z 为 0。</p><p>脚本无文件、网络或输入输出设备接口。单次调用限制运算量和执行时间，异常、无效数值或过大的状态会让该滤波器旁路。</p></details>
      <div className={styles.parameters}><h4>数值参数</h4>{editing.parameters.map((item,index)=><div className={styles.parameter} key={index}>
        <input aria-label={`参数名 ${index+1}`} placeholder="parameter_name" value={item.key} onChange={event=>{setEditing(current=>current&&{...current,parameters:current.parameters.map((item,i)=>i===index?{...item,key:event.target.value}:item)});setPreview(null);}}/>
        <input aria-label={`参数值 ${index+1}`} type="number" step="any" value={item.value} onChange={event=>{setEditing(current=>current&&{...current,parameters:current.parameters.map((item,i)=>i===index?{...item,value:event.target.value}:item)});setPreview(null);}}/>
        <button type="button" className="ghost-btn" aria-label={`删除参数 ${index+1}`} onClick={()=>{setEditing(current=>current&&{...current,parameters:current.parameters.filter((_,i)=>i!==index)});setPreview(null);}}>移除</button>
      </div>)}<button type="button" className="secondary-btn" disabled={editing.parameters.length>=32} onClick={()=>setEditing(current=>current&&{...current,parameters:[...current.parameters,{key:'',value:'0'}]})}>添加参数</button></div>
      <div className={styles.preview} ref={previewArea}><div className={styles.editorTools}><select aria-label="滤波预览信号" value={signal} onChange={event=>{setSignal(event.target.value);setPreview(null);}}><option value="noise">静止抖动</option><option value="step">阶跃转动</option><option value="sine">连续转动</option></select>
        <select aria-label="预览轴" value={axis} onChange={event=>setAxis(Number(event.target.value))}><option value={0}>X</option><option value={1}>Y</option><option value={2}>Z</option></select>
        <button type="button" className="secondary-btn" disabled={previewing} onClick={()=>void runPreview()}>{previewing?'正在验证…':'验证并预览'}</button></div>
        <p className={styles.hint}>预览只运行当前滤波器和合成信号，不占用手柄。完成后仍需保存配置才能应用。</p>
        {preview&&<><div className={styles.legend}><span>输入</span><span>滤波后</span></div><svg viewBox="0 0 900 180" role="img" aria-label="自定义滤波预览曲线"><line x1="20" x2="880" y1="90" y2="90" stroke="var(--border-1)"/><polyline points={line(false)} fill="none" stroke="var(--text-muted)" strokeWidth="1.3"/><polyline points={line(true)} fill="none" stroke="var(--accent)" strokeWidth="2"/></svg>
          <p className={styles.hint}>500 Hz · {preview.points.length} 个样本 · {preview.status.last_us} µs / {preview.status.max_us} µs</p>{preview.status.error&&<p className={styles.error} role="alert" translate="no">{preview.status.error}</p>}</>}
      </div>
    </Dialog>}
  </details>;
}
