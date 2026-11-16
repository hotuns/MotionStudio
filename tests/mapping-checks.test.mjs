import test from 'node:test';
import assert from 'node:assert/strict';
import {checkMapping} from '../scripts/check-mapping.mjs';

test('mapping release gate stops immediately on a failed core check',()=>{
  const calls=[];
  assert.throws(()=>checkMapping((command,args)=>{calls.push({command,args});return {status:1};}),/publication is blocked/);
  assert.equal(calls.length,1);
  assert.ok(calls[0].args.includes('bindings'));
});
test('mapping release gate also requires the synthetic host lifecycle checks',()=>{
  const calls=[];
  assert.throws(()=>checkMapping((command,args)=>{calls.push(args);return {status:calls.length===1?0:1};}),/publication is blocked/);
  assert.ok(calls[1].includes('runtime::tests'));
  assert.doesNotThrow(()=>checkMapping(()=>({status:0})));
});
test('mapping UI check failure also blocks publication',()=>{
  let calls=0;
  assert.throws(()=>checkMapping(()=>({status:++calls===3?1:0})),/UI regression checks failed/);
  assert.equal(calls,3);
});
