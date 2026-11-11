import { Channel, invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useRef, useState } from 'react';

type Mode = 'installed' | 'portable' | 'development';
type Check = { mode: Mode } & (
  { state: 'idle' | 'current' | 'unpublished' }
  | { state: 'available'; version: string; notes: string }
);
type Activity = 'checking' | 'downloading' | 'ready' | 'installing';
type Transfer = { received: number; total: number | null };

export function useStudioUpdate(native: boolean) {
  const [result, setResult] = useState<Check | null>(null);
  const [activity, setActivity] = useState<Activity | null>(null);
  const [transfer, setTransfer] = useState<Transfer | null>(null);
  const [error, setError] = useState<string | null>(null);
  const inFlight = useRef(false);
  const attempted = useRef(false);
  const release = result?.state === 'available' ? result : null;

  const check = useCallback(async () => {
    if (!native || inFlight.current) return;
    inFlight.current = true;
    attempted.current = true;
    setActivity('checking');
    setError(null);
    setResult(null);
    try {
      setResult(await invoke<Check>('check_studio_update'));
    } catch (error) {
      setError(String(error));
    } finally {
      inFlight.current = false;
      setActivity(null);
    }
  }, [native]);

  useEffect(() => {
    if (!native) return;
    const timer = window.setTimeout(() => { if (!attempted.current) void check(); }, 2500);
    return () => window.clearTimeout(timer);
  }, [native, check]);

  async function download() {
    if (!release || inFlight.current) return;
    inFlight.current = true;
    setActivity('downloading');
    setTransfer(null);
    setError(null);
    try {
      const progress = new Channel<Transfer>();
      progress.onmessage = setTransfer;
      await invoke('download_studio_update', { version: release.version, progress });
      setActivity('ready');
    } catch (error) {
      setError(String(error));
      setActivity(null);
    } finally {
      inFlight.current = false;
    }
  }

  async function install() {
    if (!release || activity !== 'ready' || inFlight.current) return;
    inFlight.current = true;
    setActivity('installing');
    setError(null);
    try {
      await invoke('install_studio_update', { version: release.version });
      // The current window exits on success.
    } catch (error) {
      setError(String(error));
      setActivity('ready');
      inFlight.current = false;
    }
  }

  return { result, release, activity, transfer, error, check, download, install };
}

export type StudioUpdate = ReturnType<typeof useStudioUpdate>;
