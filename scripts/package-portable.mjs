import { mkdirSync, readFileSync, cpSync, copyFileSync, writeFileSync, existsSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const version=JSON.parse(readFileSync(resolve(root,'package.json'),'utf8')).version;
if(!/^\d+\.\d+\.\d+$/.test(version))throw new Error('Invalid package version');
const output=resolve(root,'artifacts','MotionStudio-'+version+'-windows-x64');
const build=resolve(root,'target/release');
const installer=resolve(build,'bundle/nsis','Motion Studio_'+version+'_x64-setup.exe');
if(!existsSync(installer))throw new Error('Run npm run build first');
mkdirSync(output,{recursive:true});
copyFileSync(resolve(build,'motion-studio.exe'),resolve(output,'Motion Studio.exe'));
for(const dir of ['resources','drivers','licenses'])cpSync(resolve(build,dir),resolve(output,dir),{recursive:true});
for(const file of ['LICENSE','README.md','THIRD_PARTY_NOTICES.md'])copyFileSync(resolve(root,file),resolve(output,file));
cpSync(resolve(root,'docs'),resolve(output,'docs'),{recursive:true});
cpSync(resolve(root,'examples'),resolve(output,'examples'),{recursive:true});
copyFileSync(installer,resolve(root,'artifacts','MotionStudio-'+version+'-x64-setup.exe'));
const sha=file=>createHash('sha256').update(readFileSync(file)).digest('hex');
writeFileSync(resolve(root,'artifacts','checksums.json'),JSON.stringify({
  version,files:[
    {file:'MotionStudio-'+version+'-x64-setup.exe',sha256:sha(installer)},
    {file:'MotionStudio-'+version+'-windows-x64/Motion Studio.exe',sha256:sha(resolve(output,'Motion Studio.exe'))},
    {file:'MotionStudio-'+version+'-windows-x64/resources/motion-engine.exe',sha256:sha(resolve(output,'resources/motion-engine.exe'))},
    {file:'MotionStudio-'+version+'-windows-x64/resources/SDL3.dll',sha256:sha(resolve(output,'resources/SDL3.dll'))}
  ]
},null,2)+'\n');
console.log('Portable folder: '+output);
