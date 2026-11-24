import {useId, useState} from 'react';
import styles from './MotionControls.module.css';

export function CurvePointField({label, value, disabled=false, max, step='any', onEdit}: {
  label: string; value: number; disabled?: boolean; max?: number; step?: number | 'any'; onEdit: (text: string)=>string | undefined;
}) {
  const [draft, setDraft] = useState<{text: string; error?: string} | null>(null);
  const id = useId();
  return <div className={styles.stickPointField}><label htmlFor={id}>{label}</label>
    <input id={id} type="number" step={step} min={0} max={max} disabled={disabled} value={draft?.text ?? value}
      aria-invalid={!!draft?.error} aria-describedby={draft?.error ? id+'-error' : undefined}
      onChange={event=>{const text=event.currentTarget.value; setDraft({text, error:onEdit(text)});}}
      onBlur={()=>setDraft(null)}/>
    {draft?.error && <small role="status" id={id+'-error'}>{draft.error}</small>}
  </div>;
}
