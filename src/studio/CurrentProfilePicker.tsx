import {useId, useMemo, useRef, useState} from 'react';
import {createPortal} from 'react-dom';
import {t} from '../i18n';
import type {CommunityProfile} from '../profile-sharing';
import {CommunityProfiles} from './CommunityProfiles';
import {Dialog} from './Controls';
import {OptionList} from './OptionPicker';
import {StudioIcon} from './StudioIcon';
import type {LibraryGame} from '../types';
import {allGamesValue, gameOf, groupProfiles, preferredProfile} from '../game-library';
import {GameGroups} from './GameGroups';
import gameStyles from './GameGroups.module.css';
import pickerStyles from './OptionPicker.module.css';
import styles from './CurrentProfilePicker.module.css';

export function CurrentProfilePicker({profiles, games, value, disabled, onSelect, onChooseCommunity, onCreate}: {
  profiles: {id: string; name: string}[];
  games: LibraryGame[];
  value: string;
  disabled: boolean;
  onSelect: (id: string) => void;
  onChooseCommunity: (profile: CommunityProfile, gameId:string) => void;
  onCreate: (gameId:string) => void;
}) {
  const [view, setView] = useState<'local' | 'community' | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const valueId = useId();
  const currentGame = gameOf(games, value);
  const [browse, setBrowse] = useState('');
  const byGame = useMemo(() => groupProfiles(profiles, games), [profiles, games]);
  const browsedGame = games.find(game => game.id === browse);
  const destination = browse === allGamesValue ? currentGame?.id ?? '' : browse;
  const preferred = browsedGame && preferredProfile(byGame.get(browse) ?? [], browsedGame);
  const name = profiles.find(profile => profile.id === value)?.name ?? t('选择配置');
  const shown = currentGame ? currentGame.name + ' / ' + name : name;
  const groups = useMemo(() => {
    const ids = browse === allGamesValue ? [...games.map(game => game.id), ''] : [browse];
    return ids.map(id => ({
      label: browse === allGamesValue ? games.find(game => game.id === id)?.name ?? '未归类' : undefined,
      localizeLabel: !id,
      options: (byGame.get(id) ?? []).map(profile => ({value:profile.id, label:profile.name, localize:false as const, disabled})),
    }));
  }, [browse, games, byGame, disabled]);

  function close() {
    setView(null);
    requestAnimationFrame(() => {
      if (!document.querySelector('dialog[open]')) trigger.current?.focus();
    });
  }

  return <>
    <button ref={trigger} type="button" className={pickerStyles.trigger} aria-label="当前配置"
      aria-describedby={valueId} aria-haspopup="dialog" aria-expanded={view !== null}
      disabled={disabled} onClick={() => {setBrowse(currentGame?.id ?? '');setView('local');}}>
      <span id={valueId} title={shown} translate="no">{shown}</span><StudioIcon name="search" size={14}/>
    </button>
    {view && createPortal(<Dialog title="选择配置" className={styles.dialog} onClose={close} actions={<>
      <button type="button" className="secondary-btn" onClick={close}>取消选择</button>
      <button type="button" className="primary-btn" disabled={disabled} onClick={() => {close();onCreate(destination);}}>新建配置</button>
    </>}>
      <div className={styles.sources} role="group" aria-label="配置来源">
        <button type="button" className="secondary-btn" aria-pressed={view === 'local'} onClick={() => setView('local')}>本地配置</button>
        <button type="button" className="secondary-btn" aria-pressed={view === 'community'} onClick={() => setView('community')}>社区配置</button>
      </div>
      {view === 'local'
        ? <div className={gameStyles.split}><GameGroups games={games} groups={byGame} value={browse} onChange={setBrowse}/>
          <div className={gameStyles.content}>
          {preferred && preferred.id !== value && <button type="button" className="secondary-btn" disabled={disabled}
            onClick={() => {close();onSelect(preferred.id);}}><span>{preferred.id === browsedGame?.last_profile ? '继续上次配置' :
              preferred.id === browsedGame?.default_profile ? '加载默认配置' : '加载'}</span> · <span translate="no">{preferred.name}</span></button>}
          <OptionList key={browse} value={value} layout="list" icon="profiles" selectedLabel="当前配置" groups={groups}
          searchLabel="搜索配置名称" searchPlaceholder="搜索配置名称…"
          emptyMessage={groups.some(group=>group.options.length) ? '没有匹配的配置。' : '此分组还没有配置。'}
          onClose={close} onChoose={id => {if(disabled)return;close();onSelect(id);}}/>
          </div></div>
        : <CommunityProfiles embedded blocked={disabled} onChoose={profile => {close();onChooseCommunity(profile, destination);}}/>}
    </Dialog>, document.body)}
  </>;
}
