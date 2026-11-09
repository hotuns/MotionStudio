import {readFileSync} from 'node:fs';

// Verify the Windows image without loading it or touching input devices.
export function verifySdlLinkage(path,{required=true}={}) {
  const data=readFileSync(path);
  const pe=data.readUInt32LE(0x3c), optional=pe+24;
  if(data.toString('ascii',pe,pe+4)!=='PE\0\0'||data.readUInt16LE(optional)!==0x20b)throw new Error('Expected Windows x64 PE: '+path);
  const count=data.readUInt16LE(pe+6), table=optional+data.readUInt16LE(pe+20);
  const sections=Array.from({length:count},(_,i)=>{
    const p=table+i*40;
    return {rva:data.readUInt32LE(p+12),size:Math.max(data.readUInt32LE(p+8),data.readUInt32LE(p+16)),offset:data.readUInt32LE(p+20)};
  });
  const offset=rva=>{
    const section=sections.find(s=>rva>=s.rva&&rva<s.rva+s.size);
    if(!section)throw new Error('Invalid import RVA');
    return section.offset+rva-section.rva;
  };
  const dllName=rva=>{const start=offset(rva),end=data.indexOf(0,start);if(end<start)throw new Error('Invalid import name');return data.toString('ascii',start,end).toLowerCase();};
  const imports=(directory,stride,nameField)=>{
    const rva=data.readUInt32LE(optional+112+directory*8);
    if(!rva)return [];
    const names=[];let p=offset(rva);
    for(let i=0;i<1024;i++,p+=stride){const name=data.readUInt32LE(p+nameField);if(!name)return names;names.push(dllName(name));}
    throw new Error('Unbounded PE import table');
  };
  const eager=imports(1,20,12), delayed=imports(13,32,4);
  if(eager.includes('sdl3.dll')||(required&&!delayed.includes('sdl3.dll')))throw new Error('SDL must be delay-loaded after selecting the absolute verified DLL');
  // The desktop shell can dead-strip its unused input code entirely. Only the
  // engine must retain the shared import; the shell must never eagerly load it.
  if(!delayed.includes('sdl3.dll'))return {sdl:null,linkage:'no SDL import',embeddedRuntime:false};
  return {sdl:'SDL3.dll',linkage:'delayed shared import',embeddedRuntime:false};
}
