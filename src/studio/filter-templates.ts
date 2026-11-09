import passthrough from '../../examples/filters/passthrough.rhai?raw';
import ema from '../../examples/filters/ema.rhai?raw';
import median from '../../examples/filters/median.rhai?raw';

export const filterTemplates=[
  {id:'passthrough',name:'透传模板',source:passthrough,params:{} as Record<string,number>},
  {id:'ema',name:'时间平滑示例',source:ema,params:{tau_ms:12} as Record<string,number>},
  {id:'median',name:'中值滤波示例',source:median,params:{window:3} as Record<string,number>},
];
