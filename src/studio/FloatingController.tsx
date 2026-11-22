import {useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {setLanguage,useLanguage} from '../i18n';
import type {Device,Status} from '../types';
import {liveControllerStatus,type ControllerOverlayView} from '../controller-overlay';
import {OverlaySurface} from './OverlaySurface';

export function FloatingController() {
  useLanguage();
  const [view,setView]=useState<ControllerOverlayView>();
  const [sample,setSample]=useState<{status:Status|null;received:number}>();
  const [offline,setOffline]=useState(false);
  const lastDevice=useRef<Device|null>(null);

  useEffect(()=>{
    document.documentElement.dataset.controllerOverlay='true';
    document.documentElement.dataset.theme='dark';
    let disposed=false,pending=false,shown=false;
    let serial=0;
    const update=async()=>{
      if(pending)return;
      pending=true;
      const requestSerial=serial;
      try {
        const next=await invoke<ControllerOverlayView>('controller_overlay_state');
        if(disposed)return;
        setLanguage(next.language);
        setView(next);
        setOffline(false);
        if(next.settings.enabled&&!shown){
          await invoke('controller_overlay_ready');
          shown=true;
        }
        // An event received while this request was in flight is newer.
        if(!disposed&&serial===requestSerial)setSample({status:next.status,received:performance.now()-(next.status_age_ms??0)});
      }catch{
        if(!disposed&&serial===requestSerial){setOffline(true);setSample(undefined);}
      }finally{pending=false;}
    };
    const subscription=listen<Status>('engine-status',({payload})=>{
      if(!disposed){serial++;setOffline(false);setSample({status:liveControllerStatus(payload)?payload:null,received:performance.now()});}
    });
    void subscription.catch(()=>{});
    void update();
    const poll=window.setInterval(()=>{
      setSample(current=>current&&performance.now()-current.received>600?undefined:current);
      void update();
    },250);
    return ()=>{
      disposed=true;
      clearInterval(poll);
      void subscription.then(unlisten=>unlisten()).catch(()=>{});
      delete document.documentElement.dataset.controllerOverlay;
    };
  },[]);

  const ageMs=sample?Math.max(0,performance.now()-sample.received):Infinity;
  const live=ageMs<600&&sample?.status&&liveControllerStatus(sample.status)?sample.status:undefined;
  const device=live?.device??view?.device??lastDevice.current;
  if(device)lastDevice.current=device;
  if(!view?.settings.enabled)return null;
  return <OverlaySurface settings={view.settings} device={device} live={live} ageMs={ageMs}
    restarts={view.restart_count} offline={offline} width={window.innerWidth}/>;
}
