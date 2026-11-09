import catalogue from './keyboard-keys.json';

export const keyboardKeys=catalogue;
export const keyNames=keyboardKeys.map(key=>key.name);
export const modifierOutputs=keyboardKeys.filter(key=>key.modifier!==0).map(key=>'key:'+key.name);

export function keyboardKeyFromEvent(event:Pick<KeyboardEvent,'code'|'key'>):string {
  const physical=keyboardKeys.find(key=>key.code===event.code);
  if(physical)return physical.name;
  // Older embedded browsers may omit code. Never collapse a known numpad
  // location or accept the reserved F12 through the logical-key fallback.
  if(event.code)return '';
  const logical=event.key===' '?'SPACE':event.key==='Escape'?'ESC':event.key==='Control'?'CTRL':event.key.toUpperCase();
  return keyNames.includes(logical)?logical:'';
}

export function syncRecordedModifiers(held:Set<string>,event:Pick<KeyboardEvent,'ctrlKey'|'altKey'|'shiftKey'|'metaKey'>) {
  const groups:[boolean,string[]][]=[
    [event.ctrlKey,['key:CTRL','key:RCTRL']],[event.altKey,['key:ALT','key:RALT']],
    [event.shiftKey,['key:SHIFT','key:RSHIFT']],[event.metaKey,['key:WIN','key:RWIN']],
  ];
  for(const [down,keys] of groups){
    if(!down)keys.forEach(key=>held.delete(key));
    else if(!keys.some(key=>held.has(key)))held.add(keys[0]);
  }
}
