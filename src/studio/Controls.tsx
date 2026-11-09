import { useEffect, useId, useRef, type ReactNode } from 'react';
import type { GamepadTarget } from '../types';
import { StudioIcon } from './StudioIcon';
import dialogStyles from './CreateProfileDialog.module.css';
import motionStyles from './MotionControls.module.css';

export const gamepadNames: Record<GamepadTarget, string> = {
  none: '键盘与鼠标',
  xbox_360: 'Xbox 360',
  dualshock_4: 'DualShock 4（PS4）',
};

export function SettingsGroup({title, children}: {title: string; children: ReactNode}) {
  return <div className={motionStyles.group}>
    <div className={motionStyles.groupHeading}><h3>{title}</h3></div>
    {children}
  </div>;
}

export function Toggle({ value, onChange, label, help, disabled }: {
  value: boolean;
  onChange: (value: boolean) => void;
  label: string;
  help?: string;
  disabled?: boolean;
}) {
  return <div className="studio-toggle-row">
    <div><span>{label}</span>{help && <p className="studio-muted">{help}</p>}</div>
    <button className="studio-toggle" type="button" role="switch" aria-checked={value} aria-label={label}
      disabled={disabled} onClick={() => onChange(!value)}><i /></button>
  </div>;
}

export function Range({ label, value, min, max, step, onChange, unit = '', help }: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (value: number) => void;
  unit?: string;
  help?: string;
}) {
  const id = useId();
  return <div className="studio-range">
    <div className="studio-range-heading">
      <label htmlFor={id}>{label}</label>
      <div>
        <input id={id} type="number" min={min} max={max} step={step} value={Number.isFinite(value) ? Number(value.toFixed(4)) : ''}
          onChange={event => onChange(event.target.valueAsNumber)} />
        {unit && <span>{unit}</span>}
      </div>
    </div>
    <input aria-label={label + '滑块'} type="range" min={min} max={max} step={step}
      value={Number.isFinite(value) ? value : min} onChange={event => onChange(event.target.valueAsNumber)} />
    <div className="studio-range-limits"><span>{min}{unit}</span><span>{max}{unit}</span></div>
    {help && <p className="studio-muted">{help}</p>}
  </div>;
}

export function Dialog({ title, onClose, onSubmit, actions, children, description, error, className }: {
  title: string;
  onClose: () => void;
  onSubmit?: () => void;
  actions: ReactNode;
  children: ReactNode;
  description?: string;
  error?: string;
  className?: string;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  useEffect(() => {
    const dialog=ref.current;if(!dialog)return;
    if(!dialog.open)dialog.showModal();
    if(document.documentElement.dataset.gamepad==='on'){
      const body=dialog.querySelector('[data-pad-zone="dialog-body"]');
      if(!body?.contains(document.activeElement)){
        const target=body?.querySelector<HTMLElement>('[aria-pressed="true"]:not(:disabled),input:not(:disabled),textarea:not(:disabled),select:not(:disabled),button:not(:disabled)');
        target?.focus();
      }
    }
  }, []);
  return <dialog className={[dialogStyles.dialog,className].filter(Boolean).join(' ')} ref={ref} aria-labelledby={titleId}
    onCancel={event => { event.preventDefault(); event.stopPropagation(); onClose(); }}>
    <form onSubmit={event => { event.preventDefault(); event.stopPropagation(); onSubmit?.(); }}>
      <header className="studio-dialog-heading" data-pad-zone="dialog-header">
        <h2 id={titleId}>{title}</h2>
        <button type="button" className="ghost-btn" data-cancel-capture data-dialog-close onClick={onClose} aria-label="关闭"><StudioIcon name="close" size={17} /></button>
      </header>
      {description && <p>{description}</p>}
      <p className="studio-pad-dialog-hint">方向移动 · A 确认／编辑 · B 返回 · LB／RB 切换区域</p>
      <div className={dialogStyles.body} data-pad-zone="dialog-body">
        {children}
        {error && <p className={dialogStyles.error} role="alert">{error}</p>}
      </div>
      <footer className={dialogStyles.actions} data-pad-zone="dialog-actions">{actions}</footer>
    </form>
  </dialog>;
}
