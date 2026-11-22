import {lazy,Suspense,type CSSProperties} from 'react';
import type {ControllerOverlayConfig} from '../types';
import type {OverlayDevice,OverlayStatus} from '../controller-overlay';
import {KeyMouseOverlay} from './KeyMouseOverlay';
import styles from './FloatingController.module.css';

const FloatingControllerModel=lazy(()=>import('./FloatingControllerModel'));

export function OverlaySurface({settings,device,live,ageMs,restarts,offline,width,style}:{
  settings:Pick<ControllerOverlayConfig,'content'|'opacity'|'follow_motion'>;
  device:OverlayDevice|null;live?:OverlayStatus;ageMs:number;restarts:number;offline:boolean;
  width:number;style?:CSSProperties;
}) {
  const content=settings.content??'controller';
  return <main className={styles.surface} data-content={content} aria-label="悬浮显示内容"
    style={{opacity:settings.opacity/100,...style}}>
    {content!=='key_mouse'&&<Suspense fallback={null}>
      <FloatingControllerModel key={restarts+':'+device?.id} device={device} live={live} followMotion={settings.follow_motion}
        restarts={restarts} offline={offline}/>
    </Suspense>}
    {content!=='controller'&&<KeyMouseOverlay status={live} ageMs={ageMs} width={width}/>}
  </main>;
}
