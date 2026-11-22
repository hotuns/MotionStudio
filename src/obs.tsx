import {useEffect,useState,type CSSProperties} from 'react';
import {createRoot} from 'react-dom/client';
import {setLanguage,useLanguage} from './i18n';
import type {RecordingView} from './controller-overlay';
import {OverlaySurface} from './studio/OverlaySurface';
import './styles/tokens.css';
import './styles/base.css';

function RecordingOverlay() {
  useLanguage();
  const [sample,setSample]=useState<{view:RecordingView;received:number}>();
  useEffect(()=>{
    let disposed=false,nextPoll=0,expiry=0;
    let request:AbortController|undefined;
    let language:RecordingView['language']|undefined;
    const poll=async()=>{
      let delay=750;
      const started=performance.now();
      request=new AbortController();
      const timeout=window.setTimeout(()=>request?.abort(),700);
      try {
        const response=await fetch('/overlay/frame',{headers:{'X-Motion-Overlay':'1'},cache:'no-store',
          credentials:'omit',signal:request.signal});
        if(response.status!==200)throw Error(String(response.status));
        const view:RecordingView=await response.json();
        if(disposed)return;
        if(language!==view.language){setLanguage(view.language);language=view.language;}
        const received=started-(view.status_age_ms??0);
        setSample({view,received});
        window.clearTimeout(expiry);
        if(view.status){
          // A stalled connection must not leave a key lit on the recording.
          expiry=window.setTimeout(()=>setSample(current=>current&&{
            ...current,view:{...current.view,status:null},
          }),Math.max(0,600-(performance.now()-received)));
        }
        delay=view.status?33:150;
      } catch {
        if(!disposed)setSample(undefined);
      } finally {
        window.clearTimeout(timeout);
        if(!disposed)nextPoll=window.setTimeout(poll,delay);
      }
    };
    void poll();
    return ()=>{disposed=true;request?.abort();window.clearTimeout(nextPoll);window.clearTimeout(expiry);};
  },[]);
  if(!sample)return null;
  const {view,received}=sample;
  const [width,height]=view.size;
  const scale=Math.min(1,window.innerWidth/width,window.innerHeight/height);
  const style={width,height,transform:`scale(${scale})`,'--key-mouse-height':width*0.48+'px'} as CSSProperties;
  const ageMs=Math.max(0,performance.now()-received);
  return <OverlaySurface settings={view} device={view.device} live={ageMs<600?view.status??undefined:undefined}
    restarts={view.restart_count} offline={false} width={width} ageMs={ageMs} style={style}/>;
}

createRoot(document.getElementById('obs-root')!).render(<RecordingOverlay/>);
