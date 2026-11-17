import {useCallback,useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import type {ComponentStatus} from '../types';

export function useComponentStatus(native:boolean,settingsPage:boolean) {
  const [components,setComponents]=useState<ComponentStatus|null>(null);
  const [checking,setChecking]=useState(false);
  const [error,setError]=useState<string|null>(null);
  const request=useRef<Promise<ComponentStatus>|null>(null);
  const refresh=useCallback(()=>{
    if(request.current)return request.current;
    setChecking(true);setError(null);
    const next=invoke<ComponentStatus>('component_status').then(data=>{
      setComponents(data);return data;
    }).catch(error=>{
      setComponents(null);setError(String(error));throw error;
    }).finally(()=>{request.current=null;setChecking(false);});
    request.current=next;return next;
  },[]);
  // Check once at startup and when entering settings, never on the input poll.
  useEffect(()=>{if(native)void refresh().catch(()=>{});},[native,refresh]);
  useEffect(()=>{if(native&&settingsPage)void refresh().catch(()=>{});},[native,settingsPage,refresh]);
  return {components,checking,error,refresh};
}
