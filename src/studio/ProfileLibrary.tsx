import { useMemo, useState } from 'react';
import type { Entry, GameEdit, LibraryGame } from '../types';
import {allGamesValue, gameOf, groupProfiles, newGameValue} from '../game-library';
import {t} from '../i18n';
import type { CommunityProfile } from '../profile-sharing';
import { CommunityProfiles } from './CommunityProfiles';
import { StudioIcon } from './StudioIcon';
import { Dialog, gamepadNames } from './Controls';
import {GameDestination, GameGroups} from './GameGroups';
import gameStyles from './GameGroups.module.css';
import styles from './ProfileManager.module.css';

type LibraryDialog = {kind:'rename'|'ungroup'; game:LibraryGame} | {kind:'move'; ids:string[]} | {kind:'duplicate'; entry:Entry};

export function ProfileLibrary({ entries, games, browse, onBrowse, onEdit, onDuplicate, onNewGame, selected, activeId, blocked, dirty, view, onView, onChooseCommunity, onContribute, onImportCode,
  onCopyCode, onCopyGame, onSelect, onRename, onCreate, onImport, onImportJsm, onExport, onDelete, onRestore, onSwitching, onFolder }: {
  entries: Entry[];
  games: LibraryGame[];
  browse: string;
  onBrowse: (id:string) => void;
  onEdit: (edit:GameEdit) => Promise<void>;
  onDuplicate: (entry:Entry, name:string) => Promise<void>;
  onNewGame: () => void;
  selected: Entry | null;
  activeId?: string | null;
  blocked: boolean;
  dirty: boolean;
  view: 'local' | 'community';
  onView: (view: 'local' | 'community') => void;
  onChooseCommunity: (entry: CommunityProfile) => void;
  onContribute: () => void;
  onImportCode: () => void;
  onCopyCode: (entry: Entry) => void;
  onCopyGame: (game: LibraryGame) => void;
  onSelect: (entry: Entry) => void;
  onRename: (name: string) => void;
  onCreate: () => void;
  onImport: () => void;
  onImportJsm: () => void;
  onExport: (entry: Entry) => void;
  onDelete: (entry: Entry) => void;
  onRestore: (entry: Entry) => void;
  onSwitching: () => void;
  onFolder: () => void;
}) {
  const [query, setQuery] = useState('');
  const [renameId, setRenameId] = useState<string | null>(null);
  const [checked, setChecked] = useState<string[]>([]);
  const [dialog, setDialog] = useState<LibraryDialog|null>(null);
  const [error, setError] = useState('');
  const byGame = useMemo(() => groupProfiles(entries, games), [entries, games]);
  const game = games.find(game => game.id === browse);
  const members = browse === allGamesValue ? entries : byGame.get(browse) ?? [];
  const rows = members.filter(entry => {
    const name = (entry.id === selected?.id ? selected : entry).profile.name;
    return [name, gameOf(games,entry.id)?.name].join(' ').toLowerCase().includes(query.trim().toLowerCase());
  });
  const chosen = members.filter(entry => checked.includes(entry.id)).map(entry => entry.id);
  const open = (next:LibraryDialog) => {setError('');setDialog(next);};
  function browseGame(id:string) { setChecked([]);setQuery('');setError('');onBrowse(id); }

  return <div className={styles.libraryPage}>
    <div className={styles.libraryToolbar}>
      <div className={styles.libraryViews} role="group" aria-label="配置来源">
        <button type="button" className="secondary-btn" aria-pressed={view === 'local'} onClick={() => onView('local')}>本地配置</button>
        <button type="button" className="secondary-btn" aria-pressed={view === 'community'} onClick={() => onView('community')}>社区配置</button>
      </div>
      <button type="button" className="secondary-btn" disabled={blocked} onClick={onImportCode}>输入配置码</button>
      <button type="button" className="secondary-btn" disabled={blocked} onClick={onNewGame}>新建游戏</button>
      <button type="button" className="primary-btn" disabled={blocked} onClick={onCreate}>新建配置</button>
      <details className={styles.fileTools}>
        <summary><StudioIcon name="folder" size={16}/><span>导入与文件</span></summary>
        <div className={styles.operationMenu} onClick={event=>{
          if ((event.target as Element).closest('button')) event.currentTarget.closest('details')?.removeAttribute('open');
        }}>
          <button type="button" disabled={blocked} onClick={onImport}>导入 TOML 配置</button>
          <button type="button" disabled={blocked} onClick={onImportJsm}>导入文本配置</button>
          <button type="button" onClick={onFolder}>打开配置文件夹</button>
          <button type="button" onClick={onSwitching}>自动切换</button>
        </div>
      </details>
    </div>
    {view === 'community' ? <CommunityProfiles onChoose={onChooseCommunity} onContribute={onContribute} blocked={blocked}/> :
    <section className={styles.profileCard} aria-label="本地配置">
      <div className={gameStyles.split}>
        <GameGroups games={games} groups={byGame} value={browse} onChange={browseGame}/>
        <div className={styles.profileLibrary}>
        <div className={gameStyles.actions}>
          <h3 translate={game ? 'no' : undefined}>{game?.name ?? (browse === allGamesValue ? '全部游戏' : '未归类')}</h3>
          {game && <button type="button" className="secondary-btn" disabled={blocked || !game.profile_ids.length}
            onClick={() => onCopyGame(game)}>复制整组配置码</button>}
          {game && <details className={styles.itemTools}><summary aria-label="游戏操作"><StudioIcon name="more" size={17}/></summary>
            <div className={styles.operationMenu} onClick={e=>e.currentTarget.closest('details')?.removeAttribute('open')}>
              <button type="button" disabled={blocked} onClick={()=>open({kind:'rename',game})}>重命名游戏</button>
              <button type="button" disabled={blocked} onClick={()=>open({kind:'ungroup',game})}>解散游戏分组</button>
            </div></details>}
        </div>
        <label className={styles.profileSearch}>
          <StudioIcon name="search" size={16} />
          <input type="search" aria-label="搜索配置名称" placeholder="搜索配置名称…" value={query} onChange={event => setQuery(event.target.value)} />
          <span>{rows.length} / {members.length}</span>
        </label>
        {rows.length > 0 && <div className={gameStyles.actions}>
          <label className={gameStyles.check}><input type="checkbox" disabled={blocked}
            checked={rows.every(entry => checked.includes(entry.id))}
            onChange={event => setChecked(event.target.checked ? [...new Set([...checked,...rows.map(e=>e.id)])] : checked.filter(id=>!rows.some(e=>e.id===id)))}/>全选</label>
          {chosen.length > 0 && <button type="button" className="secondary-btn" disabled={blocked} onClick={()=>open({kind:'move',ids:chosen})}>移至游戏 <span>({chosen.length})</span></button>}
        </div>}
        {error && !dialog && <p role="alert">{error}</p>}
        <div className={styles.listHeading} aria-hidden="true"><span>配置名称</span><span>输出方式</span><span>操作</span></div>
        <ul className={styles.profileLibraryList}>{rows.map(entry => {
          const current = selected?.id === entry.id;
          const owner = gameOf(games,entry.id);
          const name = current ? selected.profile.name : entry.profile.name;
          const profile = current ? selected.profile : entry.profile;
          return <li className={styles.profileLibraryItem + ' ' + styles.localItem + (current ? ' ' + styles.profileLibraryItemActive : '')} key={entry.id}>
            <div className={styles.profileLibraryName}>
              <input type="checkbox" aria-label={t('选择配置')+' '+name} checked={checked.includes(entry.id)} disabled={blocked}
                onChange={event=>setChecked(event.target.checked ? [...checked,entry.id] : checked.filter(id=>id!==entry.id))}/>
              {current && renameId === entry.id ? <input autoFocus aria-label="重命名配置" value={name} maxLength={100}
                onChange={event => onRename(event.target.value)} onBlur={() => setRenameId(null)}
                onKeyDown={event => { if (event.key === 'Enter') { event.preventDefault(); setRenameId(null); } }} />
                : <span className={styles.profileNameText} translate="no">{name}</span>}
              {current && <span className={styles.profileLibraryActiveBadge}>当前配置</span>}
              {activeId === entry.id && <span className={styles.profileLibraryActiveBadge}>引擎使用中</span>}
              {browse === allGamesValue && <small translate={owner ? 'no' : undefined}>{owner?.name ?? '未归类'}</small>}
              {owner?.default_profile === entry.id && <span className={styles.profileLibraryActiveBadge}>默认配置</span>}
            </div>
            <span className={styles.outputType}>{gamepadNames[profile.virtual_gamepad]}</span>
            <div className={styles.profileLibraryButtons}>
              {!current && <button type="button" className="secondary-btn" disabled={blocked} onClick={() => onSelect(entry)}>加载</button>}
              {current && <button type="button" className="secondary-btn" disabled={blocked} onClick={() => setRenameId(entry.id)}>重命名</button>}
              <details className={styles.itemTools}>
                <summary aria-label={'配置操作 ' + name} title="更多操作"><StudioIcon name="more" size={17}/></summary>
                <div className={styles.operationMenu} onClick={event=>{
                  if ((event.target as Element).closest('button')) event.currentTarget.closest('details')?.removeAttribute('open');
                }}>
                  <button type="button" disabled={blocked} onClick={() => onCopyCode(current ? selected : entry)}>复制配置码</button>
                  <button type="button" disabled={blocked} onClick={()=>open({kind:'duplicate',entry:current ? selected : entry})}>创建配置副本</button>
                  <button type="button" disabled={blocked} onClick={()=>open({kind:'move',ids:[entry.id]})}>移至游戏</button>
                  {owner && owner.default_profile !== entry.id &&
                    <button type="button" disabled={blocked} onClick={()=>void onEdit({kind:'set_default',id:owner.id,profile_id:entry.id}).catch(e=>setError(String(e)))}>设为默认配置</button>}
                  <button type="button" disabled={blocked} onClick={() => onExport(entry)}>导出</button>
                  {current && <button type="button" disabled={blocked || dirty} onClick={() => onRestore(entry)}>恢复上一份保存</button>}
                  <button type="button" className={styles.deleteAction} disabled={blocked} aria-label={'删除配置 ' + name} onClick={() => onDelete(entry)}>删除</button>
                </div>
              </details>
            </div>
          </li>;
        })}</ul>
        {!rows.length && <p className={styles.profileLibraryEmpty}>{members.length ? '没有匹配的配置。' : '此分组还没有配置。'}</p>}
        </div></div>
    </section>}
    {dialog && <LibraryOperation dialog={dialog} games={games} busy={blocked} error={error} onClose={()=>setDialog(null)}
      onSubmit={async (name,target) => {
        try {
          if (dialog.kind === 'duplicate') await onDuplicate(dialog.entry,name);
          else if (dialog.kind === 'move') {
            await onEdit({kind:'move',profile_ids:dialog.ids,game_id:target && target !== newGameValue ? target : undefined,
              new_game_name:target === newGameValue ? name : undefined});
            setChecked([]);
          } else if (dialog.kind === 'rename') await onEdit({kind:'rename',id:dialog.game.id,name});
          else {await onEdit({kind:'ungroup',id:dialog.game.id});browseGame('');}
          setDialog(null);
        } catch (e) {setError(String(e));}
      }}/>}
  </div>;
}

function LibraryOperation({dialog, games, busy, error, onClose, onSubmit}: {
  dialog:LibraryDialog; games:LibraryGame[]; busy:boolean; error:string; onClose:()=>void; onSubmit:(name:string,target:string)=>Promise<void>;
}) {
  const [name,setName] = useState(dialog.kind === 'rename' ? dialog.game.name :
    dialog.kind === 'duplicate' ? dialog.entry.profile.name.slice(0,87)+' · '+t('副本') : '');
  const [target,setTarget] = useState(() => {
    if (dialog.kind !== 'move') return '';
    const owner = gameOf(games,dialog.ids[0])?.id;
    return games.find(game=>game.id !== owner)?.id ?? (owner ? '' : newGameValue);
  });
  const title = dialog.kind === 'rename' ? '重命名游戏' : dialog.kind === 'ungroup' ? '解散游戏分组' : dialog.kind === 'move' ? '移至游戏' : '创建配置副本';
  const naming = dialog.kind === 'duplicate' || dialog.kind === 'rename' || (dialog.kind === 'move' && target === newGameValue);
  return <Dialog title={title} error={error} onClose={()=>{if(!busy)onClose();}} onSubmit={()=>{if(!busy)void onSubmit(name.trim(),target);}} actions={<>
    <button type="button" className="secondary-btn" disabled={busy} onClick={onClose}>取消</button>
    <button type="submit" className="primary-btn" disabled={busy || (naming && !name.trim())}>{title}</button>
  </>}>
    {dialog.kind === 'move' && <GameDestination games={games} value={target} onChange={setTarget} allowNew disabled={busy}/>}
    {naming && <label className={gameStyles.destination}>{dialog.kind === 'duplicate' ? '配置名称' : '游戏名称'}
      <input autoFocus required maxLength={100} value={name} onChange={e=>setName(e.target.value)}/></label>}
    {dialog.kind === 'ungroup' && <p><strong translate="no">{dialog.game.name}</strong> · <span>配置将移到「未归类」，该游戏的专用切换规则将移除。</span></p>}
    {dialog.kind === 'move' && <p>移动后，原游戏中指向这些配置的专用切换规则会移除。</p>}
  </Dialog>;
}
