import type {LibraryGame} from '../types';
import {allGamesValue, newGameValue} from '../game-library';
import {StudioIcon} from './StudioIcon';
import styles from './GameGroups.module.css';

export function GameGroups({games, groups, value, onChange}: {
  games: LibraryGame[]; groups: Map<string, {id:string}[]>; value: string; onChange: (id:string) => void;
}) {
  const row = (id:string, name:string, count:number) => <button key={id} type="button" aria-pressed={id === value}
    onClick={() => onChange(id)} className={styles.game}>
    <StudioIcon name={id ? 'games' : 'folder'} size={16}/>
    <span translate={id && id !== allGamesValue ? 'no' : undefined} title={name}>{name}</span><small>{count}</small>
  </button>;
  return <nav className={styles.games} aria-label="按游戏浏览">
    {games.length > 0 && row(allGamesValue, '全部游戏', [...groups.values()].reduce((count, rows) => count + rows.length, 0))}
    {games.map(game => row(game.id, game.name, groups.get(game.id)?.length ?? 0))}
    {row('', '未归类', groups.get('')?.length ?? 0)}
  </nav>;
}

export function GameDestination({games, value, onChange, allowNew = false, disabled = false, newName, onNewName}: {
  games: LibraryGame[]; value:string; onChange:(id:string)=>void; allowNew?:boolean; disabled?:boolean;
  newName?:string; onNewName?:(value:string)=>void;
}) {
  return <><label className={styles.destination}>所属游戏
    <select value={value} disabled={disabled} onChange={event => onChange(event.target.value)}>
      <option value="">未归类</option>
      {games.map(game => <option key={game.id} value={game.id} translate="no">{game.name}</option>)}
      {allowNew && <option value={newGameValue}>新建游戏…</option>}
    </select>
  </label>
  {value === newGameValue && onNewName && <label className={styles.destination}>游戏名称
    <input autoFocus required maxLength={100} value={newName ?? ''} disabled={disabled} onChange={event=>onNewName(event.target.value)}/>
  </label>}</>;
}
