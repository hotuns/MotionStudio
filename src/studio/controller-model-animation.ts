import {Box3,Color,Mesh,MeshStandardMaterial,Quaternion,Vector3,type Material,type Object3D} from 'three';
import {buttonNames} from '../types';
import type {OverlayStatus} from '../controller-overlay';

// Port of the button mesh-to-pin names in FlexInput's canvas/viewer/controller3d.rs.
const partButtons: Partial<Record<string, typeof buttonNames[number]>> = {
  a_button: 'south', b_button: 'east', x_button: 'west', y_button: 'north',
  dpad_up: 'dpad_up', dpad_down: 'dpad_down', dpad_left: 'dpad_left', dpad_right: 'dpad_right',
  start_button: 'start', back_button: 'back', guide_button: 'guide', misc: 'misc1',
  left_bumper: 'left_shoulder', right_bumper: 'right_shoulder', touchpad: 'touchpad',
};
const xAxis = new Vector3(1, 0, 0);
const zAxis = new Vector3(0, 0, 1);

export type Part = {
  node: Object3D;
  name: string;
  restPosition: Vector3;
  restRotation: Quaternion;
  height: number;
  materials: MeshStandardMaterial[];
};

export function loadParts(model: Object3D) {
  const parts: Part[] = [];
  const replaced = new Set<Material>();
  model.traverse(node => {
    const name: unknown = node.userData.inputPart;
    if (typeof name !== 'string') return;
    const materials: MeshStandardMaterial[] = [];
    node.traverse(child => {
      if (!(child instanceof Mesh) || !(child.material instanceof MeshStandardMaterial)) return;
      replaced.add(child.material);
      const material = child.material.clone();
      child.material = material;
      materials.push(material);
    });
    const height = new Box3().setFromObject(node).getSize(new Vector3()).y;
    parts.push({ node, name, height, materials, restPosition: node.position.clone(), restRotation: node.quaternion.clone() });
  });
  for (const material of replaced) material.dispose();
  return parts;
}

export function updateParts(parts: Part[], accent: Color, status?: Pick<OverlayStatus,'axes'|'buttons'|'touches'>) {
  const [lx = 0, ly = 0, rx = 0, ry = 0, lt = 0, rt = 0] = status?.axes ?? [];
  const pressed = new Set(buttonNames.filter((_, index) => status?.buttons[index]));
  const tiltX = new Quaternion();
  const tiltZ = new Quaternion();
  for (const { node, name, height, restPosition, restRotation, materials } of parts) {
    node.position.copy(restPosition);
    node.quaternion.copy(restRotation);
    const input = partButtons[name];
    let highlight = 0;
    // Travel and hinge directions follow ControllerLive::part_xform.
    if (/^(left|right)_(stick|ring|cap)$/.test(name)) {
      const left = name.startsWith('left');
      const [x, y] = left ? [lx, ly] : [rx, ry];
      const click = pressed.has(left ? 'left_stick_click' : 'right_stick_click');
      // Status exposes raw SDL axes: positive Y points down, unlike the
      // upward-positive FlexInput AutoMap vector used by ControllerLive.
      tiltX.setFromAxisAngle(xAxis, y * 0.35);
      tiltZ.setFromAxisAngle(zAxis, -x * 0.35);
      node.quaternion.premultiply(tiltX.multiply(tiltZ));
      if (click) node.position.y -= 0.035;
      highlight = name.endsWith('_cap') ? Number(click) : Math.min(1, Math.hypot(x, y));
    } else if (name === 'left_trigger' || name === 'right_trigger') {
      highlight = Math.max(0, Math.min(1, name === 'left_trigger' ? lt : rt));
      node.quaternion.premultiply(tiltX.setFromAxisAngle(xAxis, -0.4 * highlight));
    } else if (name === 'touch_point1' || name === 'touch_point2') {
      const finger = status?.touches?.[0]?.[name.endsWith('1') ? 0 : 1];
      node.visible = finger?.active === true;
      if (finger?.active) {
        const [width, depth] = node.userData.touchSize ?? [0, 0];
        node.position.x += (finger.x - 0.5) * width * 0.9;
        node.position.z += (finger.y - 0.5) * depth * 0.9;
        highlight = 1;
      }
    } else if (input && pressed.has(input)) {
      if (name.endsWith('_bumper')) node.position.z += 0.04;
      else node.position.y -= Math.max(0.002, Math.min(0.012, height * 0.25));
      highlight = 1;
    }
    for (const material of materials) {
      material.emissive.copy(accent);
      material.emissiveIntensity = highlight * 0.85;
    }
  }
}

