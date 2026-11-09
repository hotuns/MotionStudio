# Motion Studio 0.6.0 验证记录

日期：2026-11-07，Windows x64。此版本将默认输出切换为 FlexInput HIDMaestro，保留 SendInput／ViGEm 兼容模式。

## 自动验证

- Rust：133 项测试，覆盖原有映射、体感、配置、JSM 导入、手柄导航及新 HID 报告、组合键／滚轮布局、6 键限制、DS4 全部扳机取值、中立值、XInput 轴向、真实 Windows 本地管道的模拟通信、对端 PID 校验、受限协议、异步心跳及故障处理。模拟管道不启动提权、不创建系统虚拟设备。
- 静态检查：三个应用 crate 的 `cargo fmt --check`、`cargo clippy --locked --no-deps --all-targets -- -D warnings`，TypeScript 检查与 Vite 构建。固定 vendor 依赖仅校验来源和编译，不为消除上游 lint 修改源码。
- 界面：原 16 项组合键／滚轮／侧键流程、16 项 JSM 导入及手柄导航流程，加 7 项 HID 后端流程；验证默认选择、本机持久化、显式安装入口、实际后端显示、运行／准备状态锁定、版本错误不回退及紧凑深色布局。浏览器 Tauri API 均为模拟，不启动桌面应用。
- 资源：109 个固定 FlexInput 源文件／模型／驱动文件 SHA-256、SDL 修复版、2 个官方可选安装器和 HIDMaestro MIT 许可证；新增 30 项还对照固定提交的 Git blob。运行资源清单记录 HIDMaestro SDK 2.0.1、驱动 INF 2.0.0.2201 和四个内嵌驱动载荷哈希。
- 发布：Release 引擎与 NSIS／便携包构建；发布引擎 IPC 用例使用关闭实体输入后端的 SDL 合成事件，6 项通过，1 项实体手柄测试跳过。核对安装包／ZIP 的版本、文件清单、运行资源哈希与发布二进制一致性。

对应命令与原始记录位于 `.test-data/rust-0.6.0.log`、`.test-data/clippy-0.6.0.log`、`.test-data/build-0.6.0.log`、`.test-data/ui-check/*-report.json` 和 `.test-data/package-0.6.0.ps1`。便携包只包含最终使用文档，开发日志留在源码工作区。

## 真机验收边界

没有在开发验证中安装 HIDMaestro、信任签名证书、创建系统虚拟设备、发送桌面键鼠、占用实际手柄、修改 HidHide 规则或进入三角洲。安装、UAC、系统枚举、真实键鼠／手柄输出、强制终止后的驱动释放、长期体感手感及反作弊兼容性均仍需用户手动验证。

HIDMaestro 输出依赖内置固定版本。检测到其他版本或不完整安装时明确报错，当前没有强制重装／卸载或覆盖第三方驱动的入口。虚拟设备可能被游戏或反作弊识别，报告交付确认也不等于游戏采用输入。
