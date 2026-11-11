import {keyboardKeyFromEvent,keyNames,syncRecordedModifiers} from './keyboard';

type Options = {
  output: boolean;
  onValue: (value:string) => void;
  onPreview: (values:string[]) => void;
  onCancel: () => void;
  onError: (message:string) => void;
  pointerArea?: HTMLElement | null;
  wheelReady?: {current:number};
  cancelOnEmergency?: boolean;
};

/** Reads only events delivered to the current Studio window. */
export function listenForBindingInput({output,onValue,onPreview,onCancel,onError,pointerArea,wheelReady,cancelOnEmergency}:Options) {
  const held = new Set<string>();
  let best:string[] = [];
  let invalid = false;
  let finished = false;
  let timer:number | undefined;
  const commit = (value:string,delay=0) => {
    if (finished) return;
    finished = true;
    if (delay) timer = window.setTimeout(()=>onValue(value),delay);
    else onValue(value);
  };
  const mods = (event:KeyboardEvent|MouseEvent) => syncRecordedModifiers(held,event);
  const preview = () => {
    if (held.size>4) {
      invalid = true;
      onError('组合输出最多支持 4 个按键或鼠标操作，请松开后重试。');
      return;
    }
    if (held.size>=best.length) best = [...held];
    onPreview(best);
  };
  const settle = () => {
    if (held.size) return;
    if (invalid) {
      invalid = false;
      best = [];
      onPreview([]);
    } else if (best.length) {
      commit(best.join('+'),best.some(value=>value.startsWith('mouse:'))?40:0);
    }
  };
  const suppress = (event:Event) => {
    event.preventDefault();
    event.stopImmediatePropagation();
  };
  const keyDown = (event:KeyboardEvent) => {
    if (event.key==='Escape' || (cancelOnEmergency && event.code==='F12')) {
      if (event.key==='Escape') suppress(event);
      finished = true;
      onCancel();
      return;
    }
    if (!output || event.repeat) return;
    suppress(event);
    if (finished) return;
    const key = keyboardKeyFromEvent(event);
    if (keyNames.includes(key) && !event.isComposing) {
      held.add('key:'+key);
      mods(event);
      preview();
    } else {
      invalid = true;
      onError('此按键暂不支持。F12 保留用于紧急停止，Esc 可取消录入。');
    }
  };
  const keyUp = (event:KeyboardEvent) => {
    if (!output) return;
    suppress(event);
    if (finished) return;
    held.delete('key:'+keyboardKeyFromEvent(event));
    mods(event);
    settle();
  };
  const acceptsPointer = (event:Event) => !pointerArea || (event.target instanceof Node && pointerArea.contains(event.target));
  const mouseNames = ['mouse:left','mouse:middle','mouse:right','mouse:x1','mouse:x2'];
  const mouse = (event:MouseEvent) => {
    if (!output) return;
    const name = mouseNames[event.button];
    if (!acceptsPointer(event) && !(event.type==='mouseup' && held.has(name))) return;
    if (event.target instanceof Element && event.target.closest('[data-cancel-capture],button[data-dialog-close]')
      && !(pointerArea && event.type==='mouseup' && held.has(name))) return;
    suppress(event);
    if (finished || !name) return;
    mods(event);
    if (event.type==='mousedown') {held.add(name);preview();}
    else {held.delete(name);settle();}
  };
  const wheel = (event:WheelEvent) => {
    if (!output || !acceptsPointer(event)) return;
    suppress(event);
    if (!event.deltaX && !event.deltaY) return;
    if (wheelReady) {
      const now = performance.now();
      const available = now>=wheelReady.current;
      wheelReady.current = now+220;
      if (!available) return;
    }
    if (finished) return;
    mods(event);
    const part = Math.abs(event.deltaX)>Math.abs(event.deltaY)
      ? (event.deltaX>0?'wheel:right':'wheel:left') : (event.deltaY>0?'wheel:down':'wheel:up');
    const parts = [...held,part];
    if (parts.length>4 || invalid) {
      onError('组合输出最多支持 4 个操作，请松开后重试。');
      return;
    }
    commit(parts.join('+'));
  };
  const preventDefault = (event:Event) => {if (output && acceptsPointer(event)) suppress(event);};

  window.addEventListener('keydown',keyDown,true);
  window.addEventListener('keyup',keyUp,true);
  window.addEventListener('mousedown',mouse,true);
  window.addEventListener('mouseup',mouse,true);
  window.addEventListener('wheel',wheel,{capture:true,passive:false});
  window.addEventListener('auxclick',preventDefault,true);
  window.addEventListener('contextmenu',preventDefault,true);
  return () => {
    window.removeEventListener('keydown',keyDown,true);
    window.removeEventListener('keyup',keyUp,true);
    window.removeEventListener('mousedown',mouse,true);
    window.removeEventListener('mouseup',mouse,true);
    window.removeEventListener('wheel',wheel,true);
    window.removeEventListener('auxclick',preventDefault,true);
    window.removeEventListener('contextmenu',preventDefault,true);
    if (timer!==undefined) window.clearTimeout(timer);
  };
}
