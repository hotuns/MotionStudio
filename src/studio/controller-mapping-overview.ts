import type {Action,GamepadBinding,MotionBinding,Profile,TurnBinding} from '../types';
import {chordParts} from '../types';
import {editingProfile,layerSettingOverridden} from '../layers';
import layout from '../virtual-gamepad-layout.json';

export function physicalControl(input:string):string {
  if(input==='touch1'||input==='touch2'||input.startsWith('touch_swipe_'))return 'touchpad';
  return ['left_stick','right_stick','left_trigger','right_trigger'].find(control=>input.startsWith(control+'_'))??input;
}
type Origin={id:string;index:number;layerId:string;controls:string[]};
export type OverviewEntry=Origin&(
  {kind:'binding';value:Action}|{kind:'gamepad';value:GamepadBinding}|
  {kind:'motion';value:MotionBinding}|{kind:'turn';value:TurnBinding}|
  {kind:'setting';setting:'stick_mouse'|'flick'|'gyro';input:string}
);
const inputKey=(input:string)=>chordParts(input).sort().join('+');
const controls=(input:string)=>input.trim()?[...new Set(chordParts(input).map(physicalControl))]:['unbound'];

// Read-only projection of an unsaved draft, with only the selected layer active.
// Keep source indexes for editing; never feed this projection back into a profile.
export function buildMappingOverview(base:Profile,layerId='') {
  const layer=base.layers?.find(layer=>layer.id===layerId);
  const profile=editingProfile(base,layerId);
  const entries:OverviewEntry[]=[];
  const hidden=new Set(layer?[...layer.disabled_inputs,...layer.actions.map(a=>a.input)].map(inputKey):[]);
  base.actions.forEach((value,index)=>{
    if(!hidden.has(inputKey(value.input)))entries.push({kind:'binding',value,index,id:'base:binding:'+index,layerId:'',controls:controls(value.input)});
  });
  layer?.actions.forEach((value,index)=>entries.push({kind:'binding',value,index,id:'layer:binding:'+index,layerId:layer.id,controls:controls(value.input)}));
  const source=(path:string)=>layer&&layerSettingOverridden(layer,path)?layer.id:'';
  const padOutputs=profile.virtual_gamepad==='none'?[]:[...layout.buttons,...layout.sticks,...layout.triggers,
    ...(profile.virtual_gamepad==='dualshock_4'||profile.virtual_gamepad==='dualsense'?layout.dualshock_4:[])];
  // None means the native standard layout; [] means explicitly no mappings.
  const gamepad=profile.gamepad_bindings??padOutputs.map(output=>({input:output,output}));
  gamepad.forEach((value,index)=>{
    if(padOutputs.includes(value.output))entries.push({kind:'gamepad',value,index,id:'gamepad:'+index,layerId:source('gamepad_bindings'),controls:controls(value.input)});
  });
  profile.gyro.motion.turns.forEach((value,index)=>entries.push({kind:'turn',value,index,id:'turn:'+index,layerId:source('gyro.motion.turns'),controls:controls(value.input)}));
  const passthrough=profile.gyro.output==='ds4_imu';
  if(!passthrough)profile.gyro.motion.bindings.forEach((value,index)=>entries.push({kind:'motion',value,index,id:'motion:'+index,layerId:source('gyro.motion.bindings'),controls:['motion']}));
  if(profile.stick_mouse.enabled)entries.push({kind:'setting',setting:'stick_mouse',input:profile.stick_mouse.stick+'_stick',index:0,id:'setting:stick_mouse',layerId:source('stick_mouse'),controls:[profile.stick_mouse.stick+'_stick']});
  if(!passthrough&&profile.gyro.motion.flick.enabled)entries.push({kind:'setting',setting:'flick',input:profile.gyro.motion.flick.stick+'_stick',index:0,id:'setting:flick',layerId:source('gyro.motion.flick'),controls:[profile.gyro.motion.flick.stick+'_stick']});
  if(profile.gyro.enabled)entries.push({kind:'setting',setting:'gyro',input:profile.gyro.activation,index:0,id:'setting:gyro',layerId:source('gyro'),controls:['motion']});
  return {profile,layer,entries};
}

export const overviewGroups=[
  {id:'left_top',name:'左肩键与扳机',controls:['left_trigger','left_shoulder'],side:'left',anchor:[270,80]},
  {id:'left_stick',name:'左摇杆',controls:['left_stick'],side:'left',anchor:[292,298]},
  {id:'dpad',name:'方向键',controls:['dpad_up','dpad_left','dpad_right','dpad_down'],side:'left',anchor:[424,456]},
  {id:'right_top',name:'右肩键与扳机',controls:['right_trigger','right_shoulder'],side:'right',anchor:[860,80]},
  {id:'face',name:'面部按键',controls:['north','west','east','south'],side:'right',anchor:[830,298]},
  {id:'right_stick',name:'右摇杆',controls:['right_stick'],side:'right',anchor:[698,450]},
  {id:'center',name:'中央按键与触摸板',controls:['back','guide','start','touchpad','misc1','misc2'],side:'extra',anchor:null},
  {id:'extra',name:'背键与扩展键',controls:['left_paddle1','left_paddle2','right_paddle1','right_paddle2','misc3','misc4','misc5','misc6'],side:'extra',anchor:null},
  {id:'motion',name:'体感与其他',controls:['motion'],side:'extra',anchor:null},
] as const;
export type OverviewGroup=typeof overviewGroups[number];
export function entriesForGroup(entries:OverviewEntry[],group:OverviewGroup) {
  return entries.filter(entry=>entry.controls.some(control=>(group.controls as readonly string[]).includes(control)
    || group.id==='motion'&&!overviewGroups.some(g=>(g.controls as readonly string[]).includes(control))));
}
