# Beacon

本机桌面应用：把新站点自动接入 **GTM / GA4 / Search Console**。

技术栈：**Tauri 2 + Rust + Vue 3**。配置与凭证保存在本机用户数据目录，不进安装包、不进仓库。

## 功能概览

- 可视化配置（OAuth、代理、GA/GTM 账号）与应用内授权登录
- 添加站点后自动创建/复用 GTM 容器、GA4 属性与数据流并发布 GTM
- 可选监听预设：点击 / 出站 / 表单
- 等待部署检测、Search Console 验证与 sitemap 提交
- 失败从断点继续、预演模式、开通后健康检查

详细步骤见 [docs/使用手册.md](docs/使用手册.md)，需求说明见 [docs/SPEC.md](docs/SPEC.md)。

## 本地开发

要求：Node.js 22+、pnpm、Rust（stable）、Tauri 系统依赖。

```sh
pnpm install
pnpm tauri:dev
```

本地打包：

```sh
pnpm tauri:build
```

Windows 产物常见于 `src-tauri/target/release/bundle/nsis/`；macOS 常见于 `…/bundle/dmg/`。

## GitHub 打包（Windows + macOS）

仓库已包含 `.github/workflows/build.yml`：

| 触发方式 | 行为 |
|---|---|
| Actions 页手动 **Run workflow** | 构建并上传安装包为 Artifacts |
| 推送标签 `v*`（如 `v0.1.0`） | 构建并创建 **Draft** Release |

矩阵目标：

- Windows x64（NSIS）
- macOS Apple Silicon（`aarch64-apple-darwin`）
- macOS Intel（`x86_64-apple-darwin`）

未配置 Apple 代码签名时，macOS 包为临时签名；部分系统下载后可能提示「已损坏」，需在本机按 Apple 文档处理或自行配置签名证书。

## 安全

- 不要提交 `.env`、`data/`、Client Secret、Refresh Token
- 正式授权请用应用内「授权登录」；`scripts/get-token.js` 仅为遗留脚本
