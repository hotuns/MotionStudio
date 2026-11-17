import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';

const compiled=ts.transpileModule(readFileSync(new URL('../src/component-setup.ts',import.meta.url),'utf8'),
  {compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {componentPlan}=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));
const ready=()=>({hidmaestro:{status:'running'},hidhide:{status:'running'},vigem:{status:'running'},usbip:{status:'running'},cliAvailable:true,controlAvailable:true,inverse:false,engineAllowed:false,cloakEnabled:false});
const settings={output_backend:'hid_maestro',gamepad_backend:'vigem',auto_hide:true};
const kinds=(s,target)=>componentPlan(s,target,ready()).items.map(item=>item.kind);

test('setup follows the configured native output requirements instead of requesting every driver',()=>{
  assert.deepEqual(kinds(settings,'none'),['hidmaestro','hidhide']);
  assert.deepEqual(kinds(settings,'xbox_360'),['hidmaestro','hidhide','vigem']);
  assert.deepEqual(kinds({...settings,gamepad_backend:'hid_maestro'},'dualshock_4'),['hidmaestro','hidhide']);
  assert.deepEqual(kinds({...settings,output_backend:'legacy',gamepad_backend:'hid_maestro',auto_hide:false},'xbox_360'),['vigem']);
  assert.deepEqual(kinds({...settings,output_backend:'legacy',auto_hide:false},'none'),[]);
  assert.deepEqual(kinds(settings,'dualsense'),['hidmaestro','hidhide','usbip']);
});
test('only missing required components block startup; unknown and stopped statuses remain actionable but do not invent a failure',()=>{
  const c=ready();c.vigem.status='missing';
  assert.equal(componentPlan(settings,'none',c).ready,true);
  assert.deepEqual(componentPlan(settings,'xbox_360',c).blocking.map(item=>item.kind),['vigem']);
  for(const status of ['unknown','stopped']) {
    c.vigem.status=status;const plan=componentPlan(settings,'xbox_360',c);
    assert.equal(plan.ready,false);assert.equal(plan.blocking.length,0);
  }
  const unknown=componentPlan(settings,'none',null);
  assert.equal(unknown.checked,false);assert.equal(unknown.ready,false);assert.equal(unknown.blocking.length,0);
});
test('native HidHide readiness does not depend on CLI tools or pre-existing allow and cloak rules',()=>{
  const c=ready();assert.equal(componentPlan(settings,'none',c).ready,true);
  c.cliAvailable=false;assert.equal(componentPlan(settings,'none',c).ready,true);
  c.controlAvailable=false;c.hidhideSession=true;assert.equal(componentPlan(settings,'none',c).ready,true);
  c.hidhide.status='configuration';assert.equal(componentPlan(settings,'none',c).blocking[0].kind,'hidhide');
  assert.equal(componentPlan({...settings,auto_hide:false},'none',c).ready,true);
});
test('one-click installation includes only required and definitely missing bundled components',()=>{
  const c=ready();c.hidhide.status='missing';c.vigem.status='missing';c.usbip.status='unavailable';
  assert.deepEqual(componentPlan(settings,'none',c).installable,['hidhide']);
  assert.deepEqual(componentPlan(settings,'xbox_360',c).installable,['hidhide','vigem']);
  assert.deepEqual(componentPlan(settings,'dualsense',c).installable,['hidhide']);
  for(const status of ['busy','pending','unknown','stopped','control_unavailable','permission_denied']) {
    c.hidhide.status=status;
    assert.deepEqual(componentPlan(settings,'none',c).installable,[]);
    assert.equal(componentPlan(settings,'none',c).ready,false);
  }
});
test('damaged HIDMaestro and an unavailable USB/IP controller are reported without treating them as a clean install',()=>{
  const c=ready();c.hidmaestro.status='partial';c.usbip.status='unavailable';
  const plan=componentPlan(settings,'dualsense',c);
  assert.deepEqual(plan.blocking.map(item=>[item.kind,item.status]),[['hidmaestro','partial'],['usbip','unavailable']]);
  assert.equal(plan.items.some(item=>item.kind==='vigem'),false);
});
