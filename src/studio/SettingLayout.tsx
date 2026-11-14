import {createContext, useContext, useEffect, useId, useState, type ReactNode} from 'react';
import type {ProfileLayer} from '../types';
import {layerSettingOverridden} from '../layers';
import {settingForLabel, type SettingRequest} from './settings-catalog';
import {t} from '../i18n';
import {settingHelp} from './setting-help';
import styles from './SettingLayout.module.css';

export const SettingContext = createContext<{
  layer?: ProfileLayer;
  blocked: boolean;
  restore: (paths: string[]) => void;
  request?: SettingRequest;
} | null>(null);

export function SettingField({label, path, children, disabled = false, inheritance = true}: {
  label: string; path?: string | string[]; children: ReactNode; disabled?: boolean; inheritance?: boolean;
}) {
  const context = useContext(SettingContext);
  const candidate = path ?? settingForLabel(label)?.path;
  const paths = typeof candidate === 'string' ? [candidate] : candidate ?? [];
  const overridden = context?.layer && paths.some(path=>layerSettingOverridden(context.layer!,path));
  return <div className={styles.field} data-setting-path={paths.join(' ') || undefined}>
    {children}
    {inheritance && context?.layer && paths.length > 0 && <div className={styles.inheritance}>
      <span data-overridden={!!overridden}>{overridden ? '本层覆盖' : '继承基础层'}</span>
      {overridden && <button type="button" className="ghost-btn" disabled={context.blocked || disabled}
        aria-label={t('恢复继承：{0}').replace('{0}', t(label))} onClick={()=>context.restore(paths)}>恢复继承</button>}
    </div>}
  </div>;
}

export function SettingSection({id, title, summary, children, defaultOpen = false, lazy = false}: {
  id: string; title: string; summary?: ReactNode; children: ReactNode; defaultOpen?: boolean; lazy?: boolean;
}) {
  const request = useContext(SettingContext)?.request;
  const storageKey = 'motion-studio-section-' + id;
  const [open,setOpen] = useState(()=>{
    try { const saved=localStorage.getItem(storageKey); return saved === null ? defaultOpen : saved === 'open'; }
    catch { return defaultOpen; }
  });
  useEffect(()=>{if(request?.section === id) setOpen(true);},[id,request]);
  return <details className={styles.section} open={open} data-setting-section={id}
    onToggle={event=>{
      const next=event.currentTarget.open;
      setOpen(next);
      try {localStorage.setItem(storageKey,next?'open':'closed');} catch { /* Keep this session usable without storage. */ }
    }}>
    <summary><strong>{title}</strong>{summary && <span className={styles.summary}>{summary}</span>}</summary>
    <div className={styles.sectionBody}>{(!lazy || open) && children}</div>
  </details>;
}

export function SettingLabel({label, htmlFor, help, unit}: {label: string; htmlFor?: string; help?: string; unit?: string}) {
  const description = help ?? settingHelp[label];
  const text = <>{label}{unit && <span> ({unit})</span>}</>;
  return <div className={styles.fieldLabel}>
    {htmlFor ? <label htmlFor={htmlFor}>{text}</label> : <span>{text}</span>}
    {description && <SettingHelp label={label}>{description}</SettingHelp>}
  </div>;
}

export function SettingHelp({label, children}: {label: string; children: ReactNode}) {
  const [open,setOpen]=useState(false);
  const id=useId();
  return <div className={styles.help} onBlur={event=>{
    if(!event.currentTarget.contains(event.relatedTarget))setOpen(false);
  }} onKeyDown={event=>{if(event.key==='Escape'&&open){event.preventDefault();event.stopPropagation();setOpen(false);}}}>
    <button type="button" className="ghost-btn" aria-expanded={open} aria-controls={id}
      aria-label={t('{0}说明').replace('{0}',t(label))} onClick={()=>setOpen(!open)}>?</button>
    <p id={id} hidden={!open}>{typeof children==='string'?t(children):children}</p>
  </div>;
}

export {styles as settingStyles};
