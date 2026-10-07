#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""P12 spike · mitmproxy addon —— 按域过滤 + 记录 header 原始大小写 + 记录凭据替换结果。

## 用途（H2 / H3 / H4）

本 addon 是 **H2 / H3 / H4 的手动验证工具**，**不是产品代码**：

- **H2**（★ 真正的 go/no-go）：用户**手动**把自签 CA 装进系统信任库后，用 mitmproxy 做
  TLS 中间人重放 H1 的流量，**看客户端是否报证书错误**（= 证书固定 ⇒ A 路出局）。
  本 addon 只负责把「握手是否成功」这一现象记下来。
- **H3**：拦一个**真实业务请求**，按 `P12_MITM_REPLACE_AUTH` 替换凭据，记录上游是否接受
  （HTTP 200）。⚠️ Trae 的换号还需重做 `device_proof`（P-256 ECDSA）签名 —— 本 addon
  **做不到**，故 Trae 侧状态如实记为「需额外打通」（见 `P12-SPIKE-RESULTS.md`）。
- **H4**：记录客户端**实际发出的** header 名**原始大小写**（`anthropic-version` 还是
  `Anthropic-Version`），以及上游收到的是什么，用于判断转发层会不会改写大小写。

## ★ 安全边界（与设计 §1.6 / §5 一致）

- **只记录元数据**（方法 / 域 / 路径 / header **名** / 状态码 / 耗时）；
  **默认不落盘任何请求或响应正文**（`P12_MITM_ALLOW_BODY=1` 才会记，仅用于本地深度调试，
  且**只在用户知情时**使用 —— 正文可能含账号身份）。
- **按域过滤**：只有落在 `P12_MITM_DOMAINS` 白名单里的域才被记录 / 改写；其余域原样放过。
- 本 addon **不安装 CA、不改系统设置**：CA 的安装与移除一律由用户**手动**完成（决策 #3）。

## 用法

```bash
# 1) 先起 mitmproxy（它会监听一个本地端口；这一步需要你**手动**把自签 CA 装进系统信任库）
P12_MITM_DOMAINS="www.workbuddy.cn,www.codebuddy.cn,api.trae.cn,trae-api-cn.mchost.guru,copilot.tencent.com" \
P12_MITM_LOG="scripts/spike/p12-mitm.log" \
P12_MITM_CAPTURE_DIR="scripts/spike/p12-capture" \
mitmproxy -s scripts/spike/p12-mitm-addon.py -p 18888

# 2) 把客户端 HTTP/HTTPS 代理指向 127.0.0.1:18888，然后在客户端里点开「签到 / 积分 / 旅行」
# 3) 看 p12-mitm.log：若客户端**不报证书错误**且业务正常 ⇒ H2 通过
# 4) H4：对比 log 里 client_headers 的原始大小写与 upstream 收到的大小写
# 5) H3（可选）：设置 P12_MITM_REPLACE_AUTH 后重放，看 response 状态码是否 200

# 3') 还原：把客户端代理改回「无」/ 系统代理，并在系统信任库里**手动移除**自签 CA
```

## 环境变量

| 变量 | 含义 | 默认 |
|:---|:---|:---|
| `P12_MITM_DOMAINS` | 逗号分隔的域白名单（**只处理这些域**） | 空（**不处理任何域**，安全默认） |
| `P12_MITM_LOG` | 文本日志路径 | `scripts/spike/p12-mitm.log` |
| `P12_MITM_CAPTURE_DIR` | JSONL 结构化记录目录 | `scripts/spike/p12-capture` |
| `P12_MITM_REPLACE_AUTH` | 命中域时把 `Authorization` 换成该值（H3） | 空（不改写） |
| `P12_MITM_ALLOW_BODY` | `1` = 记录正文（**默认关，含敏感数据**） | 关 |

⚠️ **spike-only 工具**：`scripts/spike/*` 不得进打包产物（设计决策 #4）。
"""

from __future__ import annotations

import json
import os
import time
from pathlib import Path
from typing import Any, Dict, List, Optional

from mitmproxy import ctx, http  # type: ignore

# ---------------------------------------------------------------------------
# 配置（全部来自环境变量；缺省即「什么都不做」，这是刻意的安全默认）
# ---------------------------------------------------------------------------

_HERE = Path(__file__).resolve().parent


def _env(name: str, default: str = "") -> str:
    return os.environ.get(name, default).strip()


def _parse_domains(raw: str) -> List[str]:
    """把逗号分隔的域白名单解析成小写列表（去空、去重、保序）。"""
    out: List[str] = []
    seen = set()
    for part in raw.split(","):
        host = part.strip().lower()
        if host and host not in seen:
            seen.add(host)
            out.append(host)
    return out


def _host_matches(host: str, allowlist: List[str]) -> bool:
    """命中判定：精确相等，或（白名单项以 `.` 开头时）后缀匹配。

    刻意**不做**泛域名通配：spike 工具宁可少匹配、不可多匹配（多匹配 = 可能记录无关流量）。
    """
    h = (host or "").lower()
    for entry in allowlist:
        if entry.startswith("."):
            if h.endswith(entry) or h == entry[1:]:
                return True
        elif h == entry:
            return True
    return False


def _raw_header_fields(headers: Any) -> List[List[str]]:
    """取 header 的**原始大小写**字段名。

    mitmproxy 的 `Headers` 对象在多数访问路径上会**小写化** header 名。要拿到客户端**实际
    发出**的原始大小写（H4 的关键），须读其底层 `fields`（`(name, value)` 二元组序列，
    保留原始字节大小写）。不同 mitmproxy 版本字段名有差异，这里做多路兜底：

    1. `headers.fields`（mitmproxy 10+ 保留原始大小写的底层序列）；
    2. `headers.items(multi=True)`（老版本 API）；
    3. 退回小写视图并**标注** `casing_preserved=False`，避免把「拿不到」伪装成「拿到了」。
    """
    fields = getattr(headers, "fields", None)
    if fields is not None:
        try:
            return [[_as_text(k), _as_text(v)] for (k, v) in fields]
        except Exception:  # noqa: BLE001 - 兜底不抛，退到下一种取法
            pass
    items = getattr(headers, "items", None)
    if callable(items):
        try:
            return [[_as_text(k), _as_text(v)] for (k, v) in items(multi=True)]
        except Exception:  # noqa: BLE001
            pass
    try:
        return [[_as_text(k), _as_text(v)] for (k, v) in headers.items()]
    except Exception:  # noqa: BLE001
        return []


def _as_text(value: Any) -> str:
    """把 bytes / str 统一成 str（header 名在 mitmproxy 里是 bytes）。"""
    if isinstance(value, (bytes, bytearray)):
        return bytes(value).decode("latin-1", errors="replace")
    return str(value)


def _casing_preserved(raw_fields: List[List[str]]) -> bool:
    """判断原始字段名里**是否至少有一个非全小写**（即大小写未被抹平）。

    - 全小写 ⇒ 要么客户端本来就发小写，要么链路已小写化 ⇒ 无法据此断言「保持」；
    - 存在大写字母 ⇒ 证明**至少这一跳**保留了大小写。
    """
    for name, _ in raw_fields:
        if any(ch.isupper() for ch in name):
            return True
    return False


# ---------------------------------------------------------------------------
# addon
# ---------------------------------------------------------------------------


class P12MitmAddon:
    """按域过滤 + 记录 header 原始大小写 + 记录凭据替换结果。"""

    def __init__(self) -> None:
        self.domains: List[str] = _parse_domains(_env("P12_MITM_DOMAINS"))
        self.log_path: Path = Path(_env("P12_MITM_LOG", str(_HERE / "p12-mitm.log")))
        self.capture_dir: Path = Path(
            _env("P12_MITM_CAPTURE_DIR", str(_HERE / "p12-capture"))
        )
        self.replace_auth: str = _env("P12_MITM_REPLACE_AUTH")
        self.allow_body: bool = _env("P12_MITM_ALLOW_BODY") == "1"
        # 用 flow.id 关联 request/response 两次回调。
        self._pending: Dict[str, Dict[str, Any]] = {}

        self.log_path.parent.mkdir(parents=True, exist_ok=True)
        self.capture_dir.mkdir(parents=True, exist_ok=True)

    # -- 日志 ---------------------------------------------------------------

    def _log(self, line: str) -> None:
        """写一行带时间戳的文本日志（人读），并回显到 mitmproxy 控制台。"""
        rec = f"[{time.strftime('%Y-%m-%dT%H:%M:%S')}] {line}"
        with self.log_path.open("a", encoding="utf-8") as fh:
            fh.write(rec + "\n")
        ctx.log.info(rec)

    def _capture(self, record: Dict[str, Any]) -> None:
        """追加一条 JSONL 结构化记录（机读；**默认无正文**）。"""
        path = self.capture_dir / "flows.jsonl"
        with path.open("a", encoding="utf-8") as fh:
            fh.write(json.dumps(record, ensure_ascii=False) + "\n")

    # -- mitmproxy 钩子 -----------------------------------------------------

    def load(self, loader: Any) -> None:  # noqa: ARG002 - mitmproxy 约定签名
        if not self.domains:
            ctx.log.warn(
                "P12: P12_MITM_DOMAINS 为空 —— 本 addon 将**不处理任何域**（安全默认）。"
                "请在环境变量里给出业务域白名单后再起。"
            )
        else:
            ctx.log.info(f"P12: 仅处理这些域：{', '.join(self.domains)}")

    def request(self, flow: http.HTTPFlow) -> None:
        host = flow.request.pretty_host
        if not _host_matches(host, self.domains):
            return  # 白名单外：不记录、不改写、原样放过

        raw_fields = _raw_header_fields(flow.request.headers)
        preserved = _casing_preserved(raw_fields)
        header_names = [name for name, _ in raw_fields]

        # H4：把**原始大小写**的 header 名列出来（同名不同大小写会各出现一次）。
        self._log(
            f"REQ {flow.request.method} {host}{flow.request.path} "
            f"casing_preserved={preserved}"
        )
        self._log(f"    client_header_names(raw)={header_names}")

        record: Dict[str, Any] = {
            "kind": "request",
            "ts": time.time(),
            "method": flow.request.method,
            "host": host,
            "scheme": flow.request.scheme,
            "path": flow.request.path,
            "http_version": flow.request.http_version,
            "client_header_names_raw": header_names,
            "client_header_casing_preserved": preserved,
        }
        if self.allow_body and flow.request.content is not None:
            # ⚠️ 仅在显式开启时记录正文（可能含账号身份）。
            record["request_body_len"] = len(flow.request.content)
            record["request_body"] = flow.request.get_text(strict=False)

        # H3：凭据替换（仅在显式配置了替换值时）。
        if self.replace_auth:
            before = flow.request.headers.get("authorization", "")
            flow.request.headers["Authorization"] = self.replace_auth
            record["auth_replaced"] = True
            record["auth_before_len"] = len(before)
            self._log(
                f"    AUTH 已替换（原长度 {len(before)} -> 新长度 "
                f"{len(self.replace_auth)}）"
            )

        self._pending[flow.id] = record

    def response(self, flow: http.HTTPFlow) -> None:
        host = flow.request.pretty_host
        if not _host_matches(host, self.domains):
            return

        record = self._pending.pop(flow.id, None)
        status = flow.response.status_code if flow.response else None
        # H2 现象：TLS 握手失败在 mitmproxy 里表现为 error（客户端报证书错误）。
        error = getattr(flow, "error", None)

        self._log(f"RES {host}{flow.request.path} status={status} error={error}")

        if record is None:
            record = {
                "kind": "response-only",
                "ts": time.time(),
                "method": flow.request.method,
                "host": host,
                "path": flow.request.path,
            }
        record["kind"] = "flow"
        record["status"] = status
        record["error"] = error
        record["auth_replaced"] = bool(self.replace_auth)
        if self.allow_body and flow.response is not None and flow.response.content:
            record["response_body_len"] = len(flow.response.content)
            record["response_body"] = flow.response.get_text(strict=False)
        self._capture(record)

    def error(self, flow: http.HTTPFlow) -> None:
        """握手 / 连接错误 —— **H2 的关键信号**（证书固定会让这里报错）。"""
        host = flow.request.pretty_host if flow.request else "?"
        if not _host_matches(host, self.domains):
            return
        err = getattr(flow, "error", None)
        self._log(f"ERR  {host}{flow.request.path if flow.request else ''} error={err}")
        self._capture(
            {
                "kind": "error",
                "ts": time.time(),
                "host": host,
                "path": flow.request.path if flow.request else "",
                "error": err,
            }
        )


addons = [P12MitmAddon()]
