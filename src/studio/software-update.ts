import { Channel, invoke } from '@tauri-apps/api/core';
import { useCallback, useLayoutEffect, useRef, useState } from 'react';
import type {Settings, UpdateSource} from '../types';

type Mode = 'installed' | 'portable' | 'development';
type Check = { mode: Mode } & (
  { state: 'idle' | 'current' | 'unpublished' }
  | { state: 'available'; version: string; notes: string }
);
type Activity = 'checking' | 'downloading' | 'ready' | 'installing';
type Transfer = { received: number; total: number | null };

export function useStudioUpdate(native: boolean, settings?: Pick<Settings, 'language' | 'update_source'>) {
  const languageSource: UpdateSource = settings?.language === 'en' ? 'github' : 'sealos';
  const source = settings ? settings.update_source ?? languageSource : null;
  const [result, setResult] = useState<Check | null>(null);
  const [activity, setActivity] = useState<Activity | null>(null);
  const [transfer, setTransfer] = useState<Transfer | null>(null);
  const [error, setError] = useState<string | null>(null);
  const inFlight = useRef(false);
  const attempted = useRef(false);
  const generation = useRef(0);
  const previousSource = useRef<UpdateSource | null>(null);
  const release = result?.state === 'available' ? result : null;

  const check = useCallback(async () => {
    if (!native || !source || inFlight.current) return;
    const request = generation.current;
    inFlight.current = true;
    attempted.current = true;
    setActivity('checking');
    setError(null);
    setResult(null);
    try {
      const next = await invoke<Check>('check_studio_update', {source});
      if (request === generation.current) setResult(next);
    } catch (error) {
      if (request === generation.current) setError(String(error));
    } finally {
      if (request === generation.current) {
        inFlight.current = false;
        setActivity(null);
      }
    }
  }, [native, source]);

  useLayoutEffect(() => {
    inFlight.current = false;
    attempted.current = false;
    setActivity(null);
    setResult(null);
    setTransfer(null);
    setError(null);
    if (!native || !source) return;
    const delay = previousSource.current && previousSource.current !== source ? 0 : 2500;
    previousSource.current = source;
    const timer = window.setTimeout(() => { if (!attempted.current) void check(); }, delay);
    return () => { window.clearTimeout(timer); generation.current++; };
  }, [native, source, check]);

  async function download() {
    if (!source || !release || inFlight.current) return;
    const request = generation.current;
    inFlight.current = true;
    setActivity('downloading');
    setTransfer(null);
    setError(null);
    try {
      const progress = new Channel<Transfer>();
      progress.onmessage = next => { if (request === generation.current) setTransfer(next); };
      await invoke('download_studio_update', { source, version: release.version, progress });
      if (request === generation.current) setActivity('ready');
    } catch (error) {
      if (request === generation.current) { setError(String(error)); setActivity(null); }
    } finally {
      if (request === generation.current) inFlight.current = false;
    }
  }

  async function install() {
    if (!source || !release || activity !== 'ready' || inFlight.current) return;
    const request = generation.current;
    inFlight.current = true;
    setActivity('installing');
    setError(null);
    try {
      await invoke('install_studio_update', { source, version: release.version });
      // The current window exits on success.
    } catch (error) {
      if (request === generation.current) {
        setError(String(error));
        setActivity('ready');
        inFlight.current = false;
      }
    }
  }

  return { source, languageSource, result, release, activity, transfer, error, check, download, install };
}

export type StudioUpdate = ReturnType<typeof useStudioUpdate>;
