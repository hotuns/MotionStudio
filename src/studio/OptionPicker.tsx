import {memo, useId, useRef, useState} from 'react';
import {createPortal} from 'react-dom';
import {t, useLanguage} from '../i18n';
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
export type PickerGroup = {label?: string; localizeLabel?: boolean; options: PickerOption[]};

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

// Live pages can recreate equivalent option data on each controller snapshot.
// Compare the displayed and searchable fields before rebuilding the dialog rows.
function sameGroups(previous: PickerGroup[], next: PickerGroup[]) {
  return previous === next || (previous.length === next.length && previous.every((group, index) => {
    const other = next[index];
    return group.label === other.label && group.localizeLabel === other.localizeLabel && (group.options === other.options ||
      (group.options.length === other.options.length && group.options.every((option, i) => {
        const item = other.options[i];
        return option.value === item.value && option.label === item.label && option.search === item.search &&
          option.disabled === item.disabled && option.localize === item.localize;
      })));
  }));
}

type ResultsProps = Pick<OptionListProps, 'value'|'groups'|'emptyMessage'|'layout'|'icon'|'selectedLabel'> & {query:string; gamepad:boolean};
const OptionResults = memo(function OptionResults({value, groups, query, gamepad, emptyMessage, layout, icon, selectedLabel}: ResultsProps) {
  useLanguage();
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const sections = groups.map((group, index) => {
    const heading = group.label && (group.localizeLabel === false ? group.label : t(group.label));
    const buttons = group.options.flatMap(option => {
      const label = display(option);
      if (terms.length) {
        const text = [option.label, label, option.value, option.search, group.label, heading].join(' ').toLowerCase();
        if (!terms.every(term => text.includes(term))) return [];
      }
      return <button type="button" className="secondary-btn" key={option.value} value={option.value}
        aria-pressed={option.value === value} autoFocus={gamepad && option.value === value && !option.disabled}
        disabled={option.disabled}>
        {layout === 'list' && icon && <span className={styles.optionIcon}><StudioIcon name={icon} size={18}/></span>}
        <span className={layout === 'list' ? styles.optionName : undefined} title={label} translate="no">{label}</span>
        {layout === 'list' && option.value === value && <>
          {selectedLabel && <span className={styles.selectedLabel}>{selectedLabel}</span>}
          <StudioIcon name="check" size={17}/>
        </>}
      </button>;
    });
    if (!buttons.length) return null;
    return <section key={index} aria-label={group.label}>
      {group.label && <h3 translate={group.localizeLabel === false ? 'no' : undefined}>{group.label}</h3>}
      <div className={layout === 'list' ? styles.options+' '+styles.listOptions : styles.options}>{buttons}</div>
    </section>;
  });
  return sections.some(Boolean) ? <>{sections}</> : <p role="status">{emptyMessage}</p>;
}, (a,b) => a.query === b.query && a.value === b.value && a.gamepad === b.gamepad && a.layout === b.layout &&
  a.icon === b.icon && a.selectedLabel === b.selectedLabel && a.emptyMessage === b.emptyMessage && sameGroups(a.groups, b.groups));

export function OptionList({value, groups, onChoose, onClose, searchLabel = '搜索选项',
  searchPlaceholder = '搜索名称或按键', emptyMessage = '没有匹配的选项。', layout = 'grid', icon, selectedLabel}: OptionListProps) {
  const [query, setQuery] = useState('');
  const results = useRef<HTMLDivElement>(null);
  const gamepad = document.documentElement.dataset.gamepad === 'on';
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
    <div ref={results} className={styles.results} onClick={event => {
      const option = event.target instanceof Element ? event.target.closest('button') : null;
      if (option && event.currentTarget.contains(option) && !option.matches(':disabled')) onChoose(option.value);
    }}>
      <OptionResults value={value} groups={groups} query={query} gamepad={gamepad} layout={layout}
        icon={icon} selectedLabel={selectedLabel} emptyMessage={emptyMessage}/>
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
  let shown = value || t(placeholder);
  for (const group of groups) {
    const current = group.options.find(option => option.value === value);
    if (current) { shown = display(current); break; }
  }
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
