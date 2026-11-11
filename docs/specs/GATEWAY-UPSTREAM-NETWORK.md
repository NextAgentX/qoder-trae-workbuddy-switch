# 网关上游出站网络（直连 / 系统代理）

> 状态：现行 · 2026-11-08 落地

## 一句话

**三条产品线的上游请求一律默认直连**，只有显式打开 WorkBuddy 的「上游连接方式」开关才走系统代理。

## 为什么必须钉住这条

`reqwest::Client::builder()` **不带 `.no_proxy()` 时会自动读取** `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY`。而这些变量典型地由代理客户端写入 **User 作用域**（持久化进注册表），**关掉代理客户端也仍然存在**。

后果是每一条上游请求都先去连环境变量里那个本地代理端口（典型如 `127.0.0.1:7897`）：

| 代理状态 | 表现 |
|:--|:--|
| 端口已关 | 连接被拒 ⇒ `error sending request for url (...)` |
| 端口还在但链路坏 | 代理侧把 TLS 握手中断 ⇒ `SSL: UNEXPECTED_EOF_WHILE_READING` |

两种表现都长得像「**必须开着代理才能用**」，而真实情况往往相反。某台 Windows 机器上的对照实测（TUN 关闭）：

| 目标 | 直连 | 经本地 HTTP 代理 |
|:--|:--|:--|
| `www.workbuddy.ai` | ✅ TLS 1.3，约 0.2s | ❌ `UNEXPECTED_EOF_WHILE_READING` |
| `www.codebuddy.cn` | ✅ | ✅ |
| `qoder.com` | ✅ | ✅ |
| `api3.qoder.sh` | ✅ | ✅ |

即「代理对某个上游是坏的那条路、直连才是通的」完全可能，所以**默认不该由环境变量替用户决定出口**。

## 实现

| 产品线 | 位置 | 做法 |
|:--|:--|:--|
| Qoder | `crates/buddy-switch-gateway/src/qoder/mod.rs` | 硬编码 `.no_proxy()` |
| Trae | `crates/buddy-switch-gateway/src/trae/mod.rs` | 硬编码 `.no_proxy()` |
| **WorkBuddy** | `crates/buddy-switch-gateway/src/state.rs` | 按配置 `upstream_via_proxy`（**默认 `false`**）决定是否 `.no_proxy()` |
| 共享 HTTP 层 | `crates/buddy-switch-core/src/modules/config.rs` | `http_request` 在拿到响应前失败时**回落直连重试一次** |

### WorkBuddy 为何用配置项而非硬编码

Qoder / Trae 硬编码即可（没有「只有代理能到上游」的常见场景）。WorkBuddy 的 `www.workbuddy.ai` 是国际站，存在少数网络下只有代理可达，因此留了逃生口：

- 配置键：`upstream_via_proxy`（`#[serde(alias = "upstreamViaProxy")]`，默认 `false`）
- 界面：WorkBuddy → 网关 → 模型 页的「上游连接方式」开关（复用 `ContextSourceCard`）
- ⚠️ **切换后需重启应用**：`reqwest::Client` 在网关状态初始化时构建一次，无法热改代理（与 `max_body_mb` 同类限制）

### 共享 HTTP 层的回落

`config.rs` 的 `http_request_with_proxy` 对**环境代理**这一档做回落：`send()` 返回 `Err` ⇒ 意味着零响应字节到达 ⇒ 换 `direct_client()`（`.no_proxy()`）重发一次。

- 只对环境代理回落；**显式** `Some(proxy)` 不回落（那是调用方明确指定的出口，不许偷偷改）
- 直连也失败 ⇒ 仍报**原始**错误，不掩盖真实故障

> 早期版本只认 `is_connect()` / `is_timeout()`，**漏掉了「代理端口活着但 TLS 被掐断」**这一最常见坏法，于是兜底不触发 —— 这正是「必须开代理」现象的成因。现已放宽为「任何发送前失败」。

## 回归防线

- `state.rs::tests::upstream_client_defaults_to_direct_connection` —— 默认值必须是直连，旧配置缺键也必须是直连，且必须接受 camelCase 别名
- `state.rs::tests::gateway_config_serializes_with_snake_case_keys` —— 序列化键集**钉死**，新增配置项必须同步登记（否则前端会静默丢字段）

## 排查手法

```powershell
# 1) 上游是否可达（直连 vs 经代理）
#    用任意 Python/curl 分别测 443 的 TLS 握手，看是否 SSLEOF

# 2) 应用实际走了哪条路
$pid = (Get-Process buddy-switch-rust).Id
Get-NetTCPConnection -OwningProcess $pid -State Established |
  Group-Object RemotePort | Select-Object Name, Count
#   RemotePort 7897 = 走了系统代理；其它 = 直连

# 3) TUN 虚拟网卡残留（部分 VPN 客户端关闭界面后不清理）
Get-NetAdapter -ErrorAction SilentlyContinue |
  Where-Object { $_.InterfaceDescription -match 'TUN|TAP|wintun' } |
  Select-Object Name, Status
```

若虚拟网卡仍抢占默认路由，先在 VPN 客户端里彻底关闭 TUN 模式；这属于**网络层**问题，与网关出站配置无关。

## 相关

- 请求代理（把**客户端**指向本机网关，C 路）：[`P12-ARCH.md`](P12-ARCH.md) —— 与本文是**两件不同的事**：本文讲网关怎么出站，P12 讲客户端怎么进来
