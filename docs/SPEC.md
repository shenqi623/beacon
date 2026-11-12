# Beacon — 新站点自动接入 GTM / GA4 / GSC

> 项目名为暂定，可全局替换。本文档描述功能与已确定的技术方向。

## 1. 目标

用户输入一个域名，系统自动完成：

1. 创建 GTM 容器，得到 `GTM-XXXX`
2. 创建 GA4 属性与 Web 数据流，得到 `G-XXXX`
3. 在 GTM 中添加 GA4 标签（所有页面触发）并发布
4. 通过 GTM 方式验证 Search Console 所有权（网址前缀资源）
5. 将站点加入 Search Console 并提交 sitemap

## 1.1 技术形态（已确定）

- **桌面应用**：使用 Tauri（系统 WebView + Rust 后端）。
- **界面**：Vue（现有前端可沿用）。
- **后端**：Rust，负责 Google API、本地存储、任务执行；**不使用** Node 服务端作为正式运行时。
- **配置**：全部通过应用内可视化界面完成，不依赖手改 `.env`。
- **数据目录**：配置、凭证、数据库保存在系统用户数据目录；Client Secret、Refresh Token 不得写入代码仓库，不得完整返回给前端，不得写入日志。

## 2. 功能清单

### 2.1 添加站点
- 输入：域名（必填）、站点名称（必填）、sitemap 地址（选填，默认 `https://{域名}/sitemap.xml`）
- GTM 监听预设（均可取消勾选，**默认全开**）：点击、出站链接、表单提交；选项随站点任务持久化，仅在添加时配置。勾选后须成功启用对应 GTM 内置变量（如 Click URL、Form ID），否则「发布 GTM」失败并提示，不静默降级
- 提交后创建一个站点任务，按第 3 节流程自动执行
- 输入校验：域名格式合法；同一域名不能重复创建任务

### 2.2 站点列表与状态
- 列出所有站点：域名、名称、当前步骤、状态（进行中 / 等待部署 / 失败 / 完成）、更新时间
- 站点详情：每一步的执行结果、产出的 ID（GTM ID、GA4 衡量 ID、属性 ID 等）、失败原因、已选 GTM 监听预设
- 站点详情中展示 GTM 安装代码片段（head 部分和 body 部分），提供一键复制

### 2.3 等待部署
- GTM 发布完成后，流程暂停在"等待部署"，提示用户把 GTM 代码片段部署到网站
- 提供"检查部署"操作：抓取站点首页，检查 HTML 中是否包含该 GTM ID
- 检测通过后自动继续后续步骤（GSC 验证等）

### 2.4 失败重试与继续
- 任一步骤失败时，任务停在该步骤，并显示失败原因
- 提供"从失败处继续"操作，从失败的步骤重新执行，已完成的步骤不重复执行

### 2.5 预演模式
- 提供"预演"选项：只展示将要执行的操作和参数，不实际创建任何资源

### 2.6 授权检查
- 提供"检查授权"功能，确认 Google 授权是否有效
- 每次执行任务前自动检查一次；授权失效时提示需要重新授权，并停止执行

### 2.7 配置项（全部可视化）
应用内提供统一配置界面，至少包括：

| 配置项 | 说明 |
|---|---|
| GA 账号 ID、GTM 账号 ID | 人工预先创建后填入 |
| 默认时区、默认币种 | 默认 `Asia/Shanghai`、`USD` |
| GTM 写操作间隔、GSC 验证重试间隔与次数 | 有合理默认值 |
| Google Client ID、Client Secret | 界面填写；Secret 仅显示是否已设置，可覆盖更新 |
| Refresh Token | 界面通过「授权登录」获取，不要求用户手抄 |
| HTTPS 代理 | 可开关；填写代理地址（如 `http://127.0.0.1:7897`） |

不要求用户手改配置文件。可选提供「打开数据目录」入口。

### 2.8 开通后健康检查
- 站点详情提供「健康检查」操作：站点非执行中时可手动触发
- 检查项（缺产出则跳过）：
  1. 站点首页 HTML 是否仍包含该站 GTM ID
  2. GTM 线上版本是否仍含对应 Google 代码，以及已勾选的点击 / 出站 / 表单事件标签
  3. GA4 属性与 Web 数据流是否仍存在且衡量 ID 一致
  4. Search Console 网址前缀资源是否仍可访问，且已提交的 sitemap 仍存在
- 结果写入本机站点记录并在详情展示；总状态：任一项失败为异常，全部通过为健康，全部跳过为未知
- 不改变自动化流水线步骤顺序

## 3. 自动化流程

每个站点任务按以下步骤顺序推进：

| 步骤 | 做什么 | 调用的 Google API | 产出 |
|---|---|---|---|
| 1. 创建 GTM 容器 | 在指定 GTM 账号下创建 Web 容器，名称为站点名称 | Tag Manager API v2 `accounts.containers.create`（usageContext: web） | 容器 ID、GTM ID（publicId） |
| 2. 创建 GA4 属性 | 在指定 GA 账号下创建属性，使用默认时区和币种 | Analytics Admin API `properties.create` | 属性 ID |
| 3. 创建数据流 | 创建 Web 数据流，网址为 `https://{域名}` | Analytics Admin API `properties.dataStreams.create`（WEB_DATA_STREAM） | 衡量 ID（G-XXXX） |
| 4. 发布 GTM | 新建/复用工作区 → 添加 Google tag（类型 `googtag`，`tagId` = 衡量 ID，触发器所有页面 `2147479553`）→ **先启用监听所需内置变量（失败则整步失败）** → 按站点勾选幂等创建点击 / 出站 / 表单的触发器与 GA4 事件标签（`gaawe`，事件名分别为 `click` / `outbound_click` / `form_submit`，含 Click/Form 参数与出站过滤）→ 创建版本 → 发布；线上已有 Google 代码且所选监听标签均已存在则跳过。启用内置变量的空 POST 可直连重试（减轻代理导致的 411） | Tag Manager API v2 `workspaces.*`（triggers / tags / built_in_variables / create_version）、`versions.publish` | 版本 ID |
| 5. 等待部署 | 展示代码片段，等待网站部署并检测通过 | 无（抓取站点首页） | — |
| 6. 验证 GSC | 获取验证令牌后执行验证，验证方式 `TAG_MANAGER`，资源类型 `SITE`，标识为 `https://{域名}/` | Site Verification API `webResource.getToken`、`webResource.insert` | 验证成功 |
| 7. 加入 GSC | 添加网址前缀资源，并提交 sitemap | Search Console API `sites.add`、`sitemaps.submit` | — |

全部完成后任务状态变为"完成"。

## 4. 业务规则

1. **不重复创建**：每一步执行前先检查资源是否已存在（按名称或域名匹配），存在则直接复用，不重复创建。创建请求遇 5xx 时先复查再决定是否重试，避免重复创建。
2. **可继续**：失败后从失败步骤继续，已完成步骤的产出要保留。
3. **自动重试**：遇到限流（429）或 Google 服务端错误（5xx）时自动重试，间隔逐渐加大，最多 5 次；其他错误直接判定失败。
4. **GTM 限速**：GTM 的写操作逐个执行，两次之间至少间隔 4 秒（可配置）。
5. **验证延迟**：GTM 发布到线上有延迟，GSC 验证失败时每 60 秒重试一次，最多 15 次（可配置）。
6. **安全**：Client Secret、Refresh Token 只存在本机后端（Rust）与用户数据目录，不能出现在前端完整回传、代码仓库或日志中。

## 5. 认证（已确定）

- 使用单个 Google 账号的 OAuth Refresh Token 调用所有 API，**不使用服务账号**。
- Refresh Token 通过应用内「授权登录」完成（系统浏览器打开 Google 授权页，本机回环接收回调），写入用户数据目录。
- 所需权限范围：analytics.edit、tagmanager.edit.containers、tagmanager.edit.containerversions、tagmanager.publish、siteverification、webmasters。

## 6. 不在范围内

- 不创建 GA 账号、GTM 账号（人工预先创建）
- 不负责把 GTM 代码部署到网站，只提供代码片段和部署检测
- 不处理 DNS，不使用网域资源
- 不做浏览器自动化（不模拟登录、不模拟点击）
- 不提供完整 GTM 配置器（任意标签 / 触发器 / 变量编辑）；仅支持添加站点时的预设开关
- 不做定时/后台巡检；不做真实用户点击或转化打点验证；不做多站健康汇总看板

## 7. 开发顺序（每完成一项停下等确认）

> 以下为 Tauri + Rust 改造顺序。原 Node 实现仅作参考，正式运行时以 Rust 为准。

1. Tauri 桌面壳 + 可视化配置（含 OAuth 凭证、代理、授权登录）+ 授权检查
2. 添加站点、创建 GTM 容器、等待部署
3. 创建 GA4 属性与数据流
4. 发布 GTM
5. GSC 验证、加入 GSC、提交 sitemap
6. 站点列表与详情、失败继续、预演模式（对接 Rust）
