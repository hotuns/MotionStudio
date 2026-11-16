import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {build} from 'esbuild';
import {Box3,Color,PerspectiveCamera,Vector3} from 'three';
import {GLTFLoader} from 'three/addons/loaders/GLTFLoader.js';

// Exercise the production animation against the actual bundled geometry.
// Only CPU transforms are used; there is no window, device or system output.
const root=fileURLToPath(new URL('../',import.meta.url));
const compiled=await build({
  absWorkingDir:root,entryPoints:['src/studio/controller-model-animation.ts'],
  bundle:true,write:false,format:'esm',platform:'node',logLevel:'silent',
  plugins:[{name:'shared-packages',setup(builder){
    builder.onResolve({filter:/^(three|react(?:\/.*)?)$/},args=>({path:import.meta.resolve(args.path),external:true}));
  }}],
});
const {loadParts,updateParts}=await import('data:text/javascript;base64,'+Buffer.from(compiled.outputFiles[0].text).toString('base64'));
const accent=new Color('#8bd12a');

for(const model of ['xbox-one','xbox-360','dualsense','dualshock-4','switch-pro']) {
  test(model+': both sticks project in the physical SDL direction and release cleanly',async()=>{
    const bytes=await readFile(new URL('../src/studio/controller-models/'+model+'.glb',import.meta.url));
    const {scene}=await new GLTFLoader().parseAsync(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength),'');
    const parts=loadParts(scene);
    const camera=new PerspectiveCamera(42,4/3,0.01,100);
    camera.position.set(0,5*Math.sin(1.03),5*Math.cos(1.03));
    camera.lookAt(0,0,0);camera.updateMatrixWorld(true);
    const screen=node=>new Box3().setFromObject(node).getCenter(new Vector3()).project(camera);
    for(const [side,offset] of [['left',0],['right',2]]) {
      const cap=parts.find(part=>part.name===side+'_cap');
      assert.ok(cap,model+' must have '+side+' cap');
      updateParts(parts,accent);scene.updateMatrixWorld(true);
      const neutral=screen(cap.node);
      // SDL uses positive X to the right and positive Y down. NDC Y points up.
      for(const [x,y,dimension,sign] of [[1,0,'x',1],[-1,0,'x',-1],[0,-1,'y',1],[0,1,'y',-1]]) {
        const axes=Array(6).fill(0);axes[offset]=x;axes[offset+1]=y;
        updateParts(parts,accent,{axes,buttons:[]});scene.updateMatrixWorld(true);
        const delta=screen(cap.node)[dimension]-neutral[dimension];
        assert.ok(delta*sign>0.001,`${model} ${side} raw [${x},${y}] projects incorrectly: ${delta}`);
      }
    }
    const buttons=Array(26).fill(false);buttons[0]=buttons[7]=buttons[8]=true;
    updateParts(parts,accent,{axes:[1,-1,-1,1,0.75,0.5],buttons,touches:[[{active:true,x:0.8,y:0.3}]]});
    assert.ok(parts.find(part=>part.name==='a_button').materials.every(material=>material.emissiveIntensity>0));
    for(const name of ['left_cap','right_cap']) {
      const part=parts.find(part=>part.name===name);
      assert.ok(part.node.position.y<part.restPosition.y);
    }
    // Disconnect / stop uses an absent status and must remove every animation.
    updateParts(parts,accent);
    for(const part of parts) {
      assert.ok(part.node.position.distanceTo(part.restPosition)<1e-8,part.name+' did not release');
      assert.ok(part.node.quaternion.angleTo(part.restRotation)<1e-7,part.name+' remained tilted');
      assert.ok(part.materials.every(material=>material.emissiveIntensity===0),part.name+' remained pressed');
      if(part.name.startsWith('touch_point'))assert.equal(part.node.visible,false);
    }
  });
}
