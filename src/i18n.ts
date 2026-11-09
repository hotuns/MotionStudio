import {useSyncExternalStore} from 'react';
import english from './locales/en.json';
export type Language='zh-CN'|'en';
function initialLanguage():Language {try{return localStorage.getItem('motion-studio-language')==='en'?'en':'zh-CN';}catch{return 'zh-CN';}}
let language:Language=initialLanguage();
const subscribers=new Set<()=>void>();
const catalog:Record<string,string>=english;
const escape=(value:string)=>value.replace(/[.*+?^${}()|[\]\\]/g,'\\$&');
const patterns=Object.entries(catalog).filter(([source])=>/\{\d+\}/.test(source)).map(([source,target])=>{
 const parts=source.split(/\{\d+\}/),slots=[...source.matchAll(/\{(\d+)\}/g)].map(match=>Number(match[1]));
 return {regex:new RegExp('^'+parts.map(escape).join('([\\s\\S]*?)')+'$'),target,slots,weight:parts.join('').length};
}).sort((a,b)=>b.weight-a.weight);
export function getLanguage(){return language;}
export function setLanguage(next:Language){
 if(next!=='en'&&next!=='zh-CN')return;
 try{localStorage.setItem('motion-studio-language',next);}catch{/* Settings remain authoritative when storage is disabled. */}
 if(typeof document!=='undefined')document.documentElement.lang=next;
 if(language===next)return;language=next;for(const callback of subscribers)callback();
}
export function useLanguage(){return useSyncExternalStore(callback=>{subscribers.add(callback);return()=>subscribers.delete(callback);},getLanguage,getLanguage);}
export function t(value:string):string {
 if(language!=='en')return value;
 const normalized=value.trim().replace(/\s+/g,' ');
 if(normalized.startsWith('Error: '))return 'Error: '+t(normalized.slice(7));
 const exact=catalog[normalized];
 if(exact!==undefined)return value.replace(value.trim(),exact);
 for(const {regex,target,slots} of patterns){
  const match=regex.exec(normalized);if(match){const params=new Map(slots.map((slot,index)=>[slot,t(match[index+1])]));return target.replace(/\{(\d+)\}/g,(_,slot)=>params.get(Number(slot))??'');}
 }
 return value;
}
