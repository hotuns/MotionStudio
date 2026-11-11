import {useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {setLanguage,useLanguage} from '../i18n';
import {BatchBindings,type BatchProgress,type BindingWindowState} from './BatchBindings';
import styles from './BatchBindings.module.css';

export function FloatingBindings() {
  useLanguage();
  const [session,setSession] = useState<BindingWindowState>();
  const [progress,setProgress] = useState<BatchProgress>();
  const [busy,setBusy] = useState(false);
  const [error,setError] = useState<string>();
  const updates = useRef<Promise<void>>(Promise.resolve());

  useEffect(()=>{
    let disposed = false;
    void invoke<BindingWindowState>('binding_window_state',{id:null}).then(session=>{
      if (disposed) return;
      setLanguage(session.language);
      document.documentElement.dataset.theme = session.theme;
      setSession(session);
      setProgress(session.progress);
    }).catch(error=>{if (!disposed) setError(String(error));});
    return ()=>{disposed=true;};
  },[]);

  if (!session || !progress) return <main className={styles.surfaceHeading}>
    <h1>悬浮窗录入</h1>
    <p role={error?'alert':'status'}>{error??'正在载入动作列表…'}</p>
  </main>;

  function sync(id:string,progress:BatchProgress,finish:'return'|'keep'|null) {
    const next = updates.current.catch(()=>{}).then(()=>
      invoke<void>('update_binding_window',{id,progress,finish})
    );
    updates.current = next;
    return next;
  }
  const change = (next:BatchProgress) => {
    setProgress(next);
    void sync(session.id,next,null).catch(error=>setError(String(error)));
  };
  const finish = (next:BatchProgress,outcome:'return'|'keep') => {
    setBusy(true);
    setError(undefined);
    void sync(session.id,next,outcome).catch(error=>{
      setError(String(error));
      setBusy(false);
    });
  };

  return <BatchBindings floating scope={session.scope} split={session.split}
    progress={progress} onProgress={change} busy={busy} error={error}
    onClose={next=>finish(next,'return')} onKeep={next=>finish(next,'keep')}/>;
}
