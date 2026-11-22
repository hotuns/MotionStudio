import { useEffect, useRef, useState } from 'react';
import {
  Box3, Color, DirectionalLight, Group, HemisphereLight, Mesh,
  PerspectiveCamera, Quaternion, Scene, Vector3, WebGLRenderer,
  type BufferGeometry, type Material, type Object3D,
} from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import type {OverlayDevice, OverlayStatus} from '../controller-overlay';
import {loadParts,updateParts,type Part} from './controller-model-animation';
import styles from './DevicePage.module.css';

const models = {
  xboxOne: new URL('./controller-models/xbox-one.glb', import.meta.url).href,
  xbox360: new URL('./controller-models/xbox-360.glb', import.meta.url).href,
  dualSense: new URL('./controller-models/dualsense.glb', import.meta.url).href,
  dualShock4: new URL('./controller-models/dualshock-4.glb', import.meta.url).href,
  switchPro: new URL('./controller-models/switch-pro.glb', import.meta.url).href,
};

function modelFor(device: OverlayDevice) {
  const { vendor, product, name } = device;
  if ((vendor === 0x054c && [0x0ce6, 0x0df2].includes(product)) || /dualsense/i.test(name)) return models.dualSense;
  if ((vendor === 0x054c && [0x05c4, 0x09cc, 0x0ba0].includes(product)) || /dualshock\s*4/i.test(name)) return models.dualShock4;
  if ((vendor === 0x057e && product === 0x2009) || /switch\s+pro/i.test(name)) return models.switchPro;
  if ((vendor === 0x045e && [0x028e, 0x0719].includes(product)) || /xbox\s*360/i.test(name)) return models.xbox360;
  return models.xboxOne;
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

export default function Controller3D({ device, status, pose, onUnavailable, className }: {
  device: OverlayDevice;
  status?: OverlayStatus;
  pose: Quaternion;
  onUnavailable: (reason: ControllerPreviewFailure) => void;
  className?: string;
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

  return <div ref={container} className={[styles.modelViewport,className].filter(Boolean).join(' ')} role="img" aria-label="三维手柄预览">
    {loading && <span className={styles.modelLoading}>正在载入三维手柄…</span>}
  </div>;
}
