# 输出路径与游戏兼容性

核查日期：2026-11-08。用户反馈三角洲行动可能不接受 SendInput；本次未进入真实游戏测试，也未找到能确认具体拦截原因或保证特定驱动兼容的官方结论。

Motion Studio **0.10.0 分别选择键鼠和手柄后端**：默认 HIDMaestro 键鼠配合 FlexInput 原生 ViGEm Xbox／DualShock 4，也可明确选 HIDMaestro 手柄。键鼠覆盖组合键、相对位移、双向滚轮和侧键。用户在「设置与组件」安装驱动、选择输出后端，开始映射时授权创建虚拟设备。输入层使用 FlexInput SDL provider 逐事件链路。具体使用、异常释放和版本限制见 [HIDMaestro 输出](hidmaestro-output.md)。

Motion Studio 0.5.0 的键盘、鼠标按钮、侧键、滚轮和相对位移使用 SendInput；键盘已使用扫描码，位移带 MOVE_NOCOALESCE。引擎检查系统返回值并在失败时释放输出，但系统接收事件不代表游戏已经采用。SendInput 受 UIPI 限制，只能向相同或更低完整性等级的应用注入。[Microsoft 文档](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)

虚拟 Xbox／DualShock 4 使用 ViGEm 手柄报告。游戏原生支持手柄时，可把体感输出改为右摇杆并重新调校；本文没有确认三角洲国服当前版本的原生手柄支持。

| 候选路径 | 评估 |
|---|---|
| HIDMaestro 虚拟键鼠 | 优先评估的后端。UMDF2 HID 设备报告可进入 HID／Raw Input 路径，属于设备层输出，不等于应用代码运行在内核。FlexInput 已有独立客户端可参考。 |
| FakerInput 虚拟键鼠 | 同样值得验证的 UMDF2 HID 后端。DS4Windows 文档记载其用于部分 SendInput 不可用的场景，包括某些反作弊过滤。原仓库已归档，需核验当前系统与目标游戏兼容性。 |
| 自研 VHF HID 驱动 | 微软框架的该实现需要内核态源驱动，需承担驱动开发、签名、部署、恢复和 Windows 兼容维护。当前不优先。 |
| Interception | 截获和控制键鼠的另一种驱动方案，涉及设备栈、安装和分发许可审查，当前不作为默认方案。 |

依据：[FlexInput 固定提交](https://github.com/x-iso/FlexInput/blob/c69f53ad3bca4ed34ef5f2d63509addaa7c767be/README.md#virtual-outputs)、[HIDMaestro](https://github.com/hifihedgehog/HIDMaestro)、[Microsoft VHF](https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/virtual-hid-framework--vhf-)和 [Interception](https://github.com/oblitum/Interception)。

HIDMaestro 官方明确说明，虚拟设备仍可被内核反作弊识别，它不隐藏虚拟设备，也不保证绕过反作弊。“输出更底层”和“三角洲接受这种输出”需要分别验证。[已知边界](https://github.com/hifihedgehog/HIDMaestro#known-limitations)

用户提到的项目可能是 [Ryochan7/FakerInput](https://github.com/Ryochan7/FakerInput)。工程使用 WindowsUserModeDriver10.0 和 UMDF 2，提供键盘、相对鼠标和绝对鼠标报告。[DS4Windows 文档](https://kanuan.github.io/DS4WSite/about/companion-drivers/#travis-nickles-fakerinput)说明它可以用于部分 SendInput 失效场景，但这不是三角洲当前版本的兼容证明。核查时最新官方 release 为 0.1.1（2025-12-19），仓库已于 2026-08-19 归档。

另一个 [uiii/FakeInput](https://github.com/uiii/FakeInput/blob/master/src/mouse_win.cpp) 名称不含 r，其 Windows 键鼠实现仍调用 SendInput，不能与上述虚拟 HID 驱动混淆。

0.6.0 保留 SendInput／ViGEm 兼容模式，界面显示实际输出后端。选择 HIDMaestro 后，驱动缺失、版本不匹配或报告交付失败都会停止输出并报告原因，不会静默回退。FakerInput、VHF 和 Interception 未接入。开发过程没有安装 HIDMaestro 或创建真实虚拟设备，目标游戏是否接受该路径仍需用户手动验收。

0.10.0 不再把 joy.cpl 作为唯一创建检查。ViGEm Xbox 需获得有效槽位，HIDMaestro Xbox 需确认本设备 XUSB 就绪；「检查虚拟手柄」显示实际运行目标和后端。见 [原生接入核查](native-input-output.md)。
