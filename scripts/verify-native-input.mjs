import {readFileSync,writeFileSync} from 'node:fs';
const numeric=(file,name)=>{
  const match=readFileSync(file,'utf8').match(new RegExp('pub const '+name+':[^=]+=[ \\t]*([0-9.]+)[ \\t]*;'));
  if(!match)throw Error('Native constant was not found: '+name);
  return Number(match[1]);
};
const automap='vendor/flexinput/crates/core/src/automap.rs';
const defaults={radial_buckets:numeric('vendor/flexinput/crates/engine/src/eval/device_cal.rs','STICK_RADIAL_BUCKETS'),trigger_digital_threshold:numeric(automap,'TRIGGER_DIGITAL_THRESHOLD'),trigger_hysteresis:numeric(automap,'TRIGGER_HYSTERESIS'),trigger_debounce_ms:numeric(automap,'TRIGGER_DEBOUNCE_MS')};
const path='src/native-input-defaults.json';
if(process.argv.includes('--write'))writeFileSync(path,JSON.stringify(defaults,null,2)+'\n');
else if(JSON.stringify(JSON.parse(readFileSync(path,'utf8')))!==JSON.stringify(defaults))throw Error('Native input defaults changed; regenerate the UI metadata from the pinned source.');
console.log('Native input defaults match the pinned FlexInput constants.');
