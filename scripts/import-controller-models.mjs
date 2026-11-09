import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Color, Group, Mesh, MeshStandardMaterial, SRGBColorSpace, Vector3 } from 'three';
import { OBJLoader } from 'three/addons/loaders/OBJLoader.js';
import { GLTFExporter } from 'three/addons/exporters/GLTFExporter.js';
import { mergeVertices } from 'three/addons/utils/BufferGeometryUtils.js';

const source = process.argv[2];
if (!source) throw new Error('Pass the extracted, pinned FlexInput source directory');
const root = fileURLToPath(new URL('../', import.meta.url));
const pinned = JSON.parse(readFileSync(join(root, 'vendor/flexinput/manifest.json'), 'utf8'));
const output = join(root, 'src/studio/controller-models');
mkdirSync(output, { recursive: true });

// GLTFExporter uses FileReader for the in-memory binary. Supply the same
// asynchronous Blob API in Node so this import does not need a WebView.
globalThis.FileReader = class {
  readAsArrayBuffer(blob) {
    blob.arrayBuffer().then(result => {
      this.result = result;
      this.onloadend?.();
    });
  }
};

const models = {
  'Xbox One': 'xbox-one',
  'Xbox 360': 'xbox-360',
  DualSense: 'dualsense',
  'DualShock 4': 'dualshock-4',
  'Switch Pro': 'switch-pro',
};
const sha256 = data => createHash('sha256').update(data).digest('hex');
// Material groups for the shipped models, from FlexInput model/material.rs.
function paletteKey(part) {
  if (part === 'bottom_shell') return 'shell_secondary';
  if (part === 'extra') return 'led';
  if (part === 'guide_button' || part === 'misc') return 'logo';
  if (part === 'start_button' || part === 'back_button') return 'menu';
  if (part.startsWith('touch')) return 'touchpad';
  if (part.startsWith('dpad')) return 'dpad';
  if (part.endsWith('_bumper')) return 'bumper';
  if (part.endsWith('_trigger')) return 'trigger';
  if (part.endsWith('_stick')) return part.replace('_stick', '_dome');
  if (part.endsWith('_ring')) return part.replace('_ring', '_rim');
  if (part.endsWith('_cap')) return part;
  if (/^[abxy]_button$/.test(part)) return 'face_' + part[0];
  return 'shell';
}
const entries = [];
for (const [name, slug] of Object.entries(models)) {
  const directory = 'app/assets/models/' + name;
  const inputHashes = {};
  const read = filename => {
    const data = readFileSync(resolve(source, directory, filename));
    inputHashes[filename] = sha256(data);
    return data.toString('utf8');
  };
  const blocks = new Map();
  let values;
  for (const line of read('info.txt').split(/\r?\n/).map(value => value.trim()).filter(Boolean)) {
    if (line.endsWith('.obj') && /^[a-z0-9_]+\.obj$/i.test(line)) {
      values = [];
      blocks.set(line, values);
    } else if (values && Number.isFinite(Number(line))) {
      values.push(Number(line));
    } else {
      throw new Error('Invalid part metadata in ' + name + ': ' + line);
    }
  }

  const palette = JSON.parse(read('colors.fxcol'));
  const materials = new Map();
  const controller = new Group();
  controller.name = slug;
  for (const [filename, transform] of blocks) {
    if (transform.length < 11) throw new Error('Incomplete transform: ' + filename);
    // The upstream loader skips optional meshes named in info.txt but absent.
    if (!existsSync(resolve(source, directory, filename))) continue;
    const part = new OBJLoader().parse(read(filename));
    part.name = filename.slice(0, -4);
    part.userData.inputPart = part.name;
    part.position.fromArray(transform);
    // Same placement convention as FlexInput model/obj.rs.
    const axis = new Vector3().fromArray(transform, 3);
    if (axis.lengthSq() < 1e-8) axis.set(1, 0, 0);
    part.quaternion.setFromAxisAngle(axis.normalize(), transform[10]);
    if (part.name.startsWith('touch_point')) part.userData.touchSize = transform.slice(14, 16);
    const key = paletteKey(part.name);
    if (!materials.has(key)) {
      const [r, g, b] = palette[key] ?? palette.shell;
      const color = new Color().setRGB(r / 255, g / 255, b / 255, SRGBColorSpace);
      materials.set(key, new MeshStandardMaterial({ color, roughness: 0.78 }));
    }
    part.traverse(child => {
      if (child instanceof Mesh) {
        child.geometry = mergeVertices(child.geometry, 1e-6);
        child.material = materials.get(key);
      }
    });
    controller.add(part);
  }
  const names = new Set(controller.children.map(part => part.name));
  for (const required of ['top_shell','bottom_shell','a_button','b_button','x_button','y_button',
    'left_stick','right_stick','left_trigger','right_trigger']) {
    if (!names.has(required)) throw new Error('Missing ' + required + ' in ' + name);
  }
  const filename = slug + '.glb';
  const data = Buffer.from(await new GLTFExporter().parseAsync(controller, { binary: true }));
  writeFileSync(join(output, filename), data);
  entries.push({ directory, filename, sha256: sha256(data), parts: names.size, inputHashes });
}
writeFileSync(join(output, 'manifest.json'), JSON.stringify({
  repository: pinned.repository, commit: pinned.commit, models: entries,
}, null, 2) + '\n');
copyFileSync(resolve(source, 'licenses/3d-controller-overlay.txt'), join(root, 'licenses/3D-Controller-Overlay-MIT.txt'));
copyFileSync(join(root, 'node_modules/three/LICENSE'), join(root, 'licenses/Three-MIT.txt'));
console.log('Imported ' + entries.map(model => model.filename + ' (' + model.parts + ' parts)').join(', '));
