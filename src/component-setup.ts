import type {ComponentStatus, GamepadTarget, Settings} from './types';

export type InstallableComponent = 'hidmaestro' | 'hidhide' | 'vigem';
export type ComponentKind = InstallableComponent | 'usbip';
export const componentNames = {hidmaestro:'HIDMaestro', hidhide:'HidHide', vigem:'ViGEmBus', usbip:'usbip-win2'};
export const componentStateNames:Record<string,string> = {
  running:'已就绪', missing:'未安装', stopped:'已安装 · 待启动', unknown:'状态未知',
  unsupported:'不支持', partial:'安装不完整', incompatible:'版本不匹配', unavailable:'未检测到可用主控制器', configuration:'需要调整设置',
  busy:'暂时被占用', pending:'正在准备自动隐藏', permission_denied:'访问受限', control_unavailable:'驱动接口不可用',
};
export type RequiredComponent = {kind:ComponentKind; purpose:string; status:string; error?:string|null};
export type ComponentPlan = ReturnType<typeof componentPlan>;

// Match the existing host runtime::probe_output selection. This is a setup
// explanation for a draft; native readiness checks remain authoritative.
export function componentPlan(settings:Pick<Settings,'output_backend'|'gamepad_backend'|'auto_hide'>,
  target:GamepadTarget='none', components:ComponentStatus|null=null) {
  const items:RequiredComponent[]=[];
  const add=(kind:ComponentKind,purpose:string)=>{
    const state=components?.[kind];
    const item:RequiredComponent={kind,purpose,status:state?.status??'unknown',error:state?.error};
    // Native health already includes driver access/inverse mode or the active
    // lease. Optional CLI tools and pre-existing allow/cloak flags are irrelevant.
    items.push(item);
  };
  if((settings.output_backend??'hid_maestro')==='hid_maestro')add('hidmaestro','发送键盘、鼠标和所选 HID 虚拟手柄输入。');
  if(settings.auto_hide!==false)add('hidhide','安装后由软件自动管理：开始映射时隐藏实体手柄，停止后恢复，无需手动配置。');
  if(target==='dualsense')add('usbip','创建支持 USB 音频与高清触觉的虚拟 DualSense。');
  else if(target!=='none'&&(settings.output_backend==='legacy'||(settings.gamepad_backend??'vigem')==='vigem'))
    add('vigem','创建当前配置使用的虚拟 Xbox 360 或 DualShock 4。');
  const issues=items.filter(item=>item.status!=='running');
  // Unknown/stopped service states may be resolved by the native backend.
  // Do not turn a read-only status lookup failure into a new output blocker.
  const blocking=issues.filter(item=>['missing','partial','incompatible','unavailable','unsupported','configuration','permission_denied','control_unavailable'].includes(item.status));
  const installable=items.filter(item=>item.kind!=='usbip'&&item.status==='missing').map(item=>item.kind as InstallableComponent);
  return {items,issues,blocking,installable,checked:components!==null,ready:issues.length===0};
}
