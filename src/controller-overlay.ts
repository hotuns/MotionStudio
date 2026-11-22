import type {ControllerOverlayConfig,Device,OutputPreviewStatus,Status} from './types';

export const defaultControllerOverlay=():ControllerOverlayConfig=>({
  enabled:false,obs_enabled:false,content:'key_mouse',width:320,opacity:100,position:'bottom_left',monitor:null,follow_motion:true,
});
export type OverlayDevice=Pick<Device,'id'|'name'|'vendor'|'product'|'gyro'|'buttons'>;
export type OverlayStatus=Pick<Status,'state'|'emitting'|'binding_capture'|'ui_navigation'|'reconnects'|'buttons'|'axes'|'touches'>&{
  motion:Pick<Status['motion'],'orientation_quat'>;
  output_preview?:Pick<OutputPreviewStatus,'key_mouse'|'mouse'>|null;
};
export type RecordingView=Pick<ControllerOverlayConfig,'content'|'opacity'|'follow_motion'>&{
  size:[number,number];language:'zh-CN'|'en';device:OverlayDevice|null;status:OverlayStatus|null;
  status_age_ms:number|null;restart_count:number;
};
export type ControllerOverlayView={
  settings:ControllerOverlayConfig;
  language:'zh-CN'|'en';
  device:Device|null;
  status:Status|null;
  status_age_ms?:number|null;
  restart_count:number;
};
export function liveControllerStatus(status:Status|null|undefined) {
  return !!status?.device&&['running','observing','navigating','calibrating','capturing','capture_release'].includes(status.state);
}
