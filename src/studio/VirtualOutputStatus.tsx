import type {Status} from '../types';
import {Dialog,gamepadNames} from './Controls';

export function VirtualOutputStatus({status,onClose,onOpenLegacy}:{status?:Status;onClose:()=>void;onOpenLegacy:()=>void}) {
  const pad=status?.output_gamepad;
  return <Dialog title="虚拟手柄运行状态" onClose={onClose} actions={<button type="button" className="primary-btn" onClick={onClose}>完成</button>}>
    {pad?<>
      <p role="status">{pad.connected?'虚拟手柄已建立':'虚拟手柄连接异常'} · <span translate="no">{gamepadNames[pad.target]}</span></p>
      <p>运行后端：<span translate="no">{pad.backend==='vigem'?'ViGEm / FlexInput':'HIDMaestro / FlexInput'}</span></p>
      {pad.player_index!==null?<p>XInput 玩家槽位：<span translate="no">{pad.player_index+1} / 4</span></p>:pad.target==='xbox_360'?<p>本设备的 XInput 接口已通过就绪检查；HIDMaestro 未提供玩家槽位查询。</p>:null}
      <p>正在运行的配置：<strong translate="no">{status?.profile_name}</strong></p>
      {status?.binding_capture&&<p>正在录入，输出暂时释放；虚拟设备和隐藏会话保持连接。</p>}
      {pad.protected_devices.length>0&&<details><summary>设备标识</summary>{pad.protected_devices.map(id=><p key={id} translate="no"><code>{id}</code></p>)}</details>}
    </>:<>
      <p role="status">当前没有运行中的虚拟手柄输出</p>
      <p>选择 Xbox／DualShock 4、保存配置并开始映射后，这里显示实际创建结果。仅观察输入不会创建虚拟手柄。</p>
      {status?.message&&<p>{status.message}</p>}
    </>}
    <p>HIDMaestro Xbox 使用 XInput 伴随接口，传统游戏控制器窗口可能不列出它。请以上面的实际运行状态判断是否建立输出。</p>
    <p>部分游戏只读取第一个 XInput 玩家槽位。若显示第 2–4 槽位，请先停止映射，断开其他 XInput 手柄，再重新开始。</p>
    <button type="button" className="secondary-btn" onClick={onOpenLegacy}>打开传统游戏控制器窗口</button>
  </Dialog>;
}
