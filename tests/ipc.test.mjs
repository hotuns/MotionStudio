import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createInterface } from 'node:readline';
import { EventEmitter } from 'node:events';

const root=fileURLToPath(new URL('../',import.meta.url));
const exe=resolve(root,process.env.MOTION_ENGINE_EXE||'target/debug/motion-engine.exe');
const dll=resolve(root,'src-tauri/resources/SDL3.dll');
const delay=ms=>new Promise(r=>setTimeout(r,ms));
// Keep the packaged SDL event ABI and in-process virtual device without
// probing physical controllers in the default test suite.
const syntheticEnv={
  ...Object.fromEntries(Object.entries(process.env).filter(([key])=>!key.toUpperCase().startsWith('SDL_') && key.toUpperCase()!=='MOTION_STUDIO_DATA_DIR')),
  SDL_JOYSTICK_HIDAPI:'0',
  SDL_JOYSTICK_RAWINPUT:'0',
  SDL_JOYSTICK_DIRECTINPUT:'0',
  SDL_JOYSTICK_WGI:'0',
  SDL_JOYSTICK_GAMEINPUT:'0',
  SDL_XINPUT_ENABLED:'0',
};
function host(args=['--sdl',dll],env=syntheticEnv) {
  const child=spawn(exe,args,{cwd:root,env,windowsHide:true,stdio:['pipe','pipe','pipe']});
  const events=[],bus=new EventEmitter();let stderr='',serial=0;
  child.stderr.on('data',chunk=>{stderr+=chunk});
  child.on('error',e=>bus.emit('failure',e));
  const closed=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>resolve(code))});
  createInterface({input:child.stdout}).on('line',line=>{
    try{const value=JSON.parse(line);events.push(value);bus.emit('event',value)}
    catch{events.push({type:'line',line});bus.emit('event',{type:'line',line})}
  });
  const wait=(predicate,from=0,timeout=10000)=>new Promise((resolve,reject)=>{
    const existing=events.slice(from).find(predicate);if(existing){resolve(existing);return}
    const timer=setTimeout(()=>{cleanup();reject(new Error('Timed out waiting for event. '+stderr+'\n'+JSON.stringify(events.slice(-3))))},timeout);
    const cleanup=()=>{clearTimeout(timer);bus.off('event',onEvent);bus.off('failure',onError)};
    const onError=e=>{cleanup();reject(e)};
    const onEvent=event=>{if(predicate(event)){cleanup();resolve(event)}};
    bus.on('event',onEvent);bus.on('failure',onError);
  });
  return {child,events,closed,wait,async request(type,args={}) {
    const id=++serial,from=events.length;
    child.stdin.write(JSON.stringify({id,type,...args})+'\n');
    return wait(e=>e.type==='reply'&&e.id===id,from);
  },async close(){
    if(child.exitCode===null)child.stdin.end();
    const code=await Promise.race([closed,delay(5000).then(()=>{if(child.exitCode===null){child.kill();throw new Error('Host did not shut down on EOF: '+stderr)}})]);
    return code;
  }};
}
const profile=name=>({schema_version:1,name,gyro:{enabled:false},actions:[]});

test('packaged SDL uses real sensor-event ABI without sending system input',()=>{
  const result=spawnSync(exe,['--sdl',dll,'--self-test'],{env:syntheticEnv,encoding:'utf8',windowsHide:true,timeout:15000});
  assert.equal(result.status,0,result.stderr);
  assert.deepEqual(JSON.parse(result.stdout),{ok:true,gyro_events:3,touch_events:3,system_output:false});
});

test('worker handles discovery, pinned reconnect, valid/invalid reload, switch and shutdown',async t=>{
  const directory=mkdtempSync(join(tmpdir(),'motion-ipc-'));
  const file=join(directory,'profile.toml');
  const write=(name,target)=>writeFileSync(file,'schema_version = 1\nname = "'+name+'"\nvirtual_gamepad = '+target+'\n');
  write('Initial','true');
  const app=host();t.after(async()=>{await app.close();rmSync(directory,{recursive:true,force:true})});
  assert.equal((await app.wait(e=>e.type==='hello')).protocol,15);
  assert.deepEqual((await app.wait(e=>e.type==='devices')).devices,[]);
  const config={profile_id:'integration',profile:{...profile('Initial'),virtual_gamepad:true},profile_file:file,device_path:'test://absent/physical/MI_02',emit:false};
  assert.equal((await app.request('start',{config})).ok,true);
  await app.wait(e=>e.type==='status'&&e.status.state==='waiting');
  const scan=await app.request('scan');assert.equal(scan.ok,false);
  let mark=app.events.length;write('Hot reload','"dualshock_4"');
  await app.wait(e=>e.type==='status'&&e.status.profile_name==='Hot reload',mark);
  mark=app.events.length;writeFileSync(file,'broken invalid TOML');
  const rejected=await app.wait(e=>e.type==='status'&&e.status.reload_error,mark);
  assert.equal(rejected.status.profile_name,'Hot reload');assert.equal(rejected.status.emitting,false);
  mark=app.events.length;write('Recovered file','"none"');
  await app.wait(e=>e.type==='status'&&e.status.profile_name==='Recovered file'&&!e.status.reload_error,mark);
  assert.equal((await app.request('apply',{profile_id:'second',profile:profile('Switched'),profile_file:join(directory,'second.toml')})).ok,true);
  await app.wait(e=>e.type==='status'&&e.status.profile_id==='second',mark);
  const triggerFile=join(directory,'triggers.toml');
  const triggers={
    ...profile('Studio trigger stages'),
    trigger_stages:{right_mode:'no_skip'},
    actions:[
      {name:'Aim',input:'right_trigger_soft',output:'mouse:right'},
      {name:'Fire',input:'right_trigger_full',output:'mouse:left'},
    ],
  };
  mark=app.events.length;
  assert.equal((await app.request('apply',{profile_id:'triggers',profile:triggers,profile_file:triggerFile})).ok,true);
  await app.wait(e=>e.type==='status'&&e.status.profile_id==='triggers'&&!e.status.emitting,mark);
  mark=app.events.length;
  writeFileSync(triggerFile,'schema_version = 1\nname = "Legacy source"\n[[jsm.files]]\nname = "Aim.txt"\ntext = "ZR = RMOUSE"\n');
  const legacySource=await app.wait(e=>e.type==='status'&&e.status.reload_error,mark);
  assert.equal(legacySource.status.profile_name,'Studio trigger stages');
  assert.match(legacySource.status.reload_error,/导入 JSM 配置/);
  mark=app.events.length;
  writeFileSync(triggerFile,'schema_version = 1\nname = "Trigger reload"\n[trigger_stages]\nright_mode = "no_skip_exclusive"\n[[actions]]\nname = "Fire"\ninput = "right_trigger_full"\noutput = "mouse:left"\n');
  await app.wait(e=>e.type==='status'&&e.status.profile_name==='Trigger reload'&&!e.status.reload_error,mark);
  assert.equal((await app.request('stop')).ok,true);
  assert.equal((await app.request('shutdown')).ok,true);
  assert.equal(await app.close(),0);
});

test('invalid IPC ends the worker cleanly instead of leaving an orphan',async()=>{
  const app=host();await app.wait(e=>e.type==='hello');
  app.child.stdin.write('not JSON\n');
  assert.equal(await app.close(),0);
});

test('advanced motion commands reject invalid or unavailable calibration and keep the worker alive',async t=>{
  const app=host();t.after(()=>app.close());await app.wait(e=>e.type==='hello');
  assert.equal((await app.request('begin_measure',{axis:'yaw'})).ok,false);
  assert.equal((await app.request('calibrate_face',{axis:3,positive:true})).ok,false);
  assert.equal((await app.request('calibrate_face',{axis:0,positive:true})).ok,false);
  assert.equal((await app.request('set_sensor_calibration',{calibration:{mounting:[2,0,0,0,1,0,0,0,1]}})).ok,false);
  assert.equal((await app.request('reset_pose')).ok,true);
  assert.equal((await app.request('scan')).ok,true);
  assert.equal((await app.request('shutdown')).ok,true);
  assert.equal(await app.close(),0);
});

test('independent output guardian exits on owner EOF and heartbeat timeout (empty lease)',async()=>{
  for(const heartbeatTimeout of [false,true]){
    const app=host(['--guardian']);
    await app.wait(e=>e.type==='line'&&e.line==='ready');
    app.child.stdin.write('{"held":[]}\n');
    if(!heartbeatTimeout)app.child.stdin.end();
    const result=await Promise.race([app.closed,delay(4000).then(()=>{if(app.child.exitCode===null){app.child.kill();throw new Error('Guardian timeout')}})]);
    assert.equal(result,0);
  }
});

test('UI navigation is a blank observer without requiring a profile and scan/stop clears it',async t=>{
  const app=host();t.after(async()=>app.close());await app.wait(e=>e.type==='hello');
  const mark=app.events.length;
  assert.equal((await app.request('navigate',{device_path:'test://absent/physical/MI_02'})).ok,true);
  const waiting=await app.wait(e=>e.type==='status'&&e.status.ui_navigation===true,mark);
  assert.equal(waiting.status.emitting,false);assert.equal(waiting.status.profile_id,null);
  assert.deepEqual(waiting.status.navigation_held,[]);
  assert.equal((await app.request('scan')).ok,true);
  const stopMark=app.events.length;
  assert.equal((await app.request('stop')).ok,true);
  await app.wait(e=>e.type==='status'&&!e.status.ui_navigation&&e.status.state==='idle',stopMark);
  assert.equal((await app.request('navigate',{device_path:''})).ok,false);
});

test('physical gyro observe-only: exact interface, live events and profile reload', {skip:process.env.MOTION_TEST_HARDWARE!=='1'}, async t=>{
  const directory=mkdtempSync(join(tmpdir(),'motion-hardware-'));
  const app=host(['--sdl',dll],process.env);
  t.after(async()=>{await app.close();rmSync(directory,{recursive:true,force:true})});
  await app.wait(e=>e.type==='hello');
  const isPhysicalGyro=d=>d.origin==='physical'&&d.gamepad&&d.gyro&&!d.open_error;
  const eligible=e=>e.type==='devices'&&e.devices.some(isPhysicalGyro);
  let discovered;
  for(let attempt=0;attempt<4&&!discovered;attempt++){
    const mark=attempt?app.events.length:0;
    if(attempt)await app.request('scan');
    try{discovered=await app.wait(eligible,mark,2000)}catch(error){if(attempt===3)throw error}
  }
  const candidates=discovered.devices.filter(isPhysicalGyro);
  assert.equal(candidates.length,1,'Choose a single gyro interface for this explicit hardware test');
  const device=candidates[0],file=join(directory,'observe.toml');
  const original={...profile('Hardware observation'),gyro:{enabled:true,activation:'always',sensitivity:1}};
  writeFileSync(file,'schema_version=1\nname="Hardware observation"\n[gyro]\nenabled=true\nactivation="always"\n');
  assert.equal((await app.request('start',{config:{profile_id:'hardware',profile:original,profile_file:file,device_path:device.path,emit:false}})).ok,true);
  const live=await app.wait(e=>e.type==='status'&&e.status.state==='observing'&&e.status.stats.gyro_events>200);
  assert.equal(live.status.device.path.toLowerCase(),device.path.toLowerCase());
  assert.equal(live.status.emitting,false);
  const from=app.events.length;
  writeFileSync(file,'schema_version=1\nname="Hardware reloaded"\n[gyro]\nenabled=true\nactivation="always"\nsensitivity=1.5\n');
  const reloaded=await app.wait(e=>e.type==='status'&&e.status.profile_name==='Hardware reloaded'&&e.status.stats.gyro_events>200,from);
  assert.ok(reloaded.status.revision>live.status.revision);
  assert.ok(app.events.filter(e=>e.type==='status').every(e=>!e.status.emitting));
  mkdirSync(resolve(root,'.test-data'),{recursive:true});
  writeFileSync(resolve(root,'.test-data/hardware-observe.json'),JSON.stringify({device,first:live.status,hotReload:reloaded.status,systemOutput:false},null,2));
  t.diagnostic('Received '+Math.round(live.status.stats.gyro_hz)+' gyro events/sec; process P99 '+live.status.stats.process_p99_us+' us; queue P99 '+live.status.stats.queue_p99_us+' us');
  assert.equal((await app.request('stop')).ok,true);
});
