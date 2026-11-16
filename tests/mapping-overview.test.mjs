import test from 'node:test';
import assert from 'node:assert/strict';
import {fileURLToPath} from 'node:url';
import {build} from 'esbuild';

const compiled=await build({absWorkingDir:fileURLToPath(new URL('../',import.meta.url)),
  entryPoints:['src/studio/controller-mapping-overview.ts'],bundle:true,write:false,format:'esm',platform:'node',logLevel:'silent',
  plugins:[{name:'translation-identity',setup(builder){
    builder.onResolve({filter:/\/i18n$/},()=>({path:'translation',namespace:'test'}));
    builder.onLoad({filter:/.*/,namespace:'test'},()=>({contents:'export const t=value=>value;',loader:'js'}));
  }}],
});
const {buildMappingOverview,entriesForGroup,overviewGroups}=await import('data:text/javascript;base64,'+Buffer.from(compiled.outputFiles[0].text).toString('base64'));
const action=(name,input,output='key:SPACE',trigger='down')=>({name,input,output,trigger,threshold:0.5});
const base=()=>({schema_version:1,name:'Test',virtual_gamepad:'none',gamepad_bindings:[],actions:[],layers:[],
  stick_mouse:{enabled:false,stick:'left'},gyro:{enabled:false,activation:'always',output:'mouse',motion:{flick:{enabled:false,stick:'right'},bindings:[],turns:[]}}});

test('overview keeps action names and source indexes while excluding overridden and disabled inputs',()=>{
  const p=base();p.actions=[action('Tap','south+left_shoulder','key:E','short'),action('Hold','left_shoulder+south','key:R','long'),action('Inherited name','west'),action('Disabled','east')];
  p.layers=[{id:'aim',name:'Aim',input:'left_trigger',settings:{},actions:[action('Layer action','south+left_shoulder','mouse:right')],disabled_inputs:['east']},
    {id:'other',name:'Other',input:'right_trigger',settings:{},actions:[action('Other layer','west','key:X')],disabled_inputs:[]}];
  const before=structuredClone(p),view=buildMappingOverview(p,'aim');
  assert.deepEqual(view.entries.map(e=>[e.value.name,e.layerId,e.index]),[['Inherited name','',2],['Layer action','aim',0]]);
  assert.deepEqual(p,before,'preview must not mutate the saved or draft configuration');
  assert.equal(buildMappingOverview(p,'missing').entries.length,4);
});
test('chords appear at each member control, while shared stick directions appear once in a group',()=>{
  const p=base();p.actions=[action('Combo','left_shoulder+south'),action('Diagonal','left_stick_up+left_stick_right'),action('Swipe','touch_swipe_left'),action('Unassigned','')];
  const {entries}=buildMappingOverview(p);
  const names=id=>entriesForGroup(entries,overviewGroups.find(g=>g.id===id)).map(e=>e.value.name);
  assert.deepEqual(names('left_top'),['Combo']);assert.deepEqual(names('face'),['Combo']);
  assert.deepEqual(names('left_stick'),['Diagonal']);assert.deepEqual(names('center'),['Swipe']);assert.deepEqual(names('motion'),['Unassigned']);
});
test('virtual layout inheritance is a whole-list override and explicit empty stays empty',()=>{
  const p=base();p.virtual_gamepad='dualsense';p.gamepad_bindings=[{input:'south',output:'east'}];
  p.layers=[{id:'aim',name:'Aim',input:'left_trigger',settings:{gamepad_bindings:[{input:'west',output:'south'}]},actions:[],disabled_inputs:[]}];
  let view=buildMappingOverview(p,'aim');assert.equal(view.entries.length,1);assert.equal(view.entries[0].value.input,'west');assert.equal(view.entries[0].layerId,'aim');
  p.layers[0].settings.gamepad_bindings=[];assert.deepEqual(buildMappingOverview(p,'aim').entries,[]);
  p.layers[0].settings.gamepad_bindings={clear:true};view=buildMappingOverview(p,'aim');
  assert.ok(view.entries.some(e=>e.value.output==='touchpad'));assert.ok(view.entries.some(e=>e.value.output==='right_stick'));
  p.virtual_gamepad='none';assert.deepEqual(buildMappingOverview(p,'aim').entries,[]);
});
test('motion and fixed turns retain their names and settings use the edited layer values',()=>{
  const p=base();p.gyro.motion.turns=[{name:'Turn around',input:'right_stick_click',output:'mouse',degrees:180,duration_ms:100}];
  p.gyro.motion.bindings=[{name:'Lean',input:'imu_tilt_left',output:'key:Q'}];
  p.layers=[{id:'aim',name:'Aim',input:'left_trigger',settings:{stick_mouse:{enabled:true,stick:'right'},gyro:{motion:{turns:[{name:'Layer turn',input:'south',output:'right_stick',degrees:90,duration_ms:100}]}}},actions:[],disabled_inputs:[]}];
  const view=buildMappingOverview(p,'aim');
  assert.equal(view.entries.find(e=>e.kind==='turn').value.name,'Layer turn');assert.equal(view.entries.find(e=>e.kind==='turn').layerId,'aim');
  assert.equal(view.entries.find(e=>e.kind==='motion').value.name,'Lean');assert.equal(view.entries.find(e=>e.kind==='motion').layerId,'');
  const stick=view.entries.find(e=>e.kind==='setting');assert.equal(stick.input,'right_stick');assert.equal(stick.layerId,'aim');
});
