# Linux Wayland：创建密钥弹窗无响应时使用本机 WebUI

关联 [Issue #80](https://github.com/NextAgentX/qoder-trae-workbuddy-switch/issues/80)。在部分 Linux Wayland / WebKitGTK 桌面环境中，「网关 → 创建密钥」弹窗可能不显示。**这是不修改账号数据的备用使用方法，不代表桌面渲染问题已修复。**

## 从官方 Release 启动（无须编译）

1. 完全退出桌面版（包括托盘）。不要让桌面 App 和 CLI **同时读写同一份数据**。
2. 从 [GitHub Releases](https://github.com/NextAgentX/qoder-trae-workbuddy-switch/releases/latest) 下载 CLI 裸二进制：Linux x86_64 为 `buddy-switch-linux-x64`，arm64 为 `buddy-switch-linux-arm64`。不要把 `.AppImage` 当成 CLI。
3. 进入下载目录执行（x86_64 示例）：

~~~bash
chmod +x ./buddy-switch-linux-x64
BUDDY_SWITCH_HOME="$HOME" ./buddy-switch-linux-x64 serve \
  --host 127.0.0.1 --port 57890 --no-open
~~~

4. 在**同一台电脑**的浏览器访问 <http://127.0.0.1:57890>，进入「网关 → 统一网关 → 密钥」或「独立网关 → 密钥」，创建 Key。统一密钥需勾选至少一个授权命名空间，明文只出现一次，妥善本地保存。用 `Ctrl+C` 停止 CLI。

**关键数据目录**：`BUDDY_SWITCH_HOME` 应设为 `"$HOME"`（用户家目录），不要设为 `"$HOME/.buddy-switch"`。后者会把数据写到嵌套的 `~/.buddy-switch/.buddy-switch`，看起来像「所有账号/密钥都不见了」。

验证管理 API：

~~~bash
curl --noproxy '*' -fsS http://127.0.0.1:57890/api/status >/dev/null &&
  echo 'WebUI ready'
~~~

创建密钥不要求已安装上游客户端，但真正调用模型仍需要可用的账号凭据。

## 可选：systemd 用户服务，登录后自动启动

先把下载的裸二进制放到固定路径：

~~~bash
mkdir -p "$HOME/.local/bin" "$HOME/.config/systemd/user"
install -m 755 ./buddy-switch-linux-x64 "$HOME/.local/bin/buddy-switch-web"
~~~

新建 `~/.config/systemd/user/buddy-switch-web.service`：

~~~ini
[Unit]
Description=Buddy Switch local WebUI

[Service]
Type=simple
Environment=BUDDY_SWITCH_HOME=%h
ExecStart=%h/.local/bin/buddy-switch-web serve --host 127.0.0.1 --port 57890 --no-open
Restart=on-failure
RestartSec=3
NoNewPrivileges=true

[Install]
WantedBy=default.target
~~~

~~~bash
systemctl --user daemon-reload
systemctl --user enable --now buddy-switch-web.service
systemctl --user status buddy-switch-web.service
# 切回桌面端前请先停止：
systemctl --user stop buddy-switch-web.service
~~~

**安全**：本机 WebUI 没有登录验证，可以访问账号与密钥管理 API，务必仅绑定 `127.0.0.1`，不要对公网暴露 `57890`。`--host` 只控制 WebUI，不控制其他网关监听地址；如果看到统一网关监听在 `0.0.0.0:57892`，请在网关设置里收紧为 `127.0.0.1` 并关闭非回环监听。不要公开粘贴任何 token、API Key 或含凭据日志。默认端口 `57890` 为 WebUI、`57891` 为 WorkBuddy 网关、`57892` 为统一网关。

`Gtk-Message: Failed to load module...` 和 `libayatana-appindicator is deprecated` 仅凭这两种提示不能判断密钥创建失败。请区分桌面**弹窗不显示**与后台 **HTTP API 错误**。
