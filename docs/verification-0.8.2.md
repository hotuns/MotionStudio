# Motion Studio 0.8.2 验证记录

日期：2026-11-08，Windows x64。

## 报错根因

自动隐藏提示“虚拟手柄不能作为自动隐藏目标”，但本机保存的选中接口属于飞智 APEX 6 的实体 HID 接管功能。只读 PnP 查询确认其祖先依次经过 USB 复合设备、USB 根集线器、PCI／ACPI 和 `ROOT\ACPI_HAL\0000`。旧规则将任何 `ROOT\...` 祖先都视为虚拟，因此在写入隐藏规则之前错误停止映射。

同时发现普通 HID 集合的 CompatibleIds 可以为空；手柄、键盘、鼠标和厂商用途实际位于 HardwareIds。原适配器只读取 CompatibleIds，无法完整判断这些功能。

## 修复规则

- `ROOT\ACPI_HAL\...` 作为系统祖先处理；该节点本身仍禁止作为隐藏目标。ViGEm、HidMaestro、软件节点及其他保守判定的软件根继续被排除。
- 使用现有 FlexInput `devnode::strings_property` 同时读取硬件 ID 和兼容 ID，识别游戏用途、键鼠用途及厂商自定义 HID。
- 使用原 `xinput_devnodes` 解析实际 XUSB 接口实例。复合手柄优先隐藏独立 USB 功能及其游戏 HID，保留公共父设备和键鼠兄弟功能；如果 XUSB 就是公共 USB 节点，保留原有的共享键鼠／未知功能拒绝规则。
- 区分“确实是虚拟设备”和“设备实例不支持自动隐藏”，使错误原因与实际判定一致。

APEX 6 布局的合成测试验证以下目标集合，不依赖型号白名单：

| 功能 | 自动规则 |
|---|---|
| 所选 `HID\...&MI_02` 接管接口 | 隐藏 |
| `HID\...&IG_00` 游戏接口 | 隐藏 |
| 实际 `USB\...&MI_00` XUSB 功能 | 隐藏 |
| `MI_01` 下的键盘、鼠标集合 | 保留兄弟功能 |
| 同设备其他未知兄弟功能 | 保留 |
| 公共 USB 复合设备父节点 | 不加入本次规则 |
| ACPI 电脑根、虚拟手柄 | 禁止作为目标 |

HidHide 按设备实例 ID 控制访问；XInput 还需要覆盖其 USB 访问路径。上述功能节点选择基于实际 XUSB 接口和祖先关系，真实游戏的重新枚举及可见性仍需用户验证。参考 [原生 API](https://docs.nefarius.at/projects/HidHide/API-Documentation/) 和 [XInput 隐藏说明](https://docs.nefarius.at/projects/HidHide/CLI-Usage/#hiding-xbox-xinput-controllers)。

## 自动检查

- 修复前，包含完整 ACPI 系统祖先链的用例稳定复现同一报错。
- **Rust 253 项通过，0 失败。** 在原有测试之上新增 5 项回归：系统根判定与禁写、完整物理祖先链、空兼容 ID 时的硬件用途读取、复合 XUSB 与键鼠兄弟分离、XUSB 分支内键鼠／未知功能继续拒绝。
- 三个应用 crate 的格式处理及严格 Clippy（`--locked --no-deps --all-targets -D warnings`）通过。
- 翻译目录更新至 **1,214 条**；前端 898、后端 335 条源文案覆盖及参数占位符检查通过。
- 真实设备仅做只读 PnP 属性查询，回归测试使用合成拓扑与模拟驱动。没有打开真实手柄进行输入测试、发送桌面输入或改动真实 HidHide 规则。

- 最终静态 CRT 发布版引擎 IPC：**6 项通过、0 失败，1 项实体设备检查跳过**，包含独立输出保护进程的 EOF 与心跳超时退出。

## 发布验证

- 静态 CRT 引擎、桌面应用和 Windows x64 NSIS 安装器构建通过。首次桌面编译中的 rustc 访问冲突日志保留，使用同一份源码和构建参数重试成功。
- 产品版本 0.8.2；桌面程序内固定的最终引擎 SHA-256 与资源一致，四个 HIDMaestro 内嵌载荷和既有功能标记核验通过。
- 引擎 PE 仅延迟导入同一份 SDL3.dll，桌面程序无 SDL 导入；142 个 vendor 文件及固定运行资源完整性校验通过。
- 安装包与便携包核验产品版本、资源哈希、输入 provider 元数据、文档／许可证和示例清单。产物校验值在 `artifacts/checksums.json`。
- HIDMaestro 驱动保持 2.0.0.2201，修复应用即可，无需重装驱动。实际隐藏后游戏能否重新枚举，需要用户启动新版映射后验证。
