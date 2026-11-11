import { mkdirSync, readFileSync, cpSync, copyFileSync, writeFileSync, existsSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const build=resolve(root,process.env.CARGO_TARGET_DIR||'target','release');
const sha=file=>createHash('sha256').update(readFileSync(file)).digest('hex');
const directories=['resources','drivers','licenses'];
const documents=['LICENSE','README.md','THIRD_PARTY_NOTICES.md'];
export const portableEntries=['Motion Studio.exe',...directories,...documents,'docs','examples'];

export function stagePortable(version,output){
  const runtime=resolve(build,'resources');
  const manifest=JSON.parse(readFileSync(resolve(runtime,'runtime-manifest.json'),'utf8'));
  if(manifest.version!==version)throw new Error('Rebuild the runtime for '+version);
  for(const name of ['motion-engine.exe','SDL3.dll']){
    if(manifest.files.find(file=>file.name===name)?.sha256!==sha(resolve(runtime,name))){
      throw new Error('Packaged runtime checksum mismatch: '+name);
    }
  }
  mkdirSync(output,{recursive:true});
  copyFileSync(resolve(build,'motion-studio.exe'),resolve(output,'Motion Studio.exe'));
  for(const dir of directories)cpSync(resolve(build,dir),resolve(output,dir),{recursive:true});
  for(const file of documents)copyFileSync(resolve(root,file),resolve(output,file));
  for(const dir of ['docs','examples'])cpSync(resolve(root,dir),resolve(output,dir),{recursive:true});
}

if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
  const {version}=JSON.parse(readFileSync(resolve(root,'package.json'),'utf8'));
  if(!/^\d+\.\d+\.\d+$/.test(version))throw new Error('Invalid package version');
  const portable='MotionStudio-'+version+'-windows-x64';
  const output=resolve(root,'artifacts',portable);
  const installer=resolve(build,'bundle/nsis','Motion Studio_'+version+'_x64-setup.exe');
  if(!existsSync(installer))throw new Error('Run npm run build first');
  stagePortable(version,output);
  const setupName='MotionStudio-'+version+'-x64-setup.exe';
  copyFileSync(installer,resolve(root,'artifacts',setupName));
  const files=['Motion Studio.exe','resources/motion-engine.exe','resources/SDL3.dll'].map(name=>({
    file:portable+'/'+name,sha256:sha(resolve(output,name)),
  }));
  writeFileSync(resolve(root,'artifacts','checksums.json'),JSON.stringify({
    version,files:[{file:setupName,sha256:sha(installer)},...files],
  },null,2)+'\n');
  console.log('Portable folder: '+output);
}
