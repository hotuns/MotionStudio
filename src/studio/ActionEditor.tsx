import {useEffect,useRef,useState} from 'react';
import {chordParts,inputLabel,outputLabel,mouseOutputs,wheelOutputs,type Action,type Profile} from '../types';
import {triggerPeers} from '../mapping-triggers';
import type {Capture,CaptureKind} from './ActionMappings';
import {ActionTriggerFields,InputCaptureButton,InputChordFields,InputQuickKeys,OutputChordFields,quickInputKeys} from './BindingFields';
import {Dialog,Range,Toggle} from './Controls';
import styles from './ActionEditor.module.css';
import {keyboardKeys,modifierOutputs as modifiers} from '../keyboard';

const primaryModifiers=['key:CTRL','key:ALT','key:SHIFT'];
const keyboardRows=[['ESC',...Array.from({length:11},(_,i)=>'F'+(i+1))],['GRAVE',...'1234567890','MINUS','EQUALS','BACKSPACE'],['TAB',...'QWERTYUIOP','LEFTBRACKET','RIGHTBRACKET','BACKSLASH'],['CAPSLOCK',...'ASDFGHJKL','SEMICOLON','APOSTROPHE','ENTER'],[...'ZXCVBNM','COMMA','PERIOD','SLASH'],['SPACE','LEFT','UP','DOWN','RIGHT','DELETE']];
const keyText=(key:string)=>({UP:'↑',DOWN:'↓',LEFT:'←',RIGHT:'→',BACKSPACE:'⌫',ENTER:'↵',DELETE:'Del',CAPSLOCK:'Caps'}[key]??outputLabel('key:'+key));
type KeyboardSection='main'|'extended'|'numpad';
const sectionForOutput=(value:string):KeyboardSection=>{
  const parts=chordParts(value),key=keyboardKeys.find(key=>parts.includes('key:'+key.name)&&!key.modifier)??keyboardKeys.find(key=>parts.includes('key:'+key.name));
  return key?.group==='numpad'?'numpad':key?.group==='navigation'||key?.group==='modifier'&&!primaryModifiers.includes('key:'+key.name)||key?.group==='function'&&Number(key.name.slice(1))>12?'extended':'main';
};

export function ActionEditor({action,isNew,profile,index,onChange,onClose,onSubmit,capture,onCapture,onCancelCapture,onAssignEscape,busy,error,layerName,navigation=false}:{action:Action;isNew:boolean;profile?:Profile;index:number|null;onChange:(patch:Partial<Action>)=>void;onClose:()=>void;onSubmit:()=>void;capture:Capture|null;onCapture:(kind:CaptureKind)=>void;onCancelCapture:()=>void;onAssignEscape:()=>void;busy:boolean;error?:string;layerName?:string;navigation?:boolean}) {
  const [inputCombo,setInputCombo]=useState(chordParts(action.input).length>1);
  const [outputCombo,setOutputCombo]=useState(chordParts(action.output).filter(value=>!modifiers.includes(value)).length>1);
  useEffect(()=>{if(chordParts(action.input).length>1)setInputCombo(true);},[action.input]);
  useEffect(()=>{if(chordParts(action.output).filter(value=>!modifiers.includes(value)).length>1)setOutputCombo(true);},[action.output]);
  const [tab,setTab]=useState<'keyboard'|'mouse'|'wheel'>(()=>chordParts(action.output).some(value=>wheelOutputs.includes(value))?'wheel':chordParts(action.output).some(value=>mouseOutputs.includes(value))?'mouse':'keyboard');
  const [keyboardSection,setKeyboardSection]=useState<KeyboardSection>(()=>sectionForOutput(action.output));
  const previousCapture=useRef(capture?.kind);
  useEffect(()=>{
    if(previousCapture.current==='key'&&capture?.kind!=='key'){
      const values=chordParts(action.output);
      setTab(values.some(value=>wheelOutputs.includes(value))?'wheel':values.some(value=>mouseOutputs.includes(value))?'mouse':'keyboard');
      setKeyboardSection(sectionForOutput(action.output));
    }
    previousCapture.current=capture?.kind;
  },[capture?.kind,action.output]);
  const input=chordParts(action.input),output=chordParts(action.output),locked=busy||!!capture;
  const hasWheel=output.some(value=>wheelOutputs.includes(value));
  const analog=input.some(value=>/^(left|right)_trigger$|_stick_(up|down|left|right)$/.test(value));
  const validMs=(value:number|undefined|null,min:number,max:number)=>value==null||Number.isInteger(value)&&value>=min&&value<=max;
  const ready=!!action.name.trim()&&input.length>0&&input.length<=4&&output.length>0&&output.length<=4&&Number.isFinite(action.threshold)&&action.threshold>=0&&action.threshold<1&&validMs(action.trigger_ms,10,5000)&&validMs(action.turbo_ms,20,2000)&&validMs(action.repeat_ms,20,2000)&&validMs(action.chord_wait_ms,0,500);
  const nextOutput=(value:string)=>{
    if(modifiers.includes(value))return output.includes(value)?output.filter(part=>part!==value):[...output,value];
    if(!outputCombo)return [...output.filter(part=>modifiers.includes(part)),value];
    if(output.includes(value))return output.filter(part=>part!==value);
    return [...output.filter(part=>!wheelOutputs.includes(value)||!wheelOutputs.includes(part)),value];
  };
  const chooseOutput=(value:string)=>{const values=nextOutput(value);if(values.length>4)return;onChange({output:values.join('+'),repeat_ms:values.some(value=>wheelOutputs.includes(value))?action.repeat_ms??null:null});};
  const inputPreview=capture?.kind==='pad'&&capture.preview?.length?capture.preview.join('+'):action.input;
  const outputPreview=capture?.kind==='key'&&capture.preview?.length?capture.preview.join('+'):action.output;
  const record=(kind:CaptureKind)=>capture?.kind===kind?onCancelCapture():onCapture(kind);
  return <Dialog title={isNew?'添加动作':'设置游戏动作'} className={styles.editorDialog} onClose={onClose} onSubmit={()=>{if(ready&&!locked)onSubmit();}} error={error}
    actions={<><button type="button" className="secondary-btn" data-cancel-capture onClick={onClose}>取消</button><button type="submit" className="primary-btn" disabled={locked||!ready}>完成</button></>}>
    <div className={styles.identity}><label>动作名称<input aria-label="动作名称" autoFocus required value={action.name} disabled={locked} onChange={event=>onChange({name:event.target.value})}/></label>
      <span className={styles.context}>编辑层：{layerName?<strong translate="no">{layerName}</strong>:<strong>基础层</strong>}</span></div>
    <div className={styles.columns}>
      <section className={styles.panel} aria-label="手柄输入设置" data-pad-zone="binding-input">
        <div className={styles.heading}><h3>手柄输入</h3><span>1</span></div>
        <InputCaptureButton value={inputLabel(inputPreview)} recording={capture?.kind==='pad'}
          disabled={busy||!!capture&&capture.kind!=='pad'} onClick={()=>record('pad')}/>
        <fieldset className={styles.manual} disabled={locked}>
          <div className={styles.parts} aria-label="已选手柄输入">{input.map(value=><button type="button" key={value} aria-label={'移除输入 '+inputLabel(value)} onClick={()=>onChange({input:input.filter(part=>part!==value).join('+')})}>{inputLabel(value)} <span aria-hidden="true">×</span></button>)}</div>
          <div className={styles.sectionHeading}>触发方式</div><ActionTriggerFields action={action} onChange={onChange} peers={triggerPeers(profile,'action',index)}/>
          <Toggle label="组合输入" value={inputCombo} onChange={setInputCombo}/>
          <InputQuickKeys value={action.input} combo={inputCombo} onChange={input=>onChange({input})}/>
          <details className={styles.advanced} open={input.some(value=>!quickInputKeys.some(([key])=>key===value))||input.length===0}>
            <summary>完整输入与组合设置</summary><InputChordFields value={action.input} onChange={value=>onChange({input:value})}/>
          </details>
          {analog&&<Range label="触发阈值" value={action.threshold} min={0} max={0.95} step={0.05} onChange={threshold=>onChange({threshold})} help="扳机或摇杆方向超过该比例时，触发对应键鼠操作。"/>}
        </fieldset>
      </section>
      <section className={styles.panel} aria-label="键鼠输出设置" data-pad-zone="binding-output">
        <div className={styles.heading}><h3>键鼠输出</h3><span>2</span></div>
        <button type="button" className={styles.capturePad} data-cancel-capture aria-label={capture?.kind==='key'?'取消键鼠录入':'录入键鼠'} aria-pressed={capture?.kind==='key'} disabled={busy||!!capture&&capture.kind!=='key'} onClick={()=>record('key')}>
          <small>{capture?.kind==='key'?'正在录入键鼠…':'点击录入键鼠'}</small><strong>{outputLabel(outputPreview)||'选择键鼠操作'}</strong><span>{capture?.kind==='key'?'松开后完成 · 滚轮立即录入 · 点击取消':'支持键盘、鼠标按钮、侧键与四向滚轮'}</span>
        </button>
        {capture?.kind==='key'&&<div className={styles.escapeCapture}>
          <button type="button" className="secondary-btn" data-cancel-capture disabled={(capture.preview??[]).filter(value=>modifiers.includes(value)).length>=4} onClick={onAssignEscape}>映射 Esc</button><small>按 Esc 取消录入；要输出 Esc，请点此按钮。</small>
        </div>}
        <fieldset className={styles.manual} disabled={locked}>
          <div className={styles.parts} aria-label="已选键鼠输出">{output.map(value=><button type="button" key={value} aria-label={'移除输出 '+outputLabel(value)} onClick={()=>{const values=output.filter(part=>part!==value);onChange({output:values.join('+'),repeat_ms:values.some(value=>wheelOutputs.includes(value))?action.repeat_ms??null:null});}}>{outputLabel(value)} <span aria-hidden="true">×</span></button>)}</div>
          <div className={styles.outputTools}><div className={styles.tabs} role="tablist" aria-label="输出类型">{([['keyboard','键盘'],['mouse','鼠标'],['wheel','滚轮']] as const).map(([value,label])=><button type="button" role="tab" key={value} aria-selected={tab===value} onClick={()=>setTab(value)}>{label}</button>)}</div><Toggle label="组合输出" value={outputCombo} onChange={setOutputCombo}/></div>
          <div className={styles.keyboardTools}><div className={styles.modifiers}>{primaryModifiers.map(value=><button type="button" key={value} aria-label={'输出 '+outputLabel(value)} aria-pressed={output.includes(value)} disabled={nextOutput(value).length>4} onClick={()=>chooseOutput(value)}>{outputLabel(value)}</button>)}</div>
            {tab==='keyboard'&&<div className={styles.keyboardSections} role="group" aria-label="键盘区域">
              {([['main','主键区'],['extended','导航与扩展键'],['numpad','数字小键盘']] as const).map(([value,label])=><button
                type="button" className="secondary-btn" key={value} aria-pressed={keyboardSection===value} onClick={()=>setKeyboardSection(value)}>{label}</button>)}
            </div>}
          </div>
          {tab==='keyboard'?keyboardSection==='main'?<div className={styles.keyboard} aria-label="键盘选键">{keyboardRows.map((row,index)=><div className={styles.keyRow} key={index}>{row.map(key=><button type="button" key={key} aria-label={'输出 '+outputLabel('key:'+key)} title={outputLabel('key:'+key)} aria-pressed={output.includes('key:'+key)} disabled={nextOutput('key:'+key).length>4} onClick={()=>chooseOutput('key:'+key)}>{keyText(key)}</button>)}</div>)}</div>:<div className={styles.extraKeys} aria-label="扩展键盘选键">{keyboardKeys.filter(key=>keyboardSection==='numpad'?key.group==='numpad':key.group==='navigation'||key.group==='modifier'&&!primaryModifiers.includes('key:'+key.name)||key.group==='function'&&Number(key.name.slice(1))>12).map(key=><button type="button" key={key.name} aria-label={'输出 '+outputLabel('key:'+key.name)} aria-pressed={output.includes('key:'+key.name)} disabled={nextOutput('key:'+key.name).length>4} onClick={()=>chooseOutput('key:'+key.name)}>{outputLabel('key:'+key.name)}</button>)}</div>:<div className={styles.mouseKeys} aria-label={tab==='mouse'?'鼠标按钮选键':'鼠标滚轮选键'}>{(tab==='mouse'?mouseOutputs:wheelOutputs).map(value=><button type="button" key={value} aria-label={'输出 '+outputLabel(value)} aria-pressed={output.includes(value)} disabled={nextOutput(value).length>4} onClick={()=>chooseOutput(value)}>{outputLabel(value)}</button>)}</div>}
          <p className={styles.hint}>{outputCombo?'可组合最多 4 项。再次选择可移除，滚轮最多一个方向。':'选择一个普通输出，Ctrl／Alt／Shift 可独立叠加。'}</p>
          {hasWheel&&<><Toggle label="按住连续滚动" value={action.repeat_ms!=null} onChange={enabled=>onChange({repeat_ms:enabled?100:null})}/>{action.repeat_ms!=null&&<Range label="滚动间隔" value={action.repeat_ms} min={20} max={2000} step={10} unit=" ms" onChange={repeat_ms=>onChange({repeat_ms})}/>}</>}
          <details className={styles.advanced}><summary>完整输出与组合设置</summary><OutputChordFields action={action} onChange={onChange} showRepeat={false}/></details>
        </fieldset>
      </section>
    </div>
    {capture&&<p className={styles.recording} role="status">{capture.kind==='key'?'按下组合键或鼠标按钮，全部松开后完成；也可滚动滚轮。':'同时按住手柄按键或摇杆方向，全部松开后完成。'} Esc 取消录入。{navigation&&' View + Menu 取消录入。'}
      {!!capture.preview?.length&&<strong> {capture.kind==='key'?outputLabel(capture.preview.join('+')):inputLabel(capture.preview.join('+'))}</strong>}</p>}
  </Dialog>;
}
