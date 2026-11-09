const selector='button,a[href],input:not([type="hidden"]),select,textarea,summary,[tabindex]:not([tabindex="-1"])';
export function targets(root:ParentNode):HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(selector)).filter(el=>el instanceof HTMLElement&&!el.matches(':disabled,[aria-disabled="true"]')&&!el.closest('[hidden],[inert]')&&el.getClientRects().length>0&&getComputedStyle(el).visibility!=='hidden');
}
export function scope():HTMLElement {
  const active=document.activeElement?.closest<HTMLElement>('dialog[open]');
  return active??Array.from(document.querySelectorAll<HTMLDialogElement>('dialog[open]')).at(-1)??document.body;
}
export function focus(el:HTMLElement|undefined) {
  if(!el)return;el.focus({preventScroll:true});el.scrollIntoView({block:'nearest',inline:'nearest'});
}
export function move(direction:'up'|'down'|'left'|'right') {
  const root=scope(),all=targets(root),current=document.activeElement as HTMLElement;
  if(!all.includes(current)){focus(all[0]);return;}
  const from=current.getBoundingClientRect(),x=from.x+from.width/2,y=from.y+from.height/2;
  const horizontal=direction==='left'||direction==='right',sign=direction==='left'||direction==='up'?-1:1;
  const choose=(items:HTMLElement[])=>items.filter(el=>el!==current).map(el=>{
    const r=el.getBoundingClientRect(),dx=r.x+r.width/2-x,dy=r.y+r.height/2-y;
    return {el,primary:(horizontal?dx:dy)*sign,cross:Math.abs(horizontal?dy:dx)};
  }).filter(v=>v.primary>4).sort((a,b)=>(a.primary+a.cross*2.5)-(b.primary+b.cross*2.5))[0]?.el;
  const zone=current.closest<HTMLElement>('[data-pad-zone]');
  focus((zone&&choose(all.filter(el=>zone.contains(el))))||choose(all));
}
export function changeZone(delta:number) {
  const root=scope();const zones=Array.from(root.querySelectorAll<HTMLElement>('[data-pad-zone]')).filter(zone=>targets(zone).length>0);
  if(!zones.length){const all=targets(root),i=all.indexOf(document.activeElement as HTMLElement);focus(all[(i+delta+all.length)%all.length]);return;}
    const nearest=document.activeElement?.closest<HTMLElement>('[data-pad-zone]');
    const current=nearest?zones.indexOf(nearest):-1;
  const zone=zones[(Math.max(current,0)+delta+zones.length)%zones.length];
  focus(zone.querySelector<HTMLElement>('[aria-current="page"]')??targets(zone)[0]);
}
export function writeValue(el:HTMLInputElement|HTMLTextAreaElement|HTMLSelectElement,value:string) {
  const prototype=el instanceof HTMLSelectElement?HTMLSelectElement.prototype:el instanceof HTMLTextAreaElement?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(prototype,'value')?.set?.call(el,value);
  el.dispatchEvent(new Event(el instanceof HTMLSelectElement?'change':'input',{bubbles:true}));
}
export function label(el:Element|null):string {
  if(!(el instanceof HTMLElement))return '';
  const aria=el.getAttribute('aria-label');if(aria)return aria;
  if(el instanceof HTMLInputElement||el instanceof HTMLSelectElement||el instanceof HTMLTextAreaElement)return el.labels?.[0]?.textContent?.trim().slice(0,60)??'编辑输入';
  return (el.textContent??'').trim().replace(/\s+/g,' ').slice(0,60);
}
