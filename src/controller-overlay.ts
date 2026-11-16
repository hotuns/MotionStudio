import type {ControllerOverlayConfig,Device,Status} from './types';

export const defaultControllerOverlay=():ControllerOverlayConfig=>({
  enabled:false,width:320,opacity:100,position:'bottom_left',monitor:null,follow_motion:true,
});
export type ControllerOverlayView={
  settings:ControllerOverlayConfig;
  language:'zh-CN'|'en';
  device:Device|null;
  status:Status|null;
  restart_count:number;
};
export function liveControllerStatus(status:Status|null|undefined) {
  return !!status?.device&&['running','observing','navigating','calibrating','capturing','capture_release'].includes(status.state);
}
