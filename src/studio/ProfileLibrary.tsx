import { useState } from 'react';
import type { Entry } from '../types';
import type { CommunityProfile } from '../profile-sharing';
import { CommunityProfiles } from './CommunityProfiles';
import { StudioIcon } from './StudioIcon';
import { gamepadNames } from './Controls';
import styles from './ProfileManager.module.css';

export function ProfileLibrary({ entries, selected, activeId, blocked, dirty, view, onView, onChooseCommunity, onContribute, onImportCode,
  onCopyCode, onSelect, onRename, onCreate, onImport, onImportJsm, onExport, onDelete, onRestore, onSwitching, onFolder }: {
  entries: Entry[];
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
  const rows = entries.filter(entry => (entry.id === selected?.id ? selected : entry).profile.name
    .toLowerCase().includes(query.trim().toLowerCase()));

  return <div>
    <div className="studio-profile-tools">
      <div className={styles.libraryViews} role="group" aria-label="配置来源">
        <button type="button" className="secondary-btn" aria-pressed={view === 'local'} onClick={() => onView('local')}>本地配置</button>
        <button type="button" className="secondary-btn" aria-pressed={view === 'community'} onClick={() => onView('community')}>社区配置</button>
      </div>
      <button type="button" className="primary-btn" disabled={blocked} onClick={onImportCode}>输入配置码</button>
      <button type="button" className="secondary-btn" onClick={onSwitching}>自动切换</button>
      <button type="button" className="secondary-btn" onClick={onFolder}>打开配置文件夹</button>
    </div>
    {view === 'community' ? <CommunityProfiles onChoose={onChooseCommunity} onContribute={onContribute} blocked={blocked}/> :
    <section className={'app-card ' + styles.profileCard}>
      <div className={styles.profileLibrary}>
        <div className={styles.profileLibraryHeader}>
          <div><h3>本地配置</h3><p>加载到工作区后，可以继续编辑或应用到运行中的引擎。</p></div>
        </div>
        <div className={styles.profileLibraryActionsRow}>
          <button type="button" className="secondary-btn" disabled={blocked} onClick={onImport}>导入 TOML 配置</button>
          <button type="button" className="secondary-btn" disabled={blocked} onClick={onImportJsm}>导入 JSM 配置</button>
          <button type="button" className="primary-btn" disabled={blocked} onClick={onCreate}>新建配置</button>
        </div>
        <label className={styles.profileSearch}>
          <StudioIcon name="search" size={16} />
          <input type="search" aria-label="搜索配置名称" placeholder="搜索配置名称…" value={query} onChange={event => setQuery(event.target.value)} />
          <span>{rows.length} / {entries.length}</span>
        </label>
        <ul className={styles.profileLibraryList}>{rows.map(entry => {
          const current = selected?.id === entry.id;
          const name = current ? selected.profile.name : entry.profile.name;
          return <li className={styles.profileLibraryItem + (current ? ' ' + styles.profileLibraryItemActive : '')} key={entry.id}>
            <div className={styles.profileLibraryName}>
              <span className={styles.profileIcon}><StudioIcon name="profiles" size={18} /></span>
              {current && renameId === entry.id ? <input autoFocus aria-label="重命名配置" value={name} maxLength={100}
                onChange={event => onRename(event.target.value)} onBlur={() => setRenameId(null)}
                onKeyDown={event => { if (event.key === 'Enter') { event.preventDefault(); setRenameId(null); } }} />
                : <span className={styles.profileNameText} translate="no">{name}</span>}
              {current && <span className={styles.profileLibraryActiveBadge}>当前配置</span>}
              {activeId === entry.id && <span className={styles.profileLibraryActiveBadge}>引擎使用中</span>}
              <span className="studio-muted">{gamepadNames[entry.profile.virtual_gamepad]}</span>
            </div>
            <div className={styles.profileLibraryButtons}>
              {!current && <button type="button" className="secondary-btn" disabled={blocked} onClick={() => onSelect(entry)}>加载</button>}
              {current && <button type="button" className="secondary-btn" disabled={blocked} onClick={() => setRenameId(entry.id)}>重命名</button>}
              <button type="button" className="secondary-btn" disabled={blocked}
                onClick={() => onCopyCode(current ? selected : entry)}>复制配置码</button>
              <button type="button" className="secondary-btn" disabled={blocked} onClick={() => onExport(entry)}>导出</button>
              {current && <button type="button" className="secondary-btn" disabled={blocked || dirty} onClick={() => onRestore(entry)}>恢复上一份保存</button>}
              <button type="button" className="danger-btn" disabled={blocked} aria-label={'删除配置 ' + name} onClick={() => onDelete(entry)}>删除</button>
            </div>
          </li>;
        })}</ul>
        {!rows.length && <p className={styles.profileLibraryEmpty}>{entries.length ? '没有匹配的配置。' : '配置库为空，可以新建或导入配置。'}</p>}
      </div>
    </section>}
  </div>;
}
