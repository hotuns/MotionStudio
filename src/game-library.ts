import type {LibraryGame} from './types';

export const noGames: LibraryGame[] = [];
export const newGameValue = '@new';
export const allGamesValue = '@all';

export function importDestination(id:string, name:string) {
  return {gameId:id && id !== newGameValue ? id : null, newGameName:id === newGameValue ? name.trim() : null};
}

export function availableGameName(name: string, games: LibraryGame[]) {
  const base = name.trim();
  const taken = new Set(games.map(game => game.name.trim().toLowerCase()));
  let candidate = base;
  for (let copy = 2; taken.has(candidate.toLowerCase()); copy++) {
    const suffix = ` (${copy})`;
    candidate = [...base].slice(0, 100 - suffix.length).join('') + suffix;
  }
  return candidate;
}

export function gameOf(games: LibraryGame[], profileId?: string) {
  return profileId ? games.find(game => game.profile_ids.includes(profileId)) : undefined;
}

export function groupProfiles<T extends {id:string}>(profiles: T[], games: LibraryGame[]) {
  const owners = new Map(games.flatMap(game => game.profile_ids.map(id => [id, game.id] as const)));
  const groups = new Map<string, T[]>([['', []], ...games.map(game => [game.id, [] as T[]] as const)]);
  for (const profile of profiles) groups.get(owners.get(profile.id) ?? '')!.push(profile);
  return groups;
}

export function preferredProfile<T extends {id:string}>(profiles: T[], game?: LibraryGame) {
  return profiles.find(p => p.id === game?.last_profile) ?? profiles.find(p => p.id === game?.default_profile) ?? profiles[0];
}
