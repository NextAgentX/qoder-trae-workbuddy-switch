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
  "product.trae": "TraeWork",
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

  // ---- 通用设置（应用级设置弹层）----
  "appSettings.title": "通用设置",
  "appSettings.description": "不区分 WorkBuddy 与 TraeWork，对 Buddy Switch 本身全局生效。",
  "appSettings.entry": "通用设置",

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

  "appSettings.startup.title": "启动设置",
  "appSettings.startup.silent.label": "开机时静默启动到托盘",
  "appSettings.startup.silent.description": "开关直接反映系统登录项状态；之后可从托盘「打开主界面」恢复",
  "appSettings.startup.enabled": "已开启开机自启",
  "appSettings.startup.disabled": "已关闭开机自启",

  "appSettings.update.title": "自动更新",
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

  // ---- 设置页子导航（P7 §3.9：9 项，第 9 项「备份与恢复」）----
  // 应用级导航，与 appSettings 同域（§5：应用级放 shell，备份功能放 backup）。
  "settings.subnav.aria": "设置分区",
  "settings.subnav.general": "通用",
  "settings.subnav.account": "账号与登录",
  "settings.subnav.checkin": "签到与积分",
  "settings.subnav.gateway": "网关",
  "settings.subnav.schedule": "计划任务",
  "settings.subnav.client": "客户端",
  "settings.subnav.diagnostics": "权限与诊断",
  "settings.subnav.logs": "日志",
  "settings.subnav.backup": "备份与恢复",

  // ---- 网关工作台子导航（PLAN §3.9：6 项，两个产品线共用同一份表）----
  "gateway.subnav.aria": "网关分区",
  "gateway.subnav.overview": "概览",
  "gateway.subnav.keys": "密钥",
  "gateway.subnav.models": "模型",
  "gateway.subnav.pool": "账号池",
  "gateway.subnav.proxy": "请求代理",
  "gateway.subnav.logs": "请求日志",

  // ---- 设置页「日志」分区（WorkBuddy 无独立运行日志，给空态 + 出口）----
  "settings.logs.emptyTitle": "本分区暂无运行日志",
  "settings.logs.emptyBody": "WorkBuddy 分区的运行日志在网关页可见；这里是设置与备份入口。",
  "settings.logs.emptyAction": "查看网关设置",

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
