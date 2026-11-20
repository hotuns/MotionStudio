import type {Gyro} from './types';

export function usesJsmAim(gyro:Gyro) {
  return gyro.aim_engine==='jsm' && gyro.output!=='ds4_imu' && gyro.motion.mode==='pointer';
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
}

export function isJsmResponse(gyro:Gyro) {
  const m=gyro.motion;
  return m.smoothing==='ema' && gyro.smoothing_ms===0 && gyro.noise_threshold===0
    && gyro.acceleration.fast_multiplier===1 && gyro.acceleration.curve==='linear'
    && m.noise_gate===0 && m.cutoff_dps===0 && m.recovery_ms===0
    && m.angle_snap===0 && m.brake_strength===0;
}
