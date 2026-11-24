import {useEffect, useRef, useState} from 'react';

/** Preview calculations can take longer than a pointer frame. Keep one running and coalesce the rest. */
export function useCurvePreview<T>(key: string | undefined, load: () => Promise<T>) {
  type Job = {key: string; load: () => Promise<T>};
  const queued = useRef<Job>();
  const running = useRef(false);
  const frame = useRef<number>();
  const mounted = useRef(false);
  const [reply, setReply] = useState<{key: string; data?: T}>();
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      queued.current = undefined;
      if (frame.current !== undefined) cancelAnimationFrame(frame.current);
      frame.current = undefined;
    };
  }, []);
  useEffect(() => {
    queued.current = key === undefined ? undefined : {key, load};
    if (!queued.current || running.current || frame.current !== undefined) return;
    async function drain() {
      frame.current = undefined;
      running.current = true;
      while (mounted.current && queued.current) {
        const job = queued.current;
        queued.current = undefined;
        try {
          const data = await job.load();
          if (mounted.current) setReply({key:job.key, data});
        } catch {
          if (mounted.current) setReply({key:job.key});
        }
      }
      running.current = false;
    }
    // Pointer events batched into a frame only need its final position.
    frame.current = requestAnimationFrame(()=>void drain());
  }, [key]);
  return reply;
}
