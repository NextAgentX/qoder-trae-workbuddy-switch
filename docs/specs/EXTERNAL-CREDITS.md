# 第三方积分来源

> 状态：现行 · 2026-11-25 · 三家余额查询、OpenAI / Anthropic / Responses 本机网关及五入口共享 UI 规范已实现；豆包图片/视频/音乐生成接口已接入

## 目标与边界

豆包、Loomy、商汤小浣熊各有独立平台路由和「概览 / 账号 / 用量 / 网关 / 设置」入口。页面复用应用统一的页头、`StatTiles`、账号空态与弹窗、用量 `Tabs`、`SettingsGroup` / `SettingsFieldRow` 及卡片布局。余额保留各服务自己的单位，不进入 WorkBuddy 积分统计。余额查询仅读取余额，不执行签到、登录或其他会改变账户状态的动作。

## 本机直接代理

三家消费账号使用独立的本机监听端口：豆包 `57896`、Loomy `57897`、小浣熊 `57898`，仅绑定 `127.0.0.1`。各自的 API Key 保存在应用私有目录的 `external_gateway_{provider}.key`。支持 `/v1/models`、`/v1/chat/completions`、Anthropic `/v1/messages`、OpenAI Responses `/v1/responses` 和网关账本 `/v1/usage`；豆包另支持 `/v1/images/generations`、`/v1/video/generations`、`/v1/audio/generations`。应用的用量页面通过宿主 API 读取请求账本，避免直接从 WebView 跨源访问本机网关。认证支持 `Bearer <API Key>` 和 `x-api-key`。启动前必须先添加该 provider 的账号；请求只从对应 provider 的账号池轮询。

Loomy 通过消费客户端使用的 OpenAI 兼容模型服务转发；小浣熊使用 `/api/web/llm/v2` 消费模型服务，并以实际 `model_catalog` 构建模型目录。豆包使用 DoubaoWork 私有 `/chat/completion` SSE 协议转成 OpenAI 聊天响应，模型映射对齐 Doubao2API 的开源注册表。Anthropic 与 Responses 请求使用本仓协议转换器转成 Chat Completions，再将上游 SSE 转回对应协议；三家流式文本响应均边收边转换。豆包支持 base64 data URL 和远程 HTTPS 图片输入，并通过 DoubaoWork 上传接口转成附件。远程图片只接受解析到公网 IP 的 HTTPS 域名，不跟随重定向，固定 DNS 解析地址避免重绑定，限制 PNG/JPEG/WebP 且最大 8 MiB；音频输入和工具调用暂不支持。媒体生成提供 `/v1/images/generations`（模型 `doubao-image`，每次一张）、`/v1/video/generations`（模型 `doubao-video`，同步等待上游异步任务完成）和 `/v1/audio/generations`（模型 `doubao-music`，生成音乐）。图片比例支持 `1:1`、`16:9`、`9:16`、`4:3`、`3:4`。图片、视频和音乐生成协议参考 Doubao2API 的 Apache-2.0 公开客户端流程，未复制其实现代码；生成能力仍基于豆包非官方消费客户端私有接口，可能随上游版本变化。

请求仅在选中的账号返回错误状态或连接失败、且响应尚未开始时，按同一 provider 的账号池顺序重试；已开始发送的流绝不跨账号重放，以免重复消费。网关请求账本单独保存最近 500 条记录，记录 provider、协议、模型、HTTP 状态、完成时间及上游实际回报的 input/output tokens。上游未报告 token 用量时字段为空；该账本不代表平台扣点账单。它不接入现有 WorkBuddy / Trae / Qoder 网关账本。余额页仍另外呈现余额和本机采样差值，不把差值当作请求消费。

账号由用户手动添加，凭据保存在本机 `~/.buddy-switch/external_credit_accounts.json`，使用核心配置模块的私有目录与原子写入。读取列表接口不会返回凭据。单账号失败时保留最后一次成功余额并显示错误，不把失败伪装成 0。

用量页在上游余额刷新成功时记录本机余额快照（保存在浏览器本地存储，按平台和账号隔离）。快照差值只表示两次采样之间的余额变化，不代表上游逐笔消费，也不推算历史消耗。设置页可为每个平台独立配置余额自动刷新间隔与低余额提示阈值，阈值为 0 时关闭提醒。

## 数据来源

| 服务 | 凭据 | 余额接口 / 字段 |
|:--|:--|:--|
| 豆包 | 网页 Cookie | `POST www.doubao.com/alice/ug/campaign/v3/detail/` 的 `data.progress.total_amount` |
| Loomy | Web session token | `GET loomyad.xunfei.cn/api/v1/points/records` 的 `availableBalance`；缺失时按 `balance + dailyBalance` 计算 |
| 商汤小浣熊 | `xhx_token`（或包含该 Cookie 的整段 Cookie） | `GET xiaohuanxiong.com/api/web/points/v1/balance` 的 `available_points` |

小浣熊兼容旧版 `raccoon_refresh_token` Cookie：需要时调用同站 `/api/web/auth/v1/refresh`，并保存轮换后的 refresh token。新式 `xhx_token` 作为 Bearer 直接查询；不需要也不应强制用户提供 refresh token。

## 安全与失败处理

- 余额和模型上游仅访问代码中固定的 HTTPS 地址。为图片识别下载用户提交的 HTTPS URL 时，不发送账号凭据、不跟随重定向，只连接经校验并固定的公网 DNS 地址，限制图片类型和大小。
- Cookie 与 token 不放入查询响应、错误文本或日志。请求设有 15 秒超时。
- 401/403 和服务端业务错误显示重新登录或上游错误提示；余额字段缺失时不覆盖最近一次成功值。
- 查询余额不会调用每日签到，也不会自动执行积分领取。

## ZCode Start Plan

Issue #219 另提到「如果可以」自动领取 ZCode Start Plan。本应用没有 ZCode 官方登录态或受支持的领取 API。公开客户端流程要求先完成安全验证，再以 OAuth JWT、设备标识和验证参数请求领取；相关 issue 仍报告验证失败。部分开源代理尝试复刻无感验证码流程，但这不是 ZCode 官方支持的无交互接口，本应用不自动求解或伪造验证参数，也不直接复放领取请求。此可选项目前不实现；如果 ZCode 提供官方授权桥接或用户主动发起的受支持集成，再评估接入。

## 参考实现

- [AIcivilization/dsh-checkin-all](https://github.com/AIcivilization/dsh-checkin-all)：豆包凭据与小浣熊旧版 token/签到协议参考（MIT）。本功能只实现余额读取，不复制签到行为。
- [Patrick130306/loomy2api](https://github.com/Patrick130306/loomy2api)：Loomy session、积分字段与可用余额计算参考。只参考协议，不复制实现代码。
- [wicm84266964/Buddy2api](https://github.com/wicm84266964/Buddy2api)：消费级客户端登录态、Loomy / 小浣熊通道及网关管理参考。
- [7Crimson7/Agent2API](https://github.com/7Crimson7/Agent2API)：Loomy / 小浣熊的模型请求、模型清单、余额与请求报表参考。其 README 标注接口来自非官方客户端通信观察，可能随上游变化。
- [2951461586/Doubao2API](https://github.com/2951461586/Doubao2API)：豆包积分/历史与兼容网关端点参考（Apache-2.0）。
- [wangchuxiaoji-oss/doubao2api](https://github.com/wangchuxiaoji-oss/doubao2api)：豆包图片上传、附件和图片生成的客户端协议参考（Apache-2.0）；其 README 说明豆包客户端不支持原生工具调用。
- [zai-org/ZCode](https://github.com/zai-org/ZCode)：官方 Start Plan 客户端运行时与授权流程依据。
