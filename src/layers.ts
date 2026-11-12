import {t} from './i18n';
import type {Profile,ProfileLayer} from './types';
const object=(value:unknown):value is Record<string,unknown>=>!!value&&typeof value==='object'&&!Array.isArray(value);
export function mergeSettings(target:Record<string,unknown>,patch:Record<string,unknown>){
  for(const [key,value] of Object.entries(patch)) {
    if(object(value)&&Object.keys(value).length===1&&value.clear===true)target[key]=null;
    else if(object(value)&&object(target[key]))mergeSettings(target[key] as Record<string,unknown>,value);
    else target[key]=structuredClone(value);
  }
}
function difference(before:unknown,after:unknown):unknown {
  if(JSON.stringify(before)===JSON.stringify(after))return undefined;
  if(after==null)return {clear:true};
  if(object(before)&&object(after)){
    const patch:Record<string,unknown>={};for(const key of new Set([...Object.keys(before),...Object.keys(after)])) {
      const value=difference(before[key],after[key]);if(value!==undefined)patch[key]=value;
    }return patch;
  }return structuredClone(after);
}
export function editingProfile(base:Profile,id:string):Profile {
  const profile=structuredClone(base),layer=base.layers?.find(layer=>layer.id===id);
  if(layer){mergeSettings(profile as unknown as Record<string,unknown>,layer.settings);profile.actions=structuredClone(layer.actions);}
  return profile;
}
export function updateLayer(base:Profile,id:string,change:(profile:Profile)=>void){
  const layer=base.layers?.find(layer=>layer.id===id);
  if(!layer){change(base);return;}
  const before=editingProfile(base,id),after=structuredClone(before);change(after);
  for(const key of ['gyro','gamepad_bindings','input','feedback','touchpad','stick_mouse','stick_deadzone','stick_zones','stick_rebound_ms'] as const) {
    const delta=difference(before[key],after[key]);if(delta!==undefined) {
      if(object(delta)&&object(layer.settings[key]))mergePatch(layer.settings[key] as Record<string,unknown>,delta);
      else layer.settings[key]=delta;
    }
  }
  layer.actions=after.actions;
}
// Patches retain explicit clear markers; merging an editor delta must not apply
// those markers until the resulting profile is resolved.
function mergePatch(target:Record<string,unknown>,patch:Record<string,unknown>){
  for(const [key,value] of Object.entries(patch)){
    if(object(value)&&value.clear!==true&&object(target[key]))mergePatch(target[key] as Record<string,unknown>,value);
    else target[key]=structuredClone(value);
  }
}
export function newLayer():ProfileLayer{return {id:crypto.randomUUID(),name:t('新配置层'),input:'left_shoulder',activation_threshold:0.5,activation_hysteresis:0.1,settings:{},actions:[],disabled_inputs:[]};}

export function layerSettingOverridden(layer: ProfileLayer, path: string): boolean {
  let value: unknown = layer.settings;
  for (const key of path.split('.')) {
    if (!object(value)) return value !== undefined;
    if (value.clear === true) return true;
    if (!Object.hasOwn(value,key)) return false;
    value = value[key];
  }
  return value !== undefined;
}

// Delete the patch itself. Assigning the base value would leave an override
// behind and prevent future edits of the base layer from being inherited.
export function restoreLayerSetting(layer: ProfileLayer, path: string) {
  const remove = (patch: Record<string,unknown>, keys: string[]) => {
    const [key,...rest] = keys;
    if (!Object.hasOwn(patch,key)) return;
    if (!rest.length || !object(patch[key]) || patch[key].clear === true) delete patch[key];
    else {
      remove(patch[key],rest);
      if (!Object.keys(patch[key]).length) delete patch[key];
    }
  };
  remove(layer.settings,path.split('.'));
}
