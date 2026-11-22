import type {GamepadTarget, Profile} from './types';

export function isSensorOutput(output:unknown):output is 'ds4_imu'|'dualsense_imu' {
  return output==='ds4_imu' || output==='dualsense_imu';
}

export function sensorOutputLabel(output:'ds4_imu'|'dualsense_imu') {
  return output==='dualsense_imu'?'DualSense 传感器直通':'DS4 传感器直通';
}

// The virtual device is shared by the base profile and all layers. Change
// only explicit passthrough destinations and leave inherited settings alone.
function layerSensorSettings(profile:Profile) {
  return (profile.layers??[]).flatMap(layer=>{
    const gyro=layer.settings.gyro;
    return gyro && typeof gyro==='object' && 'output' in gyro && isSensorOutput(gyro.output)?[gyro]:[];
  });
}

export function profileUsesSensorOutput(profile:Profile) {
  return isSensorOutput(profile.gyro.output) || layerSensorSettings(profile).length>0;
}

export function setGamepadTarget(profile:Profile,target:GamepadTarget) {
  profile.virtual_gamepad=target;
  if(target!=='none') profile.gamepad_bindings??=[];
  if(target!=='dualshock_4' && target!=='dualsense')return;
  const output=target==='dualsense'?'dualsense_imu':'ds4_imu';
  if(isSensorOutput(profile.gyro.output))profile.gyro.output=output;
  for(const gyro of layerSensorSettings(profile))gyro.output=output;
}
