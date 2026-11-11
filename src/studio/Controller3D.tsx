import { useEffect, useRef, useState } from 'react';
import {
  Box3, Color, DirectionalLight, Group, HemisphereLight, Mesh, MeshStandardMaterial,
  PerspectiveCamera, Quaternion, Scene, Vector3, WebGLRenderer,
  type BufferGeometry, type Material, type Object3D,
} from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { buttonNames, type Device, type Status } from '../types';
import styles from './DevicePage.module.css';

const models = {
  xboxOne: new URL('./controller-models/xbox-one.glb', import.meta.url).href,
  xbox360: new URL('./controller-models/xbox-360.glb', import.meta.url).href,
  dualSense: new URL('./controller-models/dualsense.glb', import.meta.url).href,
  dualShock4: new URL('./controller-models/dualshock-4.glb', import.meta.url).href,
  switchPro: new URL('./controller-models/switch-pro.glb', import.meta.url).href,
};

function modelFor(device: Device) {
  const { vendor, product, name } = device;
  if ((vendor === 0x054c && [0x0ce6, 0x0df2].includes(product)) || /dualsense/i.test(name)) return models.dualSense;
  if ((vendor === 0x054c && [0x05c4, 0x09cc, 0x0ba0].includes(product)) || /dualshock\s*4/i.test(name)) return models.dualShock4;
  if ((vendor === 0x057e && product === 0x2009) || /switch\s+pro/i.test(name)) return models.switchPro;
  if ((vendor === 0x045e && [0x028e, 0x0719].includes(product)) || /xbox\s*360/i.test(name)) return models.xbox360;
  return models.xboxOne;
}

// Port of the button mesh-to-pin names in FlexInput's canvas/viewer/controller3d.rs.
const partButtons: Partial<Record<string, typeof buttonNames[number]>> = {
  a_button: 'south', b_button: 'east', x_button: 'west', y_button: 'north',
  dpad_up: 'dpad_up', dpad_down: 'dpad_down', dpad_left: 'dpad_left', dpad_right: 'dpad_right',
  start_button: 'start', back_button: 'back', guide_button: 'guide', misc: 'misc1',
  left_bumper: 'left_shoulder', right_bumper: 'right_shoulder', touchpad: 'touchpad',
};
const xAxis = new Vector3(1, 0, 0);
const zAxis = new Vector3(0, 0, 1);

type Part = {
  node: Object3D;
  name: string;
  restPosition: Vector3;
  restRotation: Quaternion;
  height: number;
  materials: MeshStandardMaterial[];
};

function loadParts(model: Object3D) {
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

function updateParts(parts: Part[], accent: Color, status?: Status) {
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
      tiltX.setFromAxisAngle(xAxis, -y * 0.35);
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

function disposeModel(root: Object3D) {
  const geometries = new Set<BufferGeometry>();
  const materials = new Set<Material>();
  root.traverse(node => {
    if (!(node instanceof Mesh)) return;
    geometries.add(node.geometry);
    for (const material of Array.isArray(node.material) ? node.material : [node.material]) materials.add(material);
  });
  for (const geometry of geometries) geometry.dispose();
  for (const material of materials) material.dispose();
}

function radiusAround(model: Object3D, center: Vector3) {
  let radiusSquared = 0;
  const vertex = new Vector3();
  model.traverse(node => {
    if (!(node instanceof Mesh)) return;
    const positions = node.geometry.getAttribute('position');
    for (let i = 0; i < positions.count; i++) {
      vertex.fromBufferAttribute(positions, i).applyMatrix4(node.matrixWorld);
      radiusSquared = Math.max(radiusSquared, center.distanceToSquared(vertex));
    }
  });
  return Math.sqrt(radiusSquared);
}

export type ControllerPreviewFailure = 'graphics' | 'model';

export default function Controller3D({ device, status, pose, onUnavailable }: {
  device: Device;
  status?: Status;
  pose: Quaternion;
  onUnavailable: (reason: ControllerPreviewFailure) => void;
}) {
  const container = useRef<HTMLDivElement>(null);
  const latest = useRef({ status, pose });
  const update = useRef<() => void>();
  const [loading, setLoading] = useState(true);
  const url = modelFor(device);

  useEffect(() => {
    latest.current = { status, pose };
    update.current?.();
  }, [status, pose]);

  useEffect(() => {
    const mount = container.current;
    if (!mount) return;
    let renderer: WebGLRenderer;
    try {
      renderer = new WebGLRenderer({ antialias: true, alpha: true, powerPreference: 'low-power' });
    } catch {
      onUnavailable('graphics');
      return;
    }
    setLoading(true);
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 1.5));
    renderer.domElement.setAttribute('aria-hidden', 'true');
    mount.appendChild(renderer.domElement);

    const scene = new Scene();
    const assembly = new Group();
    const camera = new PerspectiveCamera(42, 1, 0.01, 100);
    const light = new DirectionalLight(0xffffff, 2.7);
    light.position.set(-3, 5, 4);
    scene.add(assembly, new HemisphereLight(0xffffff, 0x63708b, 2.1), light);
    const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
    const target = new Quaternion();
    let model: Object3D | undefined;
    let parts: Part[] = [];
    let radius = 1;
    let frame = 0;
    let lastFrame = 0;
    let disposed = false;

    const schedule = () => {
      if (model && !frame && !document.hidden && !disposed) frame = requestAnimationFrame(render);
    };
    function render(now: number) {
      frame = 0;
      const elapsed = Math.min(32, Math.max(8, now - lastFrame));
      const amount = reduceMotion.matches ? 1 : 1 - Math.exp(-elapsed / 35);
      lastFrame = now;
      assembly.quaternion.slerp(target, amount);
      const moving = assembly.quaternion.angleTo(target) > 0.001;
      if (!moving) assembly.quaternion.copy(target);
      renderer.render(scene, camera);
      if (moving) schedule();
    }
    const resize = () => {
      const { clientWidth: width, clientHeight: height } = mount;
      if (!width || !height) return;
      camera.aspect = width / height;
      const halfFov = camera.fov * Math.PI / 360;
      const limit = Math.min(halfFov, Math.atan(Math.tan(halfFov) * camera.aspect));
      const distance = radius / Math.sin(limit) * 1.09;
      camera.position.set(0, distance * Math.sin(1.03), distance * Math.cos(1.03));
      camera.lookAt(0, 0, 0);
      camera.updateProjectionMatrix();
      renderer.setSize(width, height, false);
      schedule();
    };
    const graphicsUnavailable = () => { if (!disposed) onUnavailable('graphics'); };
    const modelUnavailable = () => { if (!disposed) onUnavailable('model'); };
    const observer = new ResizeObserver(resize);
    observer.observe(mount);
    const accent = new Color();
    const syncTheme = () => {
      accent.set(getComputedStyle(document.documentElement).getPropertyValue('--accent').trim() || '#8bd12a');
      update.current?.();
    };
    syncTheme();
    const themeObserver = new MutationObserver(syncTheme);
    themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    renderer.domElement.addEventListener('webglcontextlost', graphicsUnavailable);
    document.addEventListener('visibilitychange', schedule);
    update.current = () => {
      target.copy(latest.current.pose);
      updateParts(parts, accent, latest.current.status);
      schedule();
    };
    void new GLTFLoader().loadAsync(url).then(({ scene: loaded }) => {
      if (disposed) { disposeModel(loaded); return; }
      model = loaded;
      parts = loadParts(model);
      const center = new Box3().setFromObject(model).getCenter(new Vector3());
      radius = radiusAround(model, center);
      if (!(radius > 0)) { modelUnavailable(); return; }
      model.position.sub(center);
      assembly.quaternion.copy(latest.current.pose);
      assembly.add(model);
      update.current?.();
      resize();
      setLoading(false);
    }).catch(modelUnavailable);

    return () => {
      disposed = true;
      update.current = undefined;
      cancelAnimationFrame(frame);
      observer.disconnect();
      themeObserver.disconnect();
      document.removeEventListener('visibilitychange', schedule);
      renderer.domElement.removeEventListener('webglcontextlost', graphicsUnavailable);
      if (model) disposeModel(model);
      renderer.dispose();
      renderer.forceContextLoss();
      renderer.domElement.remove();
    };
  }, [url, onUnavailable]);

  return <div ref={container} className={styles.modelViewport} role="img" aria-label="三维手柄预览">
    {loading && <span className={styles.modelLoading}>正在载入三维手柄…</span>}
  </div>;
}
