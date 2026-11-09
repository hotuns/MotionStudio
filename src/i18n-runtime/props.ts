import {Fragment} from 'react';
import {t} from '../i18n';
const localizeChildren=(value:unknown):unknown=>typeof value==='string'?t(value):Array.isArray(value)?value.map(localizeChildren):value;
/** Translate React text/accessible labels at element creation, never mutate DOM,
 * controlled values, event handlers, identifiers or saved user configuration. */
export function localizeProps(type:unknown,props:unknown):unknown {
 if(!props||typeof props!=='object'||(typeof type!=='string'&&type!==Fragment))return props;
 const source=props as Record<string,unknown>;
 if(source.translate==='no')return props;
 const translated={...source};
 if(type==='optgroup'&&typeof source.label==='string')translated.label=t(source.label);
 if('children' in source)translated.children=localizeChildren(source.children);
 for(const name of ['aria-label','aria-description','aria-valuetext','title','placeholder','alt']) {
   if(typeof source[name]==='string')translated[name]=t(source[name] as string);
 }
 return translated;
}
