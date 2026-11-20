import test from 'node:test';
import assert from 'node:assert/strict';
import {fileURLToPath} from 'node:url';
import {createElement} from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import {build} from 'esbuild';

const compiled=await build({
  absWorkingDir:fileURLToPath(new URL('../',import.meta.url)),
  entryPoints:['src/studio/KeyMouseOverlay.tsx'],bundle:true,write:false,format:'esm',platform:'node',
  outfile:'overlay-test.mjs',logLevel:'silent',
  plugins:[{name:'shared-react',setup(builder){
    builder.onResolve({filter:/^react(?:\/.*)?$/},args=>({path:import.meta.resolve(args.path),external:true}));
  }}],
});
const js=compiled.outputFiles.find(file=>file.path.endsWith('.mjs'));
const {KeyMouseOverlay}=await import('data:text/javascript;base64,'+Buffer.from(js.text).toString('base64'));
const button=(name,down,age_ms)=>({kind:'button',name,down,age_ms});
const status=(held,recent)=>({state:'running',emitting:true,output_preview:{key_mouse:{held,recent},mouse:[]}});
function draw(status,ageMs=0) {
  return renderToStaticMarkup(createElement(KeyMouseOverlay,{status,ageMs,width:320}));
}

test('a released chord survives between frames without combining separate taps',()=>{
  const edges=[
    button('key:Q',true,95),button('key:Q',false,80),
    button('key:CTRL',true,48),button('key:A',true,40),button('key:A',false,10),button('key:CTRL',false,6),
  ];
  const markup=draw(status([],edges));
  assert.match(markup,/data-released="true" aria-label="Ctrl \+ A"/);
  assert.doesNotMatch(markup,/<kbd>Q<\/kbd>/);
  assert.doesNotMatch(draw(status([],edges),701),/<kbd>/);
  const held=draw(status(['key:W'],[button('key:E',true,50),button('key:E',false,5)]));
  assert.match(held,/data-released="false" aria-label="W"/);
  assert.match(held,/data-released="true" aria-label="E"/);
});

test('five buttons, signed wheel directions and relative movement share the writer clock',()=>{
  const sample=status(['mouse:right','mouse:x1'],[
    button('mouse:x2',false,100),{kind:'wheel',delta:[-2,3],age_ms:50},
  ]);
  sample.output_preview.mouse=[{position:[0,0],age_ms:900},{position:[12,-5],age_ms:30}];
  const markup=draw(sample);
  assert.equal((markup.match(/data-state="held"/g)||[]).length,2);
  assert.equal((markup.match(/data-state="released"/g)||[]).length,1);
  assert.match(markup,/↑<small>3/);assert.match(markup,/←<small>2/);
  assert.match(markup,/<b>\+12<\/b>/);assert.match(markup,/<b>-5<\/b>/);
  assert.match(markup,/<polyline/);
  const expired=draw(sample,480);
  assert.doesNotMatch(expired,/data-state="released"|<polyline|↑<small>/);
  assert.equal((expired.match(/data-state="held"/g)||[]).length,2);
});

test('pause, observation, capture, navigation and a lost status clear delivered outputs',()=>{
  const sample=status(['key:A','mouse:left'],[]);
  for(const inactive of [undefined,{...sample,emitting:false,state:'observing'},{...sample,binding_capture:true},
    {...sample,ui_navigation:true},{...sample,output_preview:null}]){
    const markup=draw(inactive);
    assert.match(markup,/data-live="false"/);
    assert.doesNotMatch(markup,/<kbd>|data-state="held"|<polyline/);
  }
});
