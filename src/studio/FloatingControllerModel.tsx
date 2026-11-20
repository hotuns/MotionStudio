import {useCallback,useMemo,useRef,useState} from 'react';
import {Quaternion} from 'three';
import {controllerOrientation,controllerViewPose} from '../controller-pose';
import type {Device,Status} from '../types';
import Controller from './Controller';
import Controller3D,{type ControllerPreviewFailure} from './Controller3D';
import styles from './FloatingController.module.css';

export default function FloatingControllerModel({device,live,followMotion,restarts,offline}:{
  device:Device|null;live?:Status;followMotion:boolean;restarts:number;offline:boolean;
}) {
  const [failure,setFailure]=useState<ControllerPreviewFailure>();
  const origin=useRef<{key:string;rotation:Quaternion}>();
  const unavailable=useCallback((reason:ControllerPreviewFailure)=>setFailure(reason),[]);
  const pose=useMemo(()=>{
    const orientation=followMotion&&device?.gyro?controllerOrientation(live):null;
    if(!orientation){origin.current=undefined;return new Quaternion();}
    const key=restarts+':'+(live?.reconnects??0);
    if(origin.current?.key!==key)origin.current={key,rotation:orientation.clone()};
    return controllerViewPose(orientation,origin.current.rotation);
  },[live,device?.gyro,restarts,followMotion]);
  const notice=!live?(offline?'实时输入连接中断':'等待实时输入')
    :failure?'三维预览不可用，已切换为二维':null;
  return <div className={styles.controller} aria-label="实时悬浮手柄">
    {device ? failure
      ? <div className={styles.fallback}><Controller device={device} status={live}/></div>
      : <Controller3D device={device} status={live} pose={pose} className={styles.model} onUnavailable={unavailable}/>
      : <p className={styles.waiting}>等待手柄输入</p>}
    {device&&notice&&<p className={styles.state}>{notice}</p>}
  </div>;
}
