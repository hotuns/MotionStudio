# 软件更新与发布

「设置与组件 → 软件更新」显示当前版本、版本说明与下载进度。桌面版启动时检查一次，也可手动检查。只有点确认才会退出并切换版本。安装前会要求保存或撤销草稿、结束录入和校准，并停止映射。配置和本机设置仍使用原来的应用数据目录。

安装版使用 Tauri 的 NSIS 更新流程。从 0.17.0 起，正式发布只提供安装包。0.16.0 和 0.16.1 的免安装版需要退出后手动运行新版安装包，再从安装版启动。原免安装目录可以保留，已有配置和本机设置仍从同一个应用数据目录读取。

0.15.2 及更早的包没有更新器，需先从 [GitHub Releases](https://github.com/hotuns/MotionStudio/releases) 手动换到支持在线更新的版本。JSM Studio 使用自己的更新通道。

## 发布新版本

更新器使用 GitHub 最新正式发行版里的 `latest.json`。预览版和草稿不会进入这个通道。清单的 `windows-x86_64-nsis` 条目指向安装包，签名使用 Motion Studio 的密钥并包含包的版本。更新签名与 Windows 的安装程序证书独立。

1. 首次发布前，在 [仓库的 Actions Secrets](https://github.com/hotuns/MotionStudio/settings/secrets/actions) 配置 `TAURI_SIGNING_PRIVATE_KEY`，值是与 `src-tauri/tauri.conf.json` 公钥配对的私钥文件内容。若私钥有口令，再配置 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。不要把私钥写进代码或发布资产。后续版本继续用同一密钥，并保留私钥备份。
2. 同步 `package.json`、`package-lock.json`、`src-tauri/tauri.conf.json` 和 `src-tauri`、`crates/engine`、`crates/host` 的 Cargo 版本及 `Cargo.lock`。把版本说明写入 `docs/releases/vX.Y.Z.md`。
3. 推送前在本地检查本次改动。前端改动可运行 `npm run build:web`，引擎改动可运行对应的模拟输入测试。推送提交和 `vX.Y.Z` 标签后，「Prepare Windows release」会构建并签名安装包，验证签名后创建 GitHub Release 草稿。
4. 核对草稿中的安装包、对应的 `.sig`、`SHA256SUMS.txt` 和 `latest.json`，再发布为正式版并设为 Latest。旧安装版可在应用内检查更新。

本地开发不需要私钥，常规 `npm run build` 可照常使用。如需在本机准备签名发布，在当前终端配置同名签名环境变量后执行：

```powershell
npm run build -- --ci --bundles nsis --config src-tauri/tauri.release.conf.json -- --locked
npm run package:update
cargo test --locked --release -p motion-studio updates::tests::signed_release_packages -- --ignored
```

完整发布资产在 `artifacts/release-vX.Y.Z/`。签名验证使用正式公钥，并检查修改过的安装包和不匹配的版本号能否被拒绝。验证过程只读取安装包。
