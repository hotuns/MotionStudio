import { readFileSync, statSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const sha=path=>createHash('sha256').update(readFileSync(path)).digest('hex');
const vendorRoot=resolve(root,'vendor/flexinput');
const manifest=JSON.parse(readFileSync(resolve(vendorRoot,'manifest.json'),'utf8'));
const modelRoot=resolve(root,'src/studio/controller-models');
const modelManifest=JSON.parse(readFileSync(resolve(modelRoot,'manifest.json'),'utf8'));
const { app: { security: { csp } } }=JSON.parse(readFileSync(resolve(root,'src-tauri/tauri.conf.json'),'utf8'));
// Three.js loads the embedded GLBs with fetch, which uses connect-src in the installed app.
const modelSources=csp.split(';').map(directive=>directive.trim().split(/\s+/))
  .find(([directive])=>directive==='connect-src');
if(!modelSources?.includes("'self'"))throw new Error('The installed app must allow same-origin controller model requests in connect-src');
if(modelManifest.commit!==manifest.commit||modelManifest.repository!==manifest.repository)throw new Error('Controller model source does not match the pinned FlexInput revision');
for(const model of modelManifest.models){
  if(!/^[a-z0-9-]+\.glb$/.test(model.filename)||sha(resolve(modelRoot,model.filename))!==model.sha256)throw new Error('Controller model integrity check failed: '+model.filename);
}
for(const entry of manifest.files){
  const file=resolve(vendorRoot,entry.path);
  if(!file.startsWith(vendorRoot+'/')&&!file.startsWith(vendorRoot+'\\'))throw new Error('Invalid vendor path');
  if(sha(file)!==entry.vendoredSha256)throw new Error('Vendored source integrity check failed: '+entry.path);
}
const sdlManifest=JSON.parse(readFileSync(resolve(root,'vendor/sdl3/manifest.json'),'utf8'));
if(sdlManifest.modified!==false)throw new Error('SDL must be an unmodified official release');
for(const path of ['src-tauri/resources/SDL3.dll','vendor/sdl3/SDL3.lib']){
  const entry=sdlManifest.files.find(file=>file.path===path);
  const file=resolve(root,path);
  if(!entry||statSync(file).size!==entry.size||sha(file)!==entry.sha256)throw new Error('Official SDL integrity check failed: '+path);
}
const drivers=JSON.parse(readFileSync(resolve(root,'src-tauri/drivers/manifest.json'),'utf8'));
for(const entry of Object.values(drivers.installers)){
  const file=resolve(root,'src-tauri/drivers',entry.file);
  if(statSync(file).size!==entry.size||sha(file)!==entry.sha256)throw new Error('Driver installer integrity check failed: '+entry.file);
}
if(sha(resolve(root,'licenses/HIDMaestro-MIT.txt'))!==sha(resolve(vendorRoot,'crates/hidmaestro/driver/LICENSE')))throw new Error('HIDMaestro license is missing or changed');
console.log('Verified '+manifest.files.length+' vendored files including HIDMaestro payloads, official SDL and 2 official installers.');
