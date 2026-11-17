import {spawnSync} from 'node:child_process';
import {fileURLToPath,pathToFileURL} from 'node:url';

// Pure engine/host regressions: no physical backend, driver or desktop input.
export function checkMapping(run=spawnSync) {
  const commands=[
    ['test','--locked','-p','motion-core','--test','bindings','--test','digital_activators','--test','action_modes_layers','--test','mapping_state','--test','engine'],
    ['test','--locked','-p','motion-engine','runtime::tests'],
  ];
  for(const args of commands) {
    const result=run('cargo',args,{cwd:fileURLToPath(new URL('../',import.meta.url)),stdio:'inherit',windowsHide:true});
    if(result.error||result.status!==0)throw new Error('Mapping regression checks failed; publication is blocked.');
  }
  const ui=run(process.execPath,['--test','tests/mapping-ui.test.mjs','tests/mapping-overview.test.mjs','tests/component-setup.test.mjs'],{cwd:fileURLToPath(new URL('../',import.meta.url)),stdio:'inherit',windowsHide:true});
  if(ui.error||ui.status!==0)throw new Error('Mapping UI regression checks failed; publication is blocked.');
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href) {
  try {checkMapping();}catch(error){console.error(error.message);process.exitCode=1;}
}
