import {useId, useRef, useState} from 'react';
import {createPortal} from 'react-dom';
import {t} from '../i18n';
import {Dialog} from './Controls';
import {StudioIcon, type StudioIconName} from './StudioIcon';
import styles from './OptionPicker.module.css';

export type PickerOption = {
  value: string;
  label: string;
  search?: string;
  disabled?: boolean;
  localize?: false;
};
export type PickerGroup = {label?: string; options: PickerOption[]};

const display = (option: PickerOption) => option.localize === false ? option.label : t(option.label);

type OptionListProps = {
  value: string | null;
  groups: PickerGroup[];
  onChoose: (value: string) => void;
  onClose: () => void;
  searchLabel?: string;
  searchPlaceholder?: string;
  emptyMessage?: string;
  layout?: 'grid' | 'list';
  icon?: StudioIconName;
  selectedLabel?: string;
};

export function OptionList({value, groups, onChoose, onClose, searchLabel = '搜索选项',
  searchPlaceholder = '搜索名称或按键', emptyMessage = '没有匹配的选项。', layout = 'grid', icon, selectedLabel}: OptionListProps) {
  const [query, setQuery] = useState('');
  const results = useRef<HTMLDivElement>(null);
  const gamepad = document.documentElement.dataset.gamepad === 'on';
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const matches = groups.map(group => ({...group, options:group.options.filter(option => {
    const text = [option.label, display(option), option.value, option.search, group.label, group.label && t(group.label)]
      .join(' ').toLowerCase();
    return terms.every(term => text.includes(term));
  })})).filter(group => group.options.length > 0);

  return <>
    <label className={styles.search}>
      <StudioIcon name="search" size={16}/>
      <input type="search" aria-label={searchLabel} placeholder={searchPlaceholder} value={query}
        autoFocus={!gamepad} onChange={event => setQuery(event.target.value)}
        onKeyDown={event => {
          if (event.nativeEvent.isComposing) return;
          if (event.key === 'Escape') {
            event.preventDefault();
            event.stopPropagation();
            onClose();
            return;
          }
          const first = results.current?.querySelector<HTMLButtonElement>('button:not(:disabled)');
          if (event.key === 'ArrowDown') { event.preventDefault(); first?.focus(); }
          if (event.key === 'Enter') { event.preventDefault(); first?.click(); }
        }}/>
    </label>
    <div ref={results} className={styles.results}>
      {matches.map((group, index) => <section key={index} aria-label={group.label}>
        {group.label && <h3>{group.label}</h3>}
        <div className={[styles.options, layout === 'list' ? styles.listOptions : ''].filter(Boolean).join(' ')}>
          {group.options.map(option => <button type="button" className="secondary-btn" key={option.value}
            aria-pressed={option.value === value} autoFocus={gamepad && option.value === value && !option.disabled}
            disabled={option.disabled} onClick={() => onChoose(option.value)}>
            {layout === 'list' && icon && <span className={styles.optionIcon}><StudioIcon name={icon} size={18}/></span>}
            <span className={layout === 'list' ? styles.optionName : undefined} title={display(option)} translate="no">{display(option)}</span>
            {layout === 'list' && option.value === value && <>
              {selectedLabel && <span className={styles.selectedLabel}>{selectedLabel}</span>}
              <StudioIcon name="check" size={17}/>
            </>}
          </button>)}
        </div>
      </section>)}
      {!matches.length && <p role="status">{emptyMessage}</p>}
    </div>
  </>;
}

export function OptionDialog({title, description, ...list}: OptionListProps & {title: string; description?: string}) {
  return createPortal(<Dialog title={title} description={description} className={styles.dialog} onClose={list.onClose}
    actions={<button type="button" className="secondary-btn" onClick={list.onClose}>取消选择</button>}>
    <OptionList {...list}/>
  </Dialog>, document.body);
}

export function OptionPicker({label, value, groups, onChange, disabled, placeholder = '选择选项', className}: {
  label: string;
  value: string | null;
  groups: PickerGroup[];
  onChange: (value: string) => void;
  disabled?: boolean;
  placeholder?: string;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const valueId = useId();
  const current = groups.flatMap(group => group.options).find(option => option.value === value);
  const shown = current ? display(current) : value || t(placeholder);
  const close = () => { setOpen(false); requestAnimationFrame(() => trigger.current?.focus()); };

  return <>
    <button ref={trigger} type="button" className={[styles.trigger, className].filter(Boolean).join(' ')}
      aria-label={label} aria-describedby={valueId} aria-haspopup="dialog" aria-expanded={open}
      disabled={disabled} onClick={() => setOpen(true)}>
      <span id={valueId} title={shown} translate="no">{shown}</span><StudioIcon name="search" size={14}/>
    </button>
    {open && <OptionDialog title={label} value={value} groups={groups} onClose={close} onChoose={next => {
      if (!trigger.current?.matches(':disabled')) onChange(next);
      close();
    }}/>}
  </>;
}
