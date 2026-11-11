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

  return <div className={styles.libraryPage}>
    <div className={styles.libraryToolbar}>
      <div className={styles.libraryViews} role="group" aria-label="配置来源">
        <button type="button" className="secondary-btn" aria-pressed={view === 'local'} onClick={() => onView('local')}>本地配置</button>
        <button type="button" className="secondary-btn" aria-pressed={view === 'community'} onClick={() => onView('community')}>社区配置</button>
      </div>
      <button type="button" className="secondary-btn" disabled={blocked} onClick={onImportCode}>输入配置码</button>
      <button type="button" className="primary-btn" disabled={blocked} onClick={onCreate}>新建配置</button>
      <details className={styles.fileTools}>
        <summary><StudioIcon name="folder" size={16}/><span>导入与文件</span></summary>
        <div className={styles.operationMenu} onClick={event=>{
          if ((event.target as Element).closest('button')) event.currentTarget.closest('details')?.removeAttribute('open');
        }}>
          <button type="button" disabled={blocked} onClick={onImport}>导入 TOML 配置</button>
          <button type="button" disabled={blocked} onClick={onImportJsm}>导入 JSM 配置</button>
          <button type="button" onClick={onFolder}>打开配置文件夹</button>
          <button type="button" onClick={onSwitching}>自动切换</button>
        </div>
      </details>
    </div>
    {view === 'community' ? <CommunityProfiles onChoose={onChooseCommunity} onContribute={onContribute} blocked={blocked}/> :
    <section className={styles.profileCard} aria-label="本地配置">
      <div className={styles.profileLibrary}>
        <p className={styles.libraryHint}>加载到工作区后，可以继续编辑或应用到运行中的引擎。</p>
        <label className={styles.profileSearch}>
          <StudioIcon name="search" size={16} />
          <input type="search" aria-label="搜索配置名称" placeholder="搜索配置名称…" value={query} onChange={event => setQuery(event.target.value)} />
          <span>{rows.length} / {entries.length}</span>
        </label>
        <div className={styles.listHeading} aria-hidden="true"><span>配置名称</span><span>输出方式</span><span>操作</span></div>
        <ul className={styles.profileLibraryList}>{rows.map(entry => {
          const current = selected?.id === entry.id;
          const name = current ? selected.profile.name : entry.profile.name;
          const profile = current ? selected.profile : entry.profile;
          return <li className={styles.profileLibraryItem + ' ' + styles.localItem + (current ? ' ' + styles.profileLibraryItemActive : '')} key={entry.id}>
            <div className={styles.profileLibraryName}>
              <span className={styles.profileIcon}><StudioIcon name="profiles" size={18} /></span>
              {current && renameId === entry.id ? <input autoFocus aria-label="重命名配置" value={name} maxLength={100}
                onChange={event => onRename(event.target.value)} onBlur={() => setRenameId(null)}
                onKeyDown={event => { if (event.key === 'Enter') { event.preventDefault(); setRenameId(null); } }} />
                : <span className={styles.profileNameText} translate="no">{name}</span>}
              {current && <span className={styles.profileLibraryActiveBadge}>当前配置</span>}
              {activeId === entry.id && <span className={styles.profileLibraryActiveBadge}>引擎使用中</span>}
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
                  <button type="button" disabled={blocked} onClick={() => onExport(entry)}>导出</button>
                  {current && <button type="button" disabled={blocked || dirty} onClick={() => onRestore(entry)}>恢复上一份保存</button>}
                  <button type="button" className={styles.deleteAction} disabled={blocked} aria-label={'删除配置 ' + name} onClick={() => onDelete(entry)}>删除</button>
                </div>
              </details>
            </div>
          </li>;
        })}</ul>
        {!rows.length && <p className={styles.profileLibraryEmpty}>{entries.length ? '没有匹配的配置。' : '配置库为空，可以新建或导入配置。'}</p>}
      </div>
    </section>}
  </div>;
}
