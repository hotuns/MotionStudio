# Motion Studio 社区配置

在软件的「配置库 → 社区配置」按游戏或手柄查找，或在「输入配置码」中粘贴 `MS-` 开头的短码。预览后选择「添加到本地」，即可编辑副本。配置变更通过此 GitHub 目录发布，用户浏览和导入时无需登录。

本地配置的「复制配置码」生成 `MS1.` 开头的完整码，可以直接发给朋友离线导入。

## 投稿

1. 在配置库导出已核对的 Studio TOML。为配置起一个易辨识的名字，附上游戏、手柄型号或连接方式、输出方式和使用说明。
2. 将文件保存为 UTF-8、LF 换行，计算文件原始字节的 SHA-256。用小写哈希命名 `profiles/<sha256>.toml`。短码为 `MS-` 加哈希前八位的大写形式。
3. 在 `index.json` 加入目录条目。`name`、`output` 与 TOML 的 `name`、`virtual_gamepad` 一致。`verified` 表示维护者已核对的游戏测试状态。起步示例填 `false` 并在 `notes` 说明适用条件。
4. 用 GitHub Pull Request 提交目录和文件。Windows 检查会验证短码、文件校验值和 Motion Studio 导入结果。

已发布的短码对应固定内容。需要改动配置时，按新内容生成文件和短码，旧短码可以继续供已有分享使用。

通用起步配置来自 Motion Studio 的 [examples](../examples)。三角洲行动起步配置通过 Motion Studio 的 JSM 导入转换自 [JSM Studio 社区示例](https://github.com/hotuns/JSM_Studio/blob/gh-pages/community-configs/profiles/delta-force-basic.txt)，原作者为 JSM Studio。[源项目 MIT 许可证](https://github.com/hotuns/JSM_Studio/blob/v0.2.7/LICENSE.md)
