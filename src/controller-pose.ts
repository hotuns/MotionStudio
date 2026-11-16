import {Quaternion} from 'three';
import type {Status} from './types';

export function controllerOrientation(status?: Status | null) {
  const values=status?.motion.orientation_quat;
  if(!values||values.length!==4||!values.every(Number.isFinite))return null;
  const pose=new Quaternion().fromArray(values);
  return pose.lengthSq()>0.01?pose.normalize():null;
}

// Canonical-to-Y-up basis from FlexInput core/frames.rs. Origins are local
// display references; recentering a preview never changes the input engine.
export function controllerViewPose(orientation:Quaternion,origin?:Quaternion) {
  const q=origin?orientation.clone().premultiply(origin.clone().invert()):orientation;
  return new Quaternion(-q.y,q.z,-q.x,q.w);
}
