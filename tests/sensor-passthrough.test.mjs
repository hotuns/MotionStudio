import test from 'node:test';
import assert from 'node:assert/strict';
import {fileURLToPath} from 'node:url';
import {build} from 'esbuild';
import {createElement} from 'react';
import {renderToStaticMarkup} from 'react-dom/server';

const root=fileURLToPath(new URL('../',import.meta.url));
const bundle=await build({absWorkingDir:root,stdin:{resolveDir:root,contents:
  "export * from './src/sensor-passthrough'; export {MotionOutputSettings} from './src/studio/AdvancedMotion';"},
  bundle:true,write:false,format:'esm',platform:'node',outfile:'sensor-ui-test.mjs',logLevel:'silent',
  plugins:[{name:'shared-react',setup(builder){
    builder.onResolve({filter:/^react(?:-dom)?(?:\/.*)?$/},args=>({path:import.meta.resolve(args.path),external:true}));
  }}]});
const code=bundle.outputFiles.find(file=>file.path.endsWith('.mjs')).text;
const {MotionOutputSettings,profileUsesSensorOutput,setGamepadTarget}=await import('data:text/javascript;base64,'+Buffer.from(code+'\n//# sourceURL=sensor-ui-test.mjs').toString('base64'));

function profile(target,output) {
  return {virtual_gamepad:target,gyro:{output,motion:{mode:'pointer',turns:[],flick:{}}},layers:[]};
}

test('the DualSense selector offers USB/IP sensor output independently of the ViGEm preference',()=>{
  const render=(p,vigemGamepad)=>renderToStaticMarkup(createElement(MotionOutputSettings,{profile:p,vigemGamepad}));
  const ps5=render(profile('dualsense','dualsense_imu'),false);
  assert.match(ps5,/<option value="dualsense_imu" selected="">DualSense 传感器直通<\/option>/);
  assert.match(ps5,/USB\/IP/);
  assert.doesNotMatch(ps5,/value="ds4_imu"/);
  assert.match(render(profile('dualshock_4','ds4_imu'),true),/<option value="ds4_imu" selected="">/);
  assert.match(render(profile('dualshock_4','ds4_imu'),false),/<option value="ds4_imu" disabled=""/);
});

test('switching Sony targets preserves explicit and inherited layer outputs and filter settings',()=>{
  const p=profile('dualshock_4','ds4_imu');
  p.gamepad_bindings=[{input:'south',output:'south'}];
  p.gyro.ds4_imu_filter={smoothing_ms:9};
  p.layers=[{settings:{gyro:{output:'ds4_imu',activation:'left_shoulder'}}},
    {settings:{gyro:{sensitivity:2}}},{settings:{gyro:{output:'mouse'}}}];
  const old=structuredClone(p);
  setGamepadTarget(p,'dualsense');
  assert.equal(p.gyro.output,'dualsense_imu');
  assert.equal(p.layers[0].settings.gyro.output,'dualsense_imu');
  assert.deepEqual(p.layers.slice(1),old.layers.slice(1));
  p.gyro.output='mouse';
  assert.equal(profileUsesSensorOutput(p),true);
  p.gyro.output='dualsense_imu';
  setGamepadTarget(p,'dualshock_4');
  assert.deepEqual(p,old);
});
