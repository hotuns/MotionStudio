import {useCallback,useEffect,useLayoutEffect,useMemo,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {Quaternion} from 'three';
import {setLanguage,useLanguage} from '../i18n';
import type {Device,Status} from '../types';
import {controllerOrientation,controllerViewPose} from '../controller-pose';
import {liveControllerStatus,type ControllerOverlayView} from '../controller-overlay';
import Controller3D,{type ControllerPreviewFailure} from './Controller3D';
import Controller from './Controller';
import styles from './FloatingController.module.css';

export function FloatingController() {
  useLanguage();
  const [view,setView]=useState<ControllerOverlayView>();
  const [sample,setSample]=useState<{status:Status|null;received:number}>();
  const [failure,setFailure]=useState<ControllerPreviewFailure>();
  const [offline,setOffline]=useState(false);
  const lastDevice=useRef<Device|null>(null);
  const origin=useRef<{key:string;rotation:Quaternion}>();
  const unavailable=useCallback((reason:ControllerPreviewFailure)=>setFailure(reason),[]);

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
        if(serial===requestSerial)setSample({status:next.status,received:performance.now()});
      }catch{
        if(!disposed){setOffline(true);setSample(undefined);}
      }finally{pending=false;}
    };
    const subscription=listen<Status>('engine-status',({payload})=>{
      if(!disposed){serial++;setSample({status:liveControllerStatus(payload)?payload:null,received:performance.now()});}
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

  const live=sample?.status&&liveControllerStatus(sample.status)?sample.status:undefined;
  const device=live?.device??view?.device??lastDevice.current;
  if(device)lastDevice.current=device;
  const key=(device?.path??'')+':'+(view?.restart_count??0)+':'+(live?.reconnects??0);
  // Reset before the new renderer's passive effect can report a graphics error.
  useLayoutEffect(()=>{setFailure(undefined);},[device?.path]);
  const pose=useMemo(()=>{
    const orientation=view?.settings.follow_motion&&device?.gyro?controllerOrientation(live):null;
    if(!orientation){origin.current=undefined;return new Quaternion();}
    if(origin.current?.key!==key)origin.current={key,rotation:orientation.clone()};
    return controllerViewPose(orientation,origin.current.rotation);
  },[live,device?.gyro,key,view?.settings.follow_motion]);
  const notice=!live?(offline?'实时输入连接中断':'等待实时输入')
    :failure?'三维预览不可用，已切换为二维':null;

  return <main className={styles.surface} aria-label="实时悬浮手柄" style={{opacity:(view?.settings.opacity??100)/100}}>
    {device ? failure
      ? <div className={styles.fallback}><Controller device={device} status={live}/></div>
      : <Controller3D device={device} status={live} pose={pose} className={styles.model} onUnavailable={unavailable}/>
      : <p className={styles.waiting}>等待手柄输入</p>}
    {device&&notice&&<p className={styles.state}>{notice}</p>}
  </main>;
}
