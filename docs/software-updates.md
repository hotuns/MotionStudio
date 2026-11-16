# 软件更新与发布

「设置与组件 → 软件更新」显示当前版本、版本说明与下载进度。桌面版启动时检查一次，也可手动检查。检测到新版本后，主窗口顶部常驻显示版本号和「查看更新」按钮，滚动或切换页面时仍然可见；点击直接进入软件更新。只有点确认才会退出并切换版本。安装前会要求保存或撤销草稿、结束录入和校准，并停止映射。配置和本机设置仍使用原来的应用数据目录。

安装版使用 Tauri 的 NSIS 更新流程。从 0.17.0 起，正式发布只提供安装包。0.16.0 和 0.16.1 的免安装版需要退出后手动运行新版安装包，再从安装版启动。原免安装目录可以保留，已有配置和本机设置仍从同一个应用数据目录读取。

0.15.2 及更早的包没有更新器，需先从 [GitHub Releases](https://github.com/hotuns/MotionStudio/releases) 手动换到支持在线更新的版本。JSM Studio 使用自己的更新通道。

## 发布新版本

更新器使用 GitHub 最新正式发行版里的 `latest.json`。预览版和草稿不会进入这个通道。清单的 `windows-x86_64-nsis` 条目指向安装包，签名使用 Motion Studio 的密钥并包含包的版本。更新签名与 Windows 的安装程序证书独立。

默认在本机编译、签名并上传 GitHub，复用本地 Cargo 构建缓存。推送提交或版本标签不会自动启动云端发布构建。

1. 本机需有 Windows、Rust/MSVC 和 Node/npm；正式构建时脚本会执行 `npm ci`，按锁文件准备前端依赖，保留 Cargo 编译缓存。默认使用 `~/.tauri/motion-studio-release/updater.key`，并核对旁边 `.pub` 与应用公钥一致。也可以在当前终端设置 `TAURI_SIGNING_PRIVATE_KEY`（私钥路径或内容）及需要的 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。后续版本沿用现有密钥，私钥保存在仓库之外。
2. 同步 `package.json`、`package-lock.json`、`src-tauri/tauri.conf.json` 和 `src-tauri`、`crates/engine`、`crates/host` 的 Cargo 版本及 `Cargo.lock`，写好 `docs/releases/vX.Y.Z.md`。
3. 完成本次改动必要的检查，提交并推送到 `origin/main`，保持工作区干净。已通过且未受修改影响的检查不再重复。
4. 运行 `npm run release:local`。脚本编译并签名 NSIS 安装包，复用现有 `package:update` 验签，创建指向该提交的版本标签，上传四个发布文件，逐个核对 GitHub 返回的 SHA-256 后发布为 Latest。

GitHub 身份优先使用 `GH_TOKEN` / `GITHUB_TOKEN`，否则读取已有 Git 凭据；需要仓库 Contents 写权限，不需要安装 `gh`。签名密钥和凭据不会写入发布文件。

```powershell
npm run release:local -- --check
npm run release:local
```

`--check` 只检查版本、工具、签名配置和 GitHub 权限，不编译、不创建标签、不上传。正式发布会阻止脏工作区、未推送源码、版本冲突和不匹配的资产；构建前后也会确认源码没有变化。

若构建已完成、上传途中断网，在同一干净提交上运行 `npm run release:local -- --publish-only`。它重新验证签名及源码记录，只补传缺少的文件，不重新编译，也不覆盖已有文件。源码记录存于 `artifacts/release-vX.Y.Z.source.json`，包含提交和四个文件的哈希，不上传。已正式发布的版本不重新打包覆盖，后续改动使用新版本。

完整发布资产在 `artifacts/release-vX.Y.Z/`：安装包、`.sig`、`latest.json`、`SHA256SUMS.txt`。`package:update` 使用正式公钥验证安装包的 BLAKE2b-512 / Ed25519 签名、可信注释签名和其中的版本号；任何一项不匹配都会中止打包。发布流程不安装软件，不运行游戏或物理手柄测试。

## 云端备用构建

本机不可用时，先推送准备好的 `vX.Y.Z` 标签，在 Actions 中手动运行 **Build Windows release (manual fallback)**，填写该标签。云端使用仓库 Secrets 中现有的 `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`，构建并验签后创建 Release 草稿；核对后再发布为正式版。不要为同一版本同时执行本机发布和云端备用构建。

本地开发不需要私钥，常规 `npm run build` 可照常使用。只准备签名产物、不上传时，设置上述签名环境变量后也可继续分别执行：

```powershell
npm run build -- --ci --bundles nsis --config src-tauri/tauri.release.conf.json -- --locked
npm run package:update
```

发布脚本的模拟验证使用 `npm run test:release`，不连接物理后端、不创建真实 GitHub Release。
