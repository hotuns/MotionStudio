import {useEffect, useState} from 'react';
import {loadCommunityCatalog, type CommunityProfile} from '../profile-sharing';
import {gamepadNames} from './Controls';
import {OptionPicker} from './OptionPicker';
import {StudioIcon} from './StudioIcon';
import styles from './ProfileManager.module.css';

export function CommunityProfiles({onChoose, onContribute, blocked}: {
  onChoose: (entry: CommunityProfile) => void;
  onContribute: () => void;
  blocked: boolean;
}) {
  const [entries, setEntries] = useState<CommunityProfile[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  const [query, setQuery] = useState('');
  const [output, setOutput] = useState('');
  useEffect(() => {
    const request = new AbortController();
    setLoading(true);
    setError('');
    void loadCommunityCatalog(request.signal, revision > 0).then(next => {
      if (!request.signal.aborted) setEntries(next);
    }).catch(error => {
      if (!request.signal.aborted) setError(error instanceof Error ? error.message : String(error));
    }).finally(() => {
      if (!request.signal.aborted) setLoading(false);
    });
    return () => request.abort();
  }, [revision]);
  const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  const rows = entries.filter(entry => (!output || entry.output === output) &&
    terms.every(term => [entry.name, entry.game, entry.author, entry.controller, entry.code]
      .some(value => value.toLocaleLowerCase().includes(term))));
  return <section className={'app-card ' + styles.profileLibrary}>
    <div className={styles.profileLibraryHeader}>
      <div><h3>社区配置</h3><p>按游戏或手柄查找配置，预览后添加到本地。</p></div>
      <div className={styles.profileLibraryButtons}>
        <button type="button" className="secondary-btn" disabled={loading} onClick={() => setRevision(value => value + 1)}>刷新社区配置</button>
        <button type="button" className="secondary-btn" onClick={onContribute}>社区投稿说明</button>
      </div>
    </div>
    <div className={styles.communityFilters}>
      <label className={styles.profileSearch}>
        <StudioIcon name="search" size={16}/>
        <input type="search" aria-label="搜索游戏、手柄或配置码" placeholder="搜索游戏、手柄或配置码" value={query}
          onChange={event => setQuery(event.target.value)}/>
        <span>{rows.length} / {entries.length}</span>
      </label>
      <OptionPicker label="输出方式" value={output} onChange={setOutput} groups={[{options:[
        {value: '', label: '所有输出方式'},
        ...Object.entries(gamepadNames).map(([value, label]) => ({value, label})),
      ]}]}/>
    </div>
    {loading && <p className="studio-muted" role="status">正在读取社区配置…</p>}
    {error && <p className="studio-note studio-error" role="alert">{error}</p>}
    {rows.length > 0 && <ul className={styles.profileLibraryList}>{rows.map(entry =>
      <li className={styles.profileLibraryItem} key={entry.code}>
        <div className={styles.communityInfo}>
          <strong className={styles.profileNameText} translate="no">{entry.name}</strong>
          <div className={styles.communityMeta}>
            <span translate="no">{entry.game}</span>
            <span translate="no">{entry.controller}</span>
            <span>{gamepadNames[entry.output]}</span>
            <span translate="no">{entry.author}</span>
            <code>{entry.code}</code>
            {!entry.verified && <span>示例配置</span>}
          </div>
          <p className="studio-muted" translate="no">{entry.description}</p>
        </div>
        <button type="button" className="secondary-btn" disabled={blocked} onClick={() => onChoose(entry)}>预览配置</button>
      </li>)}</ul>}
    {!loading && !error && !rows.length && <p className={styles.profileLibraryEmpty}>没有匹配的社区配置。</p>}
  </section>;
}
