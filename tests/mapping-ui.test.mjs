import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';

// Pure configuration transforms, with translation identity and no browser/IPC.
const source=readFileSync(new URL('../src/layers.ts',import.meta.url),'utf8').replace("import {t} from './i18n';",'const t=(value:string)=>value;');
const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {layerConflicts,updateLayer}=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));
const action=(name,input,trigger='down')=>({name,input,trigger,output:'key:SPACE',threshold:0.5});
const layer=(id,actions=[])=>({id,name:id,input:'left_shoulder',settings:{},actions,disabled_inputs:[]});

test('override preview includes every lower trigger and canonicalizes chord order',()=>{
  const p={actions:[action('Tap','south+left_shoulder','short'),action('Long','left_shoulder+south','long'),action('Other','east')],layers:[layer('edit',[action('New','left_shoulder+south')])]};
  assert.deepEqual(layerConflicts(p,'edit').map(c=>[c.action.name,c.kind]),[['Tap','replaces'],['Long','replaces']]);
});
test('override preview distinguishes disabled inheritance and higher-priority overrides',()=>{
  const low=layer('low',[action('Low','east')]),edit=layer('edit',[action('Edit','south')]),high=layer('high',[action('High','south')]);edit.disabled_inputs=['east'];
  const p={actions:[],layers:[low,edit,high]};
  assert.deepEqual(layerConflicts(p,'edit').map(c=>[c.source,c.action.name,c.kind]),[['low','Low','disabled'],['high','Edit','overridden']]);
  assert.deepEqual(layerConflicts(p,'missing'),[]);
});
test('layer editing preserves activation controls and isolates toggled action overrides',()=>{
  const edit={...layer('edit'),activation_mode:'toggle',exit_input:'east'};
  const p={actions:[action('Base','south')],layers:[edit],layer_reset_input:'start',gyro:{sensitivity:1},stick_deadzone:0.1};
  updateLayer(p,'edit',profile=>{profile.actions.push({...action('Latch','east'),toggle:true});profile.gyro.sensitivity=2;});
  assert.equal(p.actions.length,1);assert.equal(edit.actions[0].toggle,true);assert.equal(edit.activation_mode,'toggle');assert.equal(edit.exit_input,'east');assert.equal(p.layer_reset_input,'start');assert.deepEqual(edit.settings,{gyro:{sensitivity:2}});
});
