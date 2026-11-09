import {useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {Dialog} from './Controls';
import styles from './ImportJsm.module.css';
type Listing={directory:string;parent:string|null;places:{name:string;path:string}[];items:{name:string;path:string;directory:boolean}[];truncated:boolean};
export function decodeConfig(bytes:Uint8Array):{text:string;encoding:string} {
  if(bytes.length>512*1024)throw new Error('配置文件超过 512 KB');
  let text='',encoding='UTF-8';
  if(bytes[0]===255&&bytes[1]===254){encoding='UTF-16 LE';text=new TextDecoder('utf-16le',{fatal:true}).decode(bytes);}
  else if(bytes[0]===254&&bytes[1]===255){encoding='UTF-16 BE';text=new TextDecoder('utf-16be',{fatal:true}).decode(bytes);}
  else{try{text=new TextDecoder('utf-8',{fatal:true}).decode(bytes);}catch{encoding='GB18030';text=new TextDecoder('gb18030',{fatal:true}).decode(bytes);}}
  if(text.includes('\0'))throw new Error('请选择文本配置文件。');
  return {text,encoding};
}
export function ImportFiles({kind,allowLegacy=false,onClose,onSelect}:{kind:'jsm'|'toml';allowLegacy?:boolean;onClose:()=>void;onSelect:(text:string,name:string,encoding:string)=>void}) {
  const [listing,setListing]=useState<Listing|null>(null),[path,setPath]=useState(''),[directory,setDirectory]=useState<string|null>(null),[error,setError]=useState(''),[busy,setBusy]=useState(false);
  useEffect(()=>{let disposed=false;setBusy(true);setError('');void invoke<Listing>('browse_import_files',{directory}).then(data=>{if(!disposed){setListing(data);setPath(data.directory.replace(/^\\\\\?\\/,''));}}).catch(e=>{if(!disposed)setError(String(e));}).finally(()=>{if(!disposed)setBusy(false);});return()=>{disposed=true;};},[directory]);
  const choose=(file:string)=>{setBusy(true);setError('');void invoke<{name:string;bytes:number[]}>('read_import_file',{path:file}).then(data=>{const decoded=decodeConfig(new Uint8Array(data.bytes));onSelect(decoded.text,data.name,decoded.encoding);}).catch(e=>setError(String(e))).finally(()=>setBusy(false));};
  const items=listing?.items.filter(item=>item.directory
    || (kind==='toml'||allowLegacy) && /\.toml$/i.test(item.name)
    || kind==='jsm' && /\.(txt|jsm|cfg|conf)$/i.test(item.name))??[];
  return <Dialog title={kind==='jsm'?'选择 JSM 配置文件':'选择 Motion Studio 配置'} onClose={onClose} error={error} onSubmit={()=>{if(path.trim())setDirectory(path.trim());}}
    description="方向键或左摇杆移动，A 打开文件夹或选择文件，B 返回。支持 UTF-8、UTF-16 与中文 ANSI 文本。"
    actions={<button type="button" className="secondary-btn" onClick={onClose}>取消选择</button>}>
    <div className={styles.path}><label>文件夹路径<input aria-label="文件夹路径" value={path} onChange={e=>setPath(e.target.value)}/></label><button type="submit" className="secondary-btn" disabled={busy||!path.trim()}>进入</button></div>
    <div className={styles.places}>{listing?.places.map(p=><button type="button" className="secondary-btn" key={p.path} disabled={busy} onClick={()=>setDirectory(p.path)}>{p.name}</button>)}</div>
    <button type="button" className="secondary-btn" disabled={busy||!listing?.parent} onClick={()=>setDirectory(listing?.parent??null)}>上一级文件夹</button>
    {busy&&<p role="status">正在读取…</p>}
    <div className={styles.files} aria-label="配置文件列表">{items.map(item=><button type="button" key={item.path} disabled={busy} aria-label={(item.directory?'打开文件夹 ':'选择文件 ')+item.name}
      onClick={()=>item.directory?setDirectory(item.path):choose(item.path)}><span aria-hidden="true">{item.directory?'▸':'▤'}</span>{item.name}</button>)}</div>
    {!busy&&!items.length&&<p>此文件夹没有可选的配置文件。</p>}
    {listing?.truncated&&<p>此文件夹较大，仅显示前 1000 个项目。可直接输入更具体的文件夹路径。</p>}
  </Dialog>;
}
