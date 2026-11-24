import {useEffect, useRef, useState, type KeyboardEvent, type PointerEvent} from 'react';

export type CurveCoordinate = {readonly 0: number; readonly 1: number};
export type PlotRange = readonly [xMax: number, yMax: number];
type Picked<P> = {points: P[]; index: number};
type Options<P> = {
  points: P[];
  range: PlotRange;
  move: (points: P[], index: number, x: number, y: number) => P[];
  insert?: (points: P[], x: number, y: number) => Picked<P>;
  fixed?: (index: number, points: P[]) => boolean;
  removable?: (index: number, points: P[]) => boolean;
  onChange: (points: P[]) => void;
  keyStep?: (index: number, fine: boolean) => PlotRange;
  initialSelection?: number;
  contextKey?: string;
};
type Gesture<P> = Picked<P> & {
  pointer: number; startedAt: number; key: string; previous: number; start: PlotRange; range: PlotRange;
};
type PlotEvent = PointerEvent<HTMLDivElement>;

function position(event: PlotEvent, [width, height]: PlotRange): PlotRange {
  const box = event.currentTarget.getBoundingClientRect();
  return [(event.clientX-box.left)*width/box.width, (box.bottom-event.clientY)*height/box.height];
}

/** Keep a drag local until release, including a point inserted by the same gesture. */
export function useCurveHandles<P extends CurveCoordinate>(options: Options<P>) {
  const {points, move, insert, fixed, removable, onChange} = options;
  const key = JSON.stringify([options.contextKey, points]);
  const [selection, setSelection] = useState(options.initialSelection ?? 0);
  const [working, setWorking] = useState<P[]>();
  const surface = useRef<HTMLDivElement>(null);
  const gesture = useRef<Gesture<P> | null>(null);
  const frame = useRef<number>();
  const active = gesture.current?.key === key ? gesture.current : null;
  const visible = active && working ? working : points;
  const range = active?.range ?? options.range;
  const selected = active?.index ?? Math.min(selection, visible.length-1);

  function selectAndFocus(index: number) {
    setSelection(index);
    if (frame.current !== undefined) cancelAnimationFrame(frame.current);
    frame.current = requestAnimationFrame(() => {
      frame.current = undefined;
      surface.current?.querySelector<HTMLButtonElement>('[data-curve-point="'+index+'"]')?.focus({preventScroll:true});
    });
  }
  function end() {
    const old = gesture.current;
    gesture.current = null;
    setWorking(undefined);
    if (old && surface.current?.hasPointerCapture(old.pointer)) surface.current.releasePointerCapture(old.pointer);
  }
  function cancel() {
    if (!gesture.current) return;
    if (frame.current !== undefined) { cancelAnimationFrame(frame.current); frame.current = undefined; }
    setSelection(gesture.current.previous);
    end();
  }
  useEffect(() => {
    window.addEventListener('blur', cancel);
    return () => {
      window.removeEventListener('blur', cancel);
      if (frame.current !== undefined) cancelAnimationFrame(frame.current);
    };
  }, []);
  useEffect(() => { if (gesture.current && gesture.current.key !== key) cancel(); }, [key]);

  function remove(index: number) {
    if (!removable?.(index, points)) return;
    onChange(points.filter((_, i) => i !== index));
    selectAndFocus(Math.min(index, points.length-2));
  }
  function start(event: PlotEvent) {
    if (gesture.current || !event.isPrimary || event.button !== 0 || event.currentTarget.closest('fieldset:disabled')) return;
    const start = position(event, range);
    const hit = (event.target as Element).closest<HTMLButtonElement>('[data-curve-point]');
    let picked: Picked<P> | undefined;
    if (hit) {
      const box = event.currentTarget.getBoundingClientRect();
      const distance = (p: P) => ((p[0]-start[0])*box.width/range[0])**2 + ((p[1]-start[1])*box.height/range[1])**2;
      const index = points.reduce((best, p, i) => distance(p)<distance(points[best]) ? i : best, Number(hit.dataset.curvePoint));
      picked = {points, index};
    } else picked = insert?.(points, ...start);
    if (!picked || picked.index < 0) return;
    event.preventDefault();
    selectAndFocus(picked.index);
    if (fixed?.(picked.index, picked.points)) return;
    gesture.current = {...picked, start, range, key, pointer:event.pointerId, startedAt:event.timeStamp, previous:selected};
    event.currentTarget.setPointerCapture(event.pointerId);
    setWorking(picked.points);
  }
  function track(event: PlotEvent, finish: boolean) {
    const g = gesture.current;
    if (!g || g.pointer !== event.pointerId) return;
    if (g.key !== key || event.currentTarget.closest('fieldset:disabled')) { cancel(); return; }
    const [x,y] = position(event, g.range);
    const p = g.points[g.index];
    const nx = Math.max(0,Math.min(g.range[0],p[0]+(x-g.start[0])));
    const ny = Math.max(0,Math.min(g.range[1],p[1]+(y-g.start[1])));
    const next = move(g.points, g.index, nx, ny);
    if (finish) {
      end();
      if (JSON.stringify(next) !== JSON.stringify(points)) onChange(next);
    } else setWorking(next);
  }
  function keys(event: KeyboardEvent<HTMLDivElement>) {
    if (gesture.current && event.key === 'Escape') {
      event.preventDefault(); event.stopPropagation(); cancel(); return;
    }
    const hit = (event.target as Element).closest<HTMLButtonElement>('[data-curve-point]');
    if (!hit || hit.matches(':disabled') || gesture.current || event.ctrlKey || event.altKey || event.metaKey) return;
    const index = Number(hit.dataset.curvePoint);
    const p = points[index];
    const increment = event.shiftKey ? 0.001 : 0.01;
    const [dx,dy] = options.keyStep?.(index, event.shiftKey) ?? [range[0]*increment, range[1]*increment];
    let next = points;
    switch (event.key) {
      case 'ArrowLeft': next = move(points, index, p[0]-dx, p[1]); break;
      case 'ArrowRight': next = move(points, index, p[0]+dx, p[1]); break;
      case 'ArrowDown': next = move(points, index, p[0], p[1]-dy); break;
      case 'ArrowUp': next = move(points, index, p[0], p[1]+dy); break;
      case 'Delete': case 'Backspace': remove(index); break;
      default: return;
    }
    event.preventDefault(); event.stopPropagation();
    if (next !== points) onChange(next);
  }
  return {
    visible, selected, range, dragging: !!active, dragId:active?.startedAt, surface, setSelection, selectAndFocus, remove,
    handlers: {
      onPointerDown: start, onPointerMove: (e: PlotEvent) => track(e,false), onPointerUp: (e: PlotEvent) => track(e,true),
      onPointerCancel: (e: PlotEvent) => { if (gesture.current?.pointer === e.pointerId) cancel(); },
      onLostPointerCapture: (e: PlotEvent) => { if (gesture.current?.pointer === e.pointerId && !e.currentTarget.hasPointerCapture(e.pointerId)) cancel(); },
      onKeyDown: keys,
    },
  };
}
