import native from './native-input-defaults.json';
import type {ControllerInputConfig,Profile} from './types';

export const inputDefaults=():ControllerInputConfig=>({lstick_center:[0,0],rstick_center:[0,0],lstick_radial:Array(native.radial_buckets).fill(1),rstick_radial:Array(native.radial_buckets).fill(1),ltrig_min:0,ltrig_max:1,rtrig_min:0,rtrig_max:1,digital_triggers:false,ltrig_digital_threshold:native.trigger_digital_threshold,rtrig_digital_threshold:native.trigger_digital_threshold,trig_hysteresis:native.trigger_hysteresis,trig_debounce_ms:native.trigger_debounce_ms});
export const inputConfig=(profile:Profile):ControllerInputConfig=>({...inputDefaults(),...profile.input});
export function inputDeadzone(profile:Profile,side:'left'|'right'){
  return profile.input?.[side==='left'?'deadzone_left':'deadzone_right']??(profile.stick_mouse.enabled&&profile.stick_mouse.stick===side?profile.stick_mouse.deadzone:profile.stick_deadzone);
}
export function freezeInputDeadzones(profile:Profile){
  const left=inputDeadzone(profile,'left'),right=inputDeadzone(profile,'right');
  profile.input={...inputConfig(profile),deadzone_left:left,deadzone_right:right};
}
export function setInputDeadzone(profile:Profile,side:'left'|'right',value:number){
  freezeInputDeadzones(profile);
  profile.input![side==='left'?'deadzone_left':'deadzone_right']=value;
  if(profile.stick_mouse.stick===side)profile.stick_mouse.deadzone=value;
}
