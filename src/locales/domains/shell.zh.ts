/**
 * 文案域：**应用外壳**（侧栏 / 产品切换 / 通用设置弹层 / 通用兜底）。
 *
 * 域划分见 `src/locales/zh.ts` 的组合入口。改动本文件请遵守那里的三条约定。
 */
export const zh = {
  // ---- 应用外壳 / 侧栏 ----
  "app.name": "Buddy Switch",
  "app.demoBadge": "演示模式",
  "product.workbuddy": "WorkBuddy",
  /* ★ issue #44：`TraeWork` 是**已被官方合并掉**的老程序线名（客户端自 2026-10-13 起只剩
     每区一个 `Trae`）。侧栏原来仍写 `TraeWork` ⇒ 用户看到的名字与实际在用的客户端对不上。
     徽标 / 文案一律用合并后的 `Trae`；老程序线的名字只在程序位按钮与迁移说明里出现。 */
  "product.trae": "Trae",
  /* Qoder 是商标，两种语言下写法相同，但仍走词表（与上面两条同款）。 */
  "product.qoder": "Qoder",
  "product.switchAria": "切换产品",
  "product.navAria": "{product} 导航",

  "nav.overview": "概览",
  /* ⚠️ 导航文案对齐原型 `DESTINATIONS`（`buddy-switch-prototype.html:1139-1146`）：
     原型是**短词**「账号」「网关」—— 不是「账号管理」「API 服务」（后者会把 240px 侧栏占满）。 */
  "nav.accounts": "账号",
  "nav.usage": "用量",
  "nav.tokenStats": "Token 统计",
  "nav.credits": "积分统计",
  "nav.apiService": "网关",
  "nav.settings": "设置",

  "sidebar.version": "版本",
  // 品牌块副标题（原型 `.brand-text i`，`buddy-switch-prototype.html:163`）。
  "sidebar.brand.tagline": "多平台账号管理",
  "sidebar.running": "{product} 运行中",
  /* 侧栏折叠（只显示图标）—— 2026-10-22 加。
     `collapseHint` / `expandHint` 是**带快捷键的提示**（tooltip 用），
     与按钮上的短文案分开：按钮空间小放短文案，hover 才给完整说法。 */
  "sidebar.collapse": "收起侧栏",
  "sidebar.expand": "展开侧栏",
  "sidebar.collapseHint": "收起侧栏，只显示图标（⌘/Ctrl + B）",
  "sidebar.expandHint": "展开侧栏，显示全部文字（⌘/Ctrl + B）",
  "sidebar.notRunning": "{product} 未运行",
  "sidebar.update": "更新",

  // ---- 顶部应用栏（redesign L1 作用域 + 全局动作）----
  "topbar.scope.aria": "切换作用域",
  "topbar.scope.menuLabel": "作用域（平台 × 区域）",
  "topbar.scope.sub.loggedIn": "{name} · {n} 个账号",
  "topbar.scope.sub.loggedOut": "未登录 · {n} 个账号",
  "topbar.scope.sub.missing": "未检测到 · {n} 个账号",
  // 有需处理账号时缀在副标题后（原型 `scope-menu`）—— 切之前就能看出哪个作用域有事要办。
  "topbar.scope.sub.attention": " · {n} 个需处理",
  // 作用域菜单底部说明行（原型 `scope-menu` 最后一行）：一句话讲清切换的影响面。
  "topbar.scope.hint": "作用域决定「我现在管的是哪一套账号」。切换后，侧栏、内容区、统计口径全部跟随。",
  "topbar.run.running": "运行中",
  "topbar.run.stopped": "未运行",
  "topbar.run.missing": "未检测到",
  "topbar.search": "搜索账号、页面或动作",
  "topbar.refresh": "刷新当前作用域",
  // 刷新完成的 toast（原型 `doRefresh` 的成功提示）。文案从实：顶栏刷新重读的是
  // 账号列表 + 运行状态（原型文案提「积分」但本项目顶栏刷新不含积分口径）。
  "topbar.refreshDone": "已刷新",
  "topbar.refreshDoneDesc": "{scope} 的账号与状态已更新",
  "topbar.refreshFailed": "刷新失败",
  "topbar.theme": "切换明暗主题",
  "topbar.appMenu": "应用设置",

  // ---- 页头作用域副标题（redesign §3.2：页头统一「作用域 · 说明」+ 右侧 scope-chip）----
  // ⚠️ 只读展示：切作用域的唯一入口在顶栏，页内不放区域切换器（区域已并入 L1 作用域）。
  "pageHead.overview.signedIn": "{scope} · 当前登录 {name}",
  "pageHead.overview.signedOut": "{scope} · 未登录",
  "pageHead.accounts": "{scope} · {n} 个账号",
  "pageHead.accounts.pending": "{scope} · {n} 个账号 · {m} 个需处理",
  "pageHead.usage": "{scope} · Token 与积分统计合并到一个工作台",
  "pageHead.gateway": "{scope} · 把账号池暴露为 OpenAI / Anthropic 兼容接口",
  "pageHead.settings": "{scope} · 按主题分区，不再是一屏长滚动",
  "pageHead.sessions": "{scope} · 同一份会话在各账号里的副本 —— 谁落后、谁分叉、同步给谁",

  // ---- 平台能力标签（概览「能力」卡；取值来自注册表 `Capability`）----
  "capability.checkin": "自动签到",
  "capability.travel": "自动旅行",
  "capability.credits": "积分统计",
  "capability.oauth": "OAuth 登录",
  "capability.gateway": "API 网关",
  "capability.autoRotate": "自动轮换",
  "capability.sessionCopy": "会话复制",

  // ---- 命令面板（⌘K，P6 §3.7）----
  "palette.entry": "搜索或跳转",
  "palette.title": "搜索或跳转",
  "palette.description": "跨作用域搜索账号、页面与动作，回车直达。",
  "palette.aria": "命令面板",
  "palette.placeholder": "搜索账号、页面或动作…",
  "palette.loading": "正在加载账号…",
  "palette.results": "{count} 个结果",
  "palette.empty.title": "没有匹配项",
  "palette.empty.description": "换个关键词试试，或清空搜索查看全部。",
  "palette.empty.clear": "清空搜索",
  "palette.group.page": "页面",
  "palette.group.scope": "作用域",
  "palette.group.account": "账号",
  "palette.group.action": "动作",
  "palette.scope.current": "当前",
  "palette.account.inScope": "在 {scope} 中",
  "palette.action.refresh": "刷新当前作用域",
  "palette.action.addAccount": "新增账号",
  "palette.action.openDataDir": "打开数据目录",

  // ---- 键盘快捷键设置（P6 §3.1）----
  "shortcuts.settings.title": "键盘快捷键",
  "shortcuts.settings.label": "启用作用域快捷键",
  "shortcuts.settings.description": "用 {mod} + 1…{n} 直接切换到对应作用域。",
  "shortcuts.settings.degraded": "当前作用域超过 {limit} 个，已停用数字快捷键。",

  // ---- 首次引导（P6 §1.5）----
  "firstRun.title": "快速上手",
  "firstRun.openPalette": "按 {shortcut} 打开命令面板，跨作用域搜索并跳转。",
  "firstRun.scopeShortcut": "按 {shortcut} 直接切换到对应作用域。",
  "firstRun.listSearch": "在账号页按 / 聚焦搜索框。",
  "firstRun.dismiss": "知道了",

  // ---- 撤销 toast（P6 T04 §1.4）----
  // ★ 删除是**软删除**（账号进回收站，可恢复）⇒ 文案写「可撤销」；**不写**「已永久删除」
  //   （那是 `recycleBin.purge` 的语义）。超时只是 toast 消失，账号**仍在回收站**，
  //   故文案**不得**暗示「过期即永久删除」（那会让用户以为没救了）。
  "undo.deletedTitle": "已删除，可撤销",
  "undo.batchTitle": "已删除 {n} 个账号，可撤销",
  "undo.action": "撤销",
  "undo.undone": "已恢复",
  "undo.failedTitle": "撤销失败",

  // ---- 应用级设置（设置页「通用」分区 + 顶栏 ⚙「应用设置」抽屉，同一份实现两种外壳）----
  "appSettings.drawer.more": "更多入口",

  "appSettings.appearance.title": "外观",
  "appSettings.appearance.theme.label": "主题",
  "appSettings.appearance.theme.description": "选择浅色、深色，或跟随系统外观自动切换",
  "appSettings.appearance.theme.aria": "主题",
  "appSettings.theme.system": "系统",
  "appSettings.theme.light": "浅色",
  "appSettings.theme.dark": "深色",

  "appSettings.language.title": "语言",
  "appSettings.language.label": "界面语言",
  "appSettings.language.description": "切换后立即生效；偏好保存在本机，不上传。",
  "appSettings.language.zh": "简体中文",
  "appSettings.language.en": "English",

  "appSettings.startup.title": "启动与托盘",
  "appSettings.startup.silent.label": "开机时静默启动到托盘",
  "appSettings.startup.silent.description": "开关直接反映系统登录项状态；之后可从托盘「打开主界面」恢复",
  "appSettings.startup.enabled": "已开启开机自启",
  "appSettings.startup.disabled": "已关闭开机自启",
  "appSettings.tray.closeToTray.label": "关闭窗口时保持后台运行",
  "appSettings.tray.closeToTray.description": "关闭主窗口后隐藏到托盘并继续运行；关掉此项则点关闭即退出应用",
  "appSettings.tray.saveMismatch": "设置未生效：后端回读值与请求值不一致，请重试",

  "appSettings.update.title": "自动更新",
  "appSettings.update.auto.label": "自动检查更新",
  "appSettings.update.auto.description": "每 30 分钟检查一次；关闭后仍可在下方手动检查",
  "appSettings.update.current": "当前版本：",
  "appSettings.update.source": "公开更新源",
  "appSettings.update.openRelease": "打开 GitHub Release",
  "appSettings.update.openReleaseTitle": "打开 GitHub Release",
  "appSettings.update.proxy.label": "更新代理地址",
  "appSettings.update.proxy.description": "仅用于 GitHub 更新检查和安装包下载；留空表示关闭显式代理。",
  "appSettings.update.proxy.placeholder": "例如 http://127.0.0.1:7897",
  "appSettings.update.proxy.save": "保存代理",
  "appSettings.update.proxy.saved": "更新代理已保存",
  "appSettings.update.proxy.cleared": "已关闭更新代理",
  "appSettings.update.proxy.invalid": "代理地址格式不正确，请填写 HTTP/HTTPS 地址，例如 http://127.0.0.1:7897",
  "appSettings.update.check": "检查更新",
  "appSettings.update.checkFailed": "检查失败",
  "appSettings.update.foundTitle": "发现新版本",
  "appSettings.update.doneTitle": "更新检查完成",
  "appSettings.update.foundDetail": "发现新版本 v{latest}（当前 v{current}）",
  "appSettings.update.upToDate": "已是最新版本 v{current}",
  "appSettings.update.installNow": "立即升级",
  /* 设置页的入口改成「先看说明」：点开的是说明弹窗，不是直接下载（见 UPDATE-NOTES-ARCH.md）。 */
  "appSettings.update.viewNotes": "查看更新内容",

  // ---- 设置页子导航（P7 §3.9：9 项，第 9 项「备份与恢复」）----
  // 应用级导航，与 appSettings 同域（§5：应用级放 shell，备份功能放 backup）。
  "settings.subnav.aria": "设置分区",
  "settings.subnav.general": "通用",
  "settings.subnav.account": "账号与登录",
  "settings.subnav.checkin": "签到与积分",
  "settings.subnav.gateway": "网关",
  "settings.subnav.schedule": "计划任务",
  "settings.subnav.client": "客户端",
  "settings.subnav.cli": "CodeBuddy CLI",

  /* ── CodeBuddy CLI 接入方式（issue #109）──
     这块是「两套凭据通道二选一」，不是偏好开关：两条路都写同一份
     `~/.codebuddy/settings.json`，混用 ⇒ CLI 把上游 token 发到网关 ⇒ 401，
     而轮换每轮又重写一次 ⇒ 时好时坏。文案口径见 PR #117 的设计稿。 */
  "wbSettings.cli.stripDirect": "直连上游 · 由本工具按积分到期选号",
  "wbSettings.cli.stripGateway": "走本机网关 · 由网关按模型选号",
  "wbSettings.cli.stripDirectSub": "上游 token 写在 ~/.codebuddy/settings.json，由本工具维护",
  "wbSettings.cli.stripGatewaySub": "CLI 的请求打到 {url}，凭据是一把专用网关 Key",
  "wbSettings.cli.effective": "已生效",
  "wbSettings.cli.pendingRestart": "待重启 CLI",
  "wbSettings.cli.pendingRestartTip": "env 不热重载：重启终端与 CodeBuddy CLI 后才生效",
  "wbSettings.cli.sectionTitle": "接入方式",
  "wbSettings.cli.sectionHint": "两者互斥，切换会重写 settings.json",
  "wbSettings.cli.mode.direct": "直连上游",
  "wbSettings.cli.mode.directTagline": "本工具管理凭据，并按积分到期选号",
  "wbSettings.cli.mode.gateway": "走本机网关",
  "wbSettings.cli.mode.gatewayTagline": "CLI 指向本机网关，由网关按模型选号",
  "wbSettings.cli.cap.directAccount": "按账号切换（积分到期优先）",
  "wbSettings.cli.cap.directRotate": "自动轮换 / 到期提醒",
  "wbSettings.cli.cap.modelSwitch": "某个模型限流时自动换号",
  "wbSettings.cli.cap.usage": "按模型统计用量与成本",
  "wbSettings.cli.cap.gatewayAccount": "按账号指定",
  "wbSettings.cli.current": "当前",
  "wbSettings.cli.enabled": "已启用",
  "wbSettings.cli.switchTo": "切换到此模式",
  "wbSettings.cli.recommended": "推荐",
  "wbSettings.cli.advanced": "高级",
  "wbSettings.cli.advancedHint": "默认模型 · CLI 专用 Key",
  "wbSettings.cli.modelLabel": "默认模型",
  "wbSettings.cli.modelDesc": "留空 = 由 CLI 自己决定；固定模型会让「按模型选号」失去一半意义",
  "wbSettings.cli.modelBlank": "留空 · 由 CLI 自己决定",
  "wbSettings.cli.keyLabel": "CLI 专用 Key",
  "wbSettings.cli.keyDescGateway": "切换时自动创建，网关页可吊销",
  "wbSettings.cli.keyDescDirect": "切到网关模式时才会创建",
  "wbSettings.cli.rotateLabel": "自动轮换",
  "wbSettings.cli.rotateDescGateway": "选号已交给网关；轮换每轮都会把网关 Key 覆盖回去，故已关闭",
  "wbSettings.cli.rotateOff": "已关闭",
  "wbSettings.cli.conflictTitle": "检测到 settings 已指向本机网关（{url}）",
  "wbSettings.cli.conflictBody": "「直连上游」暂时不可选：再写上游 token，CLI 会把 token 发到网关 ⇒ 401，而且轮换每轮重写一次 ⇒ 时好时坏。这不是错误，是护栏。",
  "wbSettings.cli.conflictAction": "接管网关配置",
  "wbSettings.cli.wizardGatewayTitle": "切到「走本机网关」？",
  "wbSettings.cli.wizardDirectTitle": "切回「直连上游」？",
  "wbSettings.cli.wizardIntro": "会改写 ~/.codebuddy/settings.json：",
  "wbSettings.cli.wizardGatewayCost1": "写入网关地址与一把新 Key（列表里叫「CodeBuddy CLI（网关）」）",
  "wbSettings.cli.wizardGatewayCost2": "清掉现有的上游 token 与 apiKeyHelper",
  "wbSettings.cli.wizardGatewayCost3": "关闭自动轮换（选号改由网关负责）",
  "wbSettings.cli.wizardDirectCost1": "删掉网关地址、CLI 专用 Key 与默认模型",
  "wbSettings.cli.wizardDirectCost2": "按当前账号重新写入上游 token（或恢复 apiKeyHelper）",
  "wbSettings.cli.wizardDirectCost3": "自动轮换保持你上次的设置（不会自动开启）",
  "wbSettings.cli.wizardRestart": "改完需要重启终端与 CodeBuddy CLI 才生效（环境变量不热重载）。",
  "wbSettings.cli.wizardTokenKept": "上游 token 只是不再写进 settings，账号库里仍有 —— 想切回来点「直连上游」即可，不需要重新登录。",
  "wbSettings.cli.wizardCancel": "取消",
  "wbSettings.cli.wizardConfirm": "确认切换",
  "wbSettings.cli.toastGateway": "已切到本机网关",
  "wbSettings.cli.toastGatewayDesc": "已创建 CLI 专用 Key 并关闭自动轮换；重启终端与 CLI 后生效",
  "wbSettings.cli.toastDirect": "已切回直连上游",
  "wbSettings.cli.toastDirectDesc": "网关配置已清理，直连凭据已重建；重启终端与 CLI 后生效",
  "wbSettings.cli.toastFailed": "切换失败",
  "wbSettings.cli.loadFailed": "读取 CLI 状态失败",
  "settings.subnav.diagnostics": "权限与诊断",
  "settings.subnav.logs": "日志",
  "settings.subnav.backup": "备份与恢复",

  // ---- 网关工作台子导航（PLAN §3.9 / §3.20：7 项，两个产品线共用同一份表；Trae 侧去掉「请求代理」）----
  "gateway.subnav.aria": "网关分区",
  "gateway.subnav.overview": "概览",
  "gateway.subnav.keys": "密钥",
  "gateway.subnav.models": "模型",
  "gateway.subnav.pool": "账号池",
  "gateway.subnav.proxy": "请求代理",
  "gateway.subnav.logs": "请求日志",

  // ---- 统一网关（第三个产品线 · 跨作用域）----
  "gateway.subnav.unified": "统一网关",
  /* ★ 网关页的**两个类型**（2026-10-15，对齐原型 `GW_TYPES` / `.gw-type-seg`）。
     类型与分区是**两个正交的轴**：类型回答「哪个网关」，子导航回答「这个网关的哪个分区」。
     ⚠️ 「统一网关」既是类型名也是旧子导航项名 —— 类型化之后**子导航里不再有它**。 */
  "gateway.type.label": "网关类型",
  "gateway.type.independent": "独立网关",
  "gateway.type.unified": "统一网关",
  /* 统一网关类型的 4 个分区（id 前缀 `u-`，与独立网关的 6 个不相交）。 */
  "unifiedGateway.subnav.overview": "概览",
  "unifiedGateway.subnav.namespaces": "命名空间",
  "unifiedGateway.subnav.keys": "密钥",
  "unifiedGateway.subnav.pool": "账号池",
  /* 密钥分区**共用**工具条（`KeysBar`）—— 两个类型同一形态。 */
  "gateway.keysBar.total": "共 {count} 把",
  "gateway.keysBar.hint": "{live} 把启用 · 明文只在创建时展示一次",
  /* 概览的「一句话结论」徽标（四态 + 各自的原因）。判据顺序 = 启用 → 池 → 清单。 */
  "unifiedGateway.verdict.ready": "可用",
  "unifiedGateway.verdict.partial": "部分不可用 · {count} 段",
  "unifiedGateway.verdict.dead": "全部不可用",
  "unifiedGateway.verdict.disabled": "未启用",
  "unifiedGateway.verdict.noNamespace": "没有启用任何命名空间",
  "unifiedGateway.verdict.readyHint": "{count} 个命名空间背后都有可用账号。",
  "unifiedGateway.verdict.partialHint": "这些命名空间背后没有可用账号：{list}",
  "unifiedGateway.verdict.deadHint": "已启用的命名空间背后都没有可用账号。",
  "unifiedGateway.verdict.disabledHint": "统一网关已关闭，端口上没有监听。下面的配置会照常保存，启用后生效。",
  "unifiedGateway.verdict.noNamespaceHint": "至少勾一个命名空间，否则所有调用都会返回 400。",
  /* 中性态：配置已启用、池数据还在路上 —— **不猜**「可用」，也不谎报「不可用」。 */
  "unifiedGateway.verdict.enabled": "已启用",
  "unifiedGateway.verdict.enabledHint": "配置已启用；池数据还在读取中，稍后给出可用性结论。",
  /* 两个类型的**分工**（放在概览顶部，避免用户在两个入口之间反复试）。 */
  "unifiedGateway.whichGateway": "只用一个平台 ⇒ 用「独立网关」（跟着顶栏的作用域走）；一个 provider 用遍全部平台与版本 ⇒ 用这个统一网关。",
  /* ── 接入说明卡（与独立网关的接入说明同形）── */
  "unifiedGateway.guideTitle": "接入说明",
  "unifiedGateway.guideOpenai": "OpenAI 兼容",
  "unifiedGateway.guideAnthropic": "Anthropic 兼容",
  "unifiedGateway.guideBase": "Base URL：OpenAI 系 {openai} · Anthropic 系 {anthropic}（不带 /v1，客户端会自己拼）。",
  "unifiedGateway.guideModels": "模型名必须带命名空间前缀（如 {sample}）—— 从 /v1/models 自动发现拿到的就是这种形式。",
  "unifiedGateway.guideResponses": "/v1/responses 三条产品线都支持（Trae / Qoder 侧由网关翻译成 chat）。",
  "unifiedGateway.guideKey": "一把统一密钥可同时授权多个命名空间 —— 见「密钥」分区。",
  /* ── 裸模型名 + 错误响应样例（原型 UNI_ERRORS 的 4 条）──
     每一条都**带出路**：只说「不支持」会让人反复试错。
     ★ 2026-10-22：Trae 侧补齐 `/v1/responses`，原「501 endpoint_not_supported_for_namespace」
     那条示例已删除。 */
  "unifiedGateway.bareModel": "裸模型名",
  "unifiedGateway.bareModelHint": "不带前缀的模型名会被拒绝，并给出可用前缀清单",
  "unifiedGateway.showErrors": "看错误响应",
  "unifiedGateway.errorsTitle": "统一网关的错误响应",
  "unifiedGateway.errorsHint": "每一条错误都带出路 —— 只说「不支持」会让人反复试错。",
  "unifiedGateway.err.bareModel.title": "裸模型名被拒",
  "unifiedGateway.err.bareModel.body": "统一网关要求模型名带命名空间前缀。本网关启用的命名空间：wb-cn: / wb-global: / trae-cn: / trae-global: / qoder-cn: / qoder-global:。\n你填的是 deepseek-v4.1-flash，请改成 wb-cn:deepseek-v4.1-flash 这类形式。",
  "unifiedGateway.err.forbidden.title": "命名空间未授权",
  "unifiedGateway.err.forbidden.body": "该 API Key 未被授权使用命名空间 trae-global（它只授权了 wb-cn、wb-global）。\n请到「网关 → 统一网关 → 密钥」为该 Key 勾选，或另建一把。",
  "unifiedGateway.err.notEnabled.title": "命名空间未启用",
  "unifiedGateway.err.notEnabled.body": "命名空间 traework-cn 未在本网关启用（「命名空间」分区里没有勾选它）。",
  "unifiedGateway.err.oldPort.title": "在旧端口填了命名空间",
  "unifiedGateway.err.oldPort.body": "模型名 wb-cn:deepseek-v4.1-flash 看起来是统一网关的命名空间前缀，\n但本端口（57891）不解析命名空间。请改用统一网关 http://127.0.0.1:57892/v1，\n或去掉前缀、改用 cn: / global: 前缀。",
  /* 未启用态的一行提示（关着开关时改配置，用户不确定有没有生效）。 */
  "unifiedGateway.notEnabled": "当前未启用：下面的配置会照常保存，启用后生效；端口上现在没有监听，任何调用都会连接失败。",
  /* 开关旁的**真实监听状态**（issue #52：开关开着 ≠ 端口上有人 accept）。 */
  "unifiedGateway.listening": "监听中",
  "unifiedGateway.notListening": "未监听",
  "unifiedGateway.listeningHint": "端口上已有监听，客户端可以直接连。",
  "unifiedGateway.notListeningHint": "配置已启用，但端口上没有监听 —— 把开关关掉再打开，或重启应用后重试。",
  "unifiedGateway.addr": "接入地址",
  "unifiedGateway.copyBaseUrl": "复制 Base URL",
  "unifiedGateway.loopback": "仅本机 127.0.0.1",
  "unifiedGateway.lan": "局域网 0.0.0.0",
  "unifiedGateway.allowLanTitle": "允许局域网访问？",
  "unifiedGateway.allowLanDesc": "改为 0.0.0.0 后，局域网内任何设备都能通过你的机器调用模型，直接消耗你的账号额度。默认只监听 127.0.0.1。",
  "unifiedGateway.confirmOpen": "确认开放",
  "unifiedGateway.confirm": "确认",
  "unifiedGateway.nsCount": "已启用 {count} / {total} 个",
  /* ── 账号池分区（跨平台只读汇总）──
     ★ 统一网关**没有自己的池**：它按命名空间前缀分派到三条产品线各自的池。
     所以这一区的措辞一律是「汇总 / 只读」，**不要**写成「管理」。 */
  "unifiedGateway.pool.empty": "没有启用任何命名空间",
  "unifiedGateway.pool.emptyHint": "池是按命名空间分派的 —— 先在「命名空间」分区里勾一个。",
  "unifiedGateway.pool.count": "{count} 个池",
  "unifiedGateway.pool.totals": "{available} 个可用账号 / 共 {total} 个",
  "unifiedGateway.pool.readOnly": "只读汇总 —— 选号 / 粘性 / 恢复在各自平台的网关页",
  "unifiedGateway.pool.account": "账号",
  "unifiedGateway.pool.credits": "积分",
  "unifiedGateway.pool.state": "状态",
  "unifiedGateway.pool.available": "{count} 可用",
  "unifiedGateway.pool.total": "共 {count} 个",
  "unifiedGateway.pool.cooling": "{count} 冷却中",
  "unifiedGateway.pool.disabled": "{count} 已禁用",
  "unifiedGateway.pool.expired": "{count} 已过期",
  "unifiedGateway.pool.zeroCredits": "{count} 零积分",
  "unifiedGateway.pool.modelCooling": "{count} 模型冷却",
  // 正交计数（已含在「{count} 可用」里）：账号是好的，只是这一瞬间并发占满。
  "unifiedGateway.pool.inFlightFull": "{count} 在途已满",
  "unifiedGateway.pool.inFlightFullHint":
    "这些账号状态正常，只是并发额度占满（在途请求还没结束）—— 几秒后自动恢复；此刻对它们发请求会得到 503 pool_busy。",
  "unifiedGateway.pool.noAccount": "池里一个账号都没有 —— 去该平台的账号页添加后才能调通。",
  "unifiedGateway.pool.note": "统一网关没有自己的账号池 —— 它按模型名的命名空间前缀分派到三条产品线各自的池：wb-* → WorkBuddy · trae-* / traework-* / traecode-* → Trae · qoder-* → Qoder。所以这里给的是跨平台的汇总视图；选号策略是按产品线设的，改它要回该平台自己的网关页。",
  /* 命名空间行的「账号池」列：把池的归属与规模写在段内（同一池被多段共用时数字相同）。 */
  "unifiedGateway.nsPoolOf": "共 {total} 个 · 池 {target}",
  "unifiedGateway.pool.show": "看池",
  "unifiedGateway.pool.hide": "收起池",
  /* 「可用性」四态 + 各自的原因（顺序即优先级，见组件里的 `healthOf`）。
     ★ 这才是「能不能调通」的判据 —— 「已识别」只是清单识别过，不等于可用。 */
  "unifiedGateway.health.ready": "可调用",
  "unifiedGateway.health.readyHint": "该段背后有可用账号，且已识别到模型。",
  "unifiedGateway.health.noAccount": "无账号",
  "unifiedGateway.health.noAccountHint": "上游池里一个账号都没有 —— 去该平台的账号页添加后才能调通。",
  "unifiedGateway.health.noAvailable": "无可用账号",
  "unifiedGateway.health.noAvailableHint": "池里有账号，但此刻没有一个可用（冷却 / 禁用 / 零积分）。",
  "unifiedGateway.health.noModels": "未识别模型",
  "unifiedGateway.health.noModelsHint": "还没识别到模型清单 —— 客户端从 /v1/models 拿不到东西。",
  "unifiedGateway.health.disabled": "已停用",
  "unifiedGateway.health.disabledHint": "本网关未启用该命名空间，调用会返回 400。",
  "unifiedGateway.title": "统一网关",
  "unifiedGateway.running": "运行中",
  "unifiedGateway.stopped": "已停用",
  "unifiedGateway.crossScope": "本网关跨全部作用域；上方的作用域选择器对它不生效。",
  "unifiedGateway.summary": "{namespaces} 个命名空间 · {keys} 把可用密钥 · 监听 {addr}",
  "unifiedGateway.enable": "启用统一网关",
  "unifiedGateway.enableHint": "带命名空间前缀的单一入口 —— 一个 provider 用遍全部平台与版本",
  "unifiedGateway.bindAddr": "监听地址",
  "unifiedGateway.bindLan": "改为局域网",
  "unifiedGateway.bindLoopback": "改回仅本机",
  "unifiedGateway.port": "端口",
  /* ★ 2026-10-17 订正：端口按 5789x 一段排开（界面 57890 · WB 57891 · 统一 57892 ·
     hook 57893 · Trae 57894 · Qoder 57895）。原文混着旧值 7864 / 7865，看不出规律。
     端口提示是**事实性**内容，漏一个就会让用户撞上去。 */
  "unifiedGateway.portHint": "改端口需重启。别撞上既有端口：界面 57890 · WorkBuddy 57891 · 统一 57892 · hook 57893 · Trae 57894 · Qoder 57895",
  "unifiedGateway.portInvalid": "端口不合法：请输入 1–65535 之间的整数",
  "unifiedGateway.nsTitle": "启用的命名空间",
  "unifiedGateway.nsHint": "未启用的：不解析、不列清单、调用返回 400。至少保留一个。",
  "unifiedGateway.showMore": "显示更多程序线",
  "unifiedGateway.showLess": "收起额外程序线",
  "unifiedGateway.keepOne": "至少保留一个命名空间",
  "unifiedGateway.nsOff": "本网关已停用该命名空间，调用会返回 400",
  "unifiedGateway.keysTitle": "统一密钥",
  "unifiedGateway.createKey": "创建密钥",
  "unifiedGateway.noKeys": "还没有统一密钥",
  "unifiedGateway.noKeysHint": "一把密钥可同时授权多个命名空间（如 WorkBuddy 国内 + Trae 国内）。",
  "unifiedGateway.colName": "名称",
  "unifiedGateway.colNamespaces": "授权命名空间",
  "unifiedGateway.colMasked": "密钥",
  "unifiedGateway.colCreated": "创建于",
  "unifiedGateway.colLastUsed": "最后使用",
  "unifiedGateway.colStatus": "状态",
  "unifiedGateway.colActions": "操作",
  "unifiedGateway.neverUsed": "从未使用",
  "unifiedGateway.revoked": "已吊销",
  "unifiedGateway.active": "启用",
  "unifiedGateway.restore": "恢复",
  "unifiedGateway.delete": "删除",
  "unifiedGateway.revoke": "吊销",
  "unifiedGateway.loadFailed": "读取统一网关配置失败",
  "unifiedGateway.saveFailed": "保存失败",
  "unifiedGateway.created": "密钥已创建",
  "unifiedGateway.createdHint": "明文只展示这一次（库中只存 sha256），关闭后无法再查看，请立刻复制。",
  "unifiedGateway.copy": "复制",
  "unifiedGateway.savedClose": "我已保存，关闭",
  "unifiedGateway.keyName": "名称",
  "unifiedGateway.keyNamePlaceholder": "例如：我的主力 Key",
  "unifiedGateway.authorize": "授权命名空间（至少一个）",
  "unifiedGateway.authorizeRequired": "不勾任何一项则无法创建。已停用的命名空间也可以先授权（启用后自动生效）。",
  "unifiedGateway.bizTitle": "业务转发命名空间",
  "unifiedGateway.bizNone": "不使用业务转发",
  "unifiedGateway.bizHint": "业务请求没有模型名，命名空间只能来自密钥本身；Trae 命名空间不合法。",
  "unifiedGateway.cancel": "取消",
  "unifiedGateway.create": "创建",
  "unifiedGateway.unnamed": "未命名",
  "unifiedGateway.previewTitle": "命名空间与清单预览",
  "unifiedGateway.previewTotal": "共 {count} 条模型",
  "unifiedGateway.previewEmpty": "没有启用任何命名空间 —— 在上方「启用的命名空间」里勾一个。",
  "unifiedGateway.previewCount": "{count} 条",
  "unifiedGateway.showModels": "看清单",
  "unifiedGateway.hideModels": "收起",
  "unifiedGateway.previewNoModels": "该段暂无清单（未识别到模型）",
  "unifiedGateway.source.live": "实时",
  "unifiedGateway.source.cached": "已保存",
  "unifiedGateway.source.builtin": "内置",

  // ---- 设置页「日志」分区 ----
  // ⚠️ 2026-10-14 实测更正：WorkBuddy **有**自己的运行日志目录
  //    （`~/.workbuddy/logs/`，按日分目录 + `automation.log`）。
  //    此前这里写「WorkBuddy 分区的运行日志在网关页可见」—— 网关页显示的是
  //    **本工具的请求日志**，与客户端的运行日志不是一回事，照着找会白费功夫。
  "settings.logs.emptyTitle": "本分区暂无运行日志",
  "settings.logs.emptyBody":
    "这里只有本工具的设置与备份入口；WorkBuddy 客户端自己的运行日志在其数据目录的 logs 子目录下（本工具暂未接入展示）。",
  "settings.logs.emptyAction": "查看网关设置",

  // ---- 设置页「日志」分区 · 运行日志卡片（runtime-log-card.tsx）----
  // ⚠️ 这份是**本工具自己**的日志（网关起不来 / 切号失败 / panic）。
  //    与「网关请求日志」（JSON、面向统计）和「客户端日志」是三件不同的事 —— 文案里必须分清。
  "settings.runtimeLog.groupTitle": "运行日志",
  "settings.runtimeLog.intro":
    "本工具自身做了什么、哪一步失败了都在这里（网关启动、账号切换、后台任务、崩溃）。保留 {days} 天，当前级别 {level}。客户端自己的日志不在本页。",
  "settings.runtimeLog.levelAll": "全部级别",
  "settings.runtimeLog.levelError": "错误",
  "settings.runtimeLog.levelWarn": "警告",
  "settings.runtimeLog.levelInfo": "信息",
  "settings.runtimeLog.levelDebug": "调试",
  "settings.runtimeLog.dateAll": "全部日期",
  "settings.runtimeLog.searchPlaceholder": "搜索来源或内容…",
  "settings.runtimeLog.search": "筛选",
  "settings.runtimeLog.reset": "重置",
  "settings.runtimeLog.autoRefresh": "自动刷新",
  "settings.runtimeLog.autoRefreshAria": "自动刷新运行日志",
  "settings.runtimeLog.refresh": "刷新",
  "settings.runtimeLog.copy": "复制",
  "settings.runtimeLog.openDir": "打开目录",
  "settings.runtimeLog.openDirFailed": "打开日志目录失败",
  "settings.runtimeLog.clear": "清空",
  "settings.runtimeLog.cleared": "已删除 {count} 个日志文件",
  "settings.runtimeLog.clearFailed": "清空运行日志失败",
  "settings.runtimeLog.clearConfirmTitle": "清空运行日志？",
  "settings.runtimeLog.clearConfirmBody":
    "将删除全部运行日志文件，不可恢复。下次启动后会重新开始记录。账号、配置与用量账本不受影响。",
  "settings.runtimeLog.clearConfirmAction": "确认清空",
  "settings.runtimeLog.cancel": "取消",
  "settings.runtimeLog.countSummary": "共 {total} 条",
  "settings.runtimeLog.truncated": "已截断至最近 {limit} 条",
  "settings.runtimeLog.emptyTitle": "还没有运行日志",
  "settings.runtimeLog.emptyBody":
    "本工具启动后会开始记录。若刚装好还没运行过任何操作，这里是空的属正常。",
  "settings.runtimeLog.emptyFilteredTitle": "当前筛选条件下没有匹配",
  "settings.runtimeLog.emptyFilteredBody": "日志文件里可能确实没有这些记录；重置筛选可看到全部。",
  "settings.runtimeLog.details": "落盘位置与上限",
  "settings.runtimeLog.dirLabel": "目录",
  "settings.runtimeLog.limitLabel": "上限",
  "settings.runtimeLog.limitValue": "保留 {days} 天 · 单文件 {size} · 每天最多 {parts} 个分片",

  // ---- 作用域可见性设置（app-settings.tsx）----
  "appSettings.scopeVisibility.groupTitle": "作用域显示",
  "appSettings.scopeVisibility.toggleAria": "{scope} 可见性开关",
  "appSettings.scopeVisibility.hint": "隐藏的作用域不再出现在顶栏菜单与快捷键中；URL 直达仍可达。",
  "appSettings.scopeVisibility.onlyOneLeft": "只剩一个作用域可见——不能再隐藏更多",

  // ---- 通用兜底 ----
  "common.unknownError": "未知错误",

  // ---- 顶层错误边界（渲染期异常兜底，见 components/error-boundary.tsx）----
  "app.error.title": "界面出错了",
  "app.error.description":
    "已捕获一个界面渲染错误。为避免显示错乱，这里停住了。请复制下面的详情反馈给我们；也可以先重新加载界面继续使用。",
  "app.error.reload": "重新加载界面",
  "app.error.copy": "复制错误详情",
  "app.error.copied": "已复制",
} as const;
