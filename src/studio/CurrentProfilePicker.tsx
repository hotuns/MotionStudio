import {useId, useRef, useState} from 'react';
import {createPortal} from 'react-dom';
import {t} from '../i18n';
import type {CommunityProfile} from '../profile-sharing';
import {CommunityProfiles} from './CommunityProfiles';
import {Dialog} from './Controls';
import {OptionList} from './OptionPicker';
import {StudioIcon} from './StudioIcon';
import pickerStyles from './OptionPicker.module.css';
import styles from './ProfileManager.module.css';

export function CurrentProfilePicker({profiles, value, disabled, onSelect, onChooseCommunity, onCreate}: {
  profiles: {id: string; name: string}[];
  value: string;
  disabled: boolean;
  onSelect: (id: string) => void;
  onChooseCommunity: (profile: CommunityProfile) => void;
  onCreate: () => void;
}) {
  const [view, setView] = useState<'local' | 'community' | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const valueId = useId();
  const shown = profiles.find(profile => profile.id === value)?.name ?? t('选择配置');

  function close() {
    setView(null);
    requestAnimationFrame(() => {
      if (!document.querySelector('dialog[open]')) trigger.current?.focus();
    });
  }

  return <>
    <button ref={trigger} type="button" className={pickerStyles.trigger} aria-label="当前配置"
      aria-describedby={valueId} aria-haspopup="dialog" aria-expanded={view !== null}
      disabled={disabled} onClick={() => setView('local')}>
      <span id={valueId} title={shown} translate="no">{shown}</span><StudioIcon name="search" size={14}/>
    </button>
    {view && createPortal(<Dialog title="选择配置" className={pickerStyles.dialog} onClose={close} actions={<>
      <button type="button" className="secondary-btn" onClick={close}>取消选择</button>
      <button type="button" className="primary-btn" disabled={disabled} onClick={() => {close();onCreate();}}>新建配置</button>
    </>}>
      <div className={`${styles.libraryViews} ${styles.pickerViews}`} role="group" aria-label="配置来源">
        <button type="button" className="secondary-btn" aria-pressed={view === 'local'} onClick={() => setView('local')}>本地配置</button>
        <button type="button" className="secondary-btn" aria-pressed={view === 'community'} onClick={() => setView('community')}>社区配置</button>
      </div>
      {view === 'local'
        ? <OptionList value={value} groups={[{options: profiles.map(profile => ({
          value: profile.id, label: profile.name, localize: false, disabled,
        }))}]} searchLabel="搜索配置名称" searchPlaceholder="搜索配置名称…"
          emptyMessage={profiles.length ? '没有匹配的配置。' : '配置库为空，可以新建或导入配置。'}
          onClose={close} onChoose={id => {close();onSelect(id);}}/>
        : <CommunityProfiles embedded blocked={disabled} onChoose={profile => {close();onChooseCommunity(profile);}}/>}
    </Dialog>, document.body)}
  </>;
}
