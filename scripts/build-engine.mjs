import './verify-resources.mjs';
import { readFileSync, copyFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { verifySdlLinkage } from './verify-sdl-linkage.mjs';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const {version}=JSON.parse(readFileSync(resolve(root,'package.json'),'utf8'));
const sha=path=>createHash('sha256').update(readFileSync(path)).digest('hex');
const sdl=resolve(root,'src-tauri/resources/SDL3.dll');
const release=process.argv.includes('--release');
const graph=spawnSync('cargo',['tree','--locked','-p','motion-engine','--edges','normal','--prefix','none'],{cwd:root,encoding:'utf8',windowsHide:true});
if(graph.status!==0)throw new Error(graph.stderr||'Unable to inspect engine dependencies');
if(/^(egui|eframe|tauri|sdl2|sdl3-src|joyshockmapper)\s/im.test(graph.stdout))throw new Error('Headless engine must not contain GUI, a second SDL runtime or JSM dependencies');
const features=spawnSync('cargo',['tree','--locked','-p','motion-engine','--edges','features'],{cwd:root,encoding:'utf8',windowsHide:true});
if(features.status!==0)throw new Error(features.stderr||'Unable to inspect SDL features');
if(/sdl3(?:-sys)? feature "(?:build-from-source[^"\n]*|static-link|link-static)"/i.test(features.stdout))throw new Error('FlexInput must use the verified shared SDL DLL, without a second/static SDL build');
// Apply the CRT choice to every target dependency (including hidapi's C build),
// while keeping build scripts/proc macros on the host and the Tauri cache apart.
const buildEnv={...process.env};
const staticTarget=resolve(root,'target/engine-static');
if(release){
  if(buildEnv.CARGO_ENCODED_RUSTFLAGS!==undefined)buildEnv.CARGO_ENCODED_RUSTFLAGS+='\x1f-C\x1ftarget-feature=+crt-static';
  else buildEnv.RUSTFLAGS=((buildEnv.RUSTFLAGS||'')+' -C target-feature=+crt-static').trim();
}
const args=release?['build','--locked','-p','motion-engine','--release','--target','x86_64-pc-windows-msvc','--target-dir',staticTarget]:['build','--locked','-p','motion-engine'];
const result=spawnSync('cargo',args,{cwd:root,env:buildEnv,stdio:'inherit',windowsHide:true});
if(result.error)throw result.error;
if(result.status!==0)process.exit(result.status||1);
const exe=resolve(root,'src-tauri/resources/motion-engine.exe');
const builtEngine=release?resolve(staticTarget,'x86_64-pc-windows-msvc/release/motion-engine.exe'):resolve(root,'target/debug/motion-engine.exe');
const sdlLinkage=verifySdlLinkage(builtEngine);
copyFileSync(builtEngine,exe);
if (release) {
  const releaseEngine = resolve(root, 'target/release/motion-engine.exe');
  mkdirSync(dirname(releaseEngine), { recursive: true });
  copyFileSync(builtEngine, releaseEngine);
}
writeFileSync(resolve(root,'src-tauri/resources/runtime-manifest.json'),JSON.stringify({
  version,engine:'motion-engine',flexinputCommit:'c69f53ad3bca4ed34ef5f2d63509addaa7c767be',
  inputProvider:{crate:'flexinput-devices',backend:'SDL3',timestampedEvents:true,touchEvents:true,sdlRuntime:'shared verified DLL',...sdlLinkage},
  physicalProcessing:{crate:'flexinput-engine',entry:'eval_graph_tick / device_cal',sharedStickInput:true},
  jsmImport:{format:'Studio profile',triggerParser:'FlexInput JSM Config'},
  triggerStages:{entry:'jsm/analog.rs',calibratedTravel:true},
  gamepadOutputs:{crate:'flexinput-virtual',devices:['VirtualXInput','VirtualDS4','HidMaestroDevice'],defaultBackend:'vigem',checkedReadiness:true},
  peripheralRouting:{feedback:'native poll_outputs -> selected SDL DeviceBackend::send',touchpad:'native eval_graph_tick / route_touchpad'},
  hidmaestro:{sdkVersion:'2.0.1',driverVersion:'2.0.0.2201',client:'FlexInput c69f53ad',payloads:JSON.parse(readFileSync(resolve(root,'vendor/flexinput/manifest.json'),'utf8')).files.filter(f=>/^crates\/hidmaestro\/driver\/.*\.(dll|inf)$/.test(f.path)).map(f=>({name:f.path.split('/').at(-1),sha256:f.vendoredSha256}))},
  sdlVersion:'3.4.18',sdlPatch:'Flydigi device 150 gyro axes only: (-oldZ, -oldX, -oldY)',
  files:[{name:'motion-engine.exe',sha256:sha(exe)},{name:'SDL3.dll',sha256:sha(sdl)}],
},null,2)+'\n');
