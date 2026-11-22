import {isSensorOutput} from './sensor-passthrough';
import type {Gyro} from './types';

export function usesJsmAim(gyro:Gyro) {
  return !isSensorOutput(gyro.output) && gyro.motion.mode==='pointer';
}

export function nativeSmoothing(gyro:Gyro) {
  const r=gyro.jsm_response;
  const legacy=gyro.smoothing_ms>0&&gyro.motion.smoothing!=='one_euro';
  return {
    window:r?.smooth_time_ms??(legacy?gyro.smoothing_ms:125),
    threshold:r?.smooth_threshold_dps??(legacy?gyro.motion.decay_threshold:0),
    decay:r?.decay_smoothing??(gyro.motion.smoothing==='decay'),
    oneEuro:r?.one_euro??(gyro.motion.smoothing==='one_euro'),
  };
}

// Stock JSM's zero smoothing threshold bypasses its 125 ms window.
// Preserve game calibration, sensitivity, coordinates and custom scripts.
export function applyJsmResponse(gyro:Gyro) {
  gyro.smoothing_ms=0;
  gyro.noise_threshold=0;
  gyro.acceleration.fast_multiplier=1;
  gyro.acceleration.curve='linear';
  Object.assign(gyro.motion, {
    smoothing:'ema', noise_gate:0, cutoff_dps:0, recovery_ms:0,
    angle_snap:0, angle_snap_ease:false, brake_strength:0,
  });
  if(usesJsmAim(gyro))gyro.jsm_response={...gyro.jsm_response,
    smooth_time_ms:125,smooth_threshold_dps:0,decay_smoothing:false,one_euro:false,cutoff_recovery_dps:0,
  };
}

export function isJsmResponse(gyro:Gyro) {
  const m=gyro.motion;
  if(usesJsmAim(gyro)) {
    const smooth=nativeSmoothing(gyro);
    return smooth.threshold===0&&!smooth.decay&&!smooth.oneEuro&&m.cutoff_dps===0
      &&(gyro.jsm_response?.cutoff_recovery_dps??0)===0&&gyro.acceleration.fast_multiplier===1
      &&gyro.acceleration.curve==='linear'&&m.angle_snap===0&&m.brake_strength===0;
  }
  return m.smoothing==='ema' && gyro.smoothing_ms===0 && gyro.noise_threshold===0
    && gyro.acceleration.fast_multiplier===1 && gyro.acceleration.curve==='linear'
    && m.noise_gate===0 && m.cutoff_dps===0 && m.recovery_ms===0
    && m.angle_snap===0 && m.brake_strength===0;
}
