/**
 * 文案域：**WorkBuddy · 账号管理**（中文，键的权威之一）。
 *
 * 归属文件（这些文件的用户可见文案只能从这里取键，不要在本文件之外硬编码中文）：
 * `src/pages/AccountsPage.tsx`、`src/components/account-card.tsx`、
 * `src/components/switch-account-dialog.tsx`、`src/components/import-accounts-dialog.tsx`、
 * `src/components/export-accounts-dialog.tsx`、`src/components/oauth-login-dialog.tsx`
 *
 * 键前缀：`wbAccounts.`
 */
export const zh = {
  // ---- 页面：账号管理 ----
  // 与原型 `viewAccounts` 的 h1（`buddy-switch-prototype.html:2432`）一致：短标题「账号」。
  "wbAccounts.page.title": "账号",
  "wbAccounts.page.regionSwitchAria": "切换国内版 / 国际版",
  "wbAccounts.page.subtitle": "分别管理国内版与国际版 {product} 账号、积分和签到状态，两版账号库互相隔离。",
  "wbAccounts.page.addTitle": "添加与迁移账号",
  "wbAccounts.page.addSubtitle": "快速接入新账号，或从已有环境恢复",
  "wbAccounts.page.oauthAdd": "OAuth 扫码添加",
  "wbAccounts.page.exportTitle": "导出账号备份",
  "wbAccounts.page.loadFailed": "加载失败",
  "wbAccounts.page.accountsAria": "{count} 个账号",
  "wbAccounts.page.autoCheckin": "自动签到",
  "wbAccounts.page.autoCheckinSaving": "正在保存自动签到设置",
  "wbAccounts.page.autoTravel": "自动旅行",
  "wbAccounts.page.autoTravelSaving": "正在保存自动旅行设置",
  "wbAccounts.page.refreshCredits": "签到并刷新全部账号积分",
  "wbAccounts.page.statusLoggedIn": "已登录: {name}",
  "wbAccounts.page.cliTitle": "CodeBuddy CLI 接入",
  "wbAccounts.page.cliDescEnvOverride": "检测到进程环境变量 CODEBUDDY_AUTH_TOKEN。它会覆盖 settings.json；请先从 Windows 用户或系统环境变量中删除它，再重启本应用与 CodeBuddy CLI。",
  "wbAccounts.page.cliDescEnvSyncPending": "Windows CLI 认证配置与当前账号 Token 已脱节。点击更新认证后写入最新 Token；当前运行会话不会切换，请由 ACP 重新加载会话或重启 CLI 后生效。",
  "wbAccounts.page.cliDescEnvMigration": "检测到旧版 Windows helper 配置。接入后会改用 settings.json 的 env.CODEBUDDY_AUTH_TOKEN，不再执行 helper。",
  "wbAccounts.page.cliDescEnvDefault": "Windows 使用 CodeBuddy settings.json 中的认证 Token；切换或保活刷新后会自动更新。当前运行会话不会切换，请由 ACP 重新加载会话或重启 CLI 后生效。",
  "wbAccounts.page.cliDescHelperMigration": "检测到旧版 helper，请先升级；升级前不会将 CLI 切换显示为已验证。",
  "wbAccounts.page.cliDescHelperConfigured": "当前 helper 仍按旧索引读取账号；升级后将按账号 ID 独立切换，账号增删也不会错位。",
  "wbAccounts.page.cliDescDefault": "WorkBuddy 账号与积分功能可正常使用；如需从这里切换 CodeBuddy CLI 账号，点击下方按钮一键接入。",
  "wbAccounts.page.cliBtnUpdateAuth": "更新 CLI 认证",
  "wbAccounts.page.cliBtnConnect": "接入 CLI",
  "wbAccounts.page.cliBtnUpgradeHelper": "升级 CLI helper",

  // ---- 区域不匹配 ----
  "wbAccounts.page.mismatchTitle": "检测到凭据版本不匹配",
  "wbAccounts.page.mismatchBody1": "检测到 {region} 的登录凭据出现在 {version} 的认证文件位置（domain: {domain}）。已拒绝使用该凭据。",
  "wbAccounts.page.mismatchBody2": "请把 {env} 指向 {version} 登录态，或移除该文件。",
  "wbAccounts.page.mismatchDetail": "查看详情",
  "wbAccounts.page.mismatchDetailTitle": "凭据版本不匹配",
  "wbAccounts.page.mismatchDetailDesc": "为保证安全，该凭据不会被使用，也不会向任何上游端点发送。",
  "wbAccounts.page.mismatchActualDomain": "实际 domain",
  "wbAccounts.page.mismatchExpectedFile": "期望认证文件",
  "wbAccounts.page.mismatchEnvVar": "环境变量",

  // ---- 通用状态词（多组件复用） ----
  "wbAccounts.common.running": "运行中",
  "wbAccounts.common.notRunning": "未运行",
  "wbAccounts.common.connected": "已接入",
  "wbAccounts.common.notConnected": "未接入",
  "wbAccounts.common.needsUpgrade": "需升级",
  "wbAccounts.common.currentAccount": "当前账号: {name}",
  "wbAccounts.common.unknownAccount": "未知账号",
  "wbAccounts.common.notLoggedIn": "未登录",
  "wbAccounts.common.notDetected": "未检测到",
  "wbAccounts.common.notInstalled": "未安装",

  // ---- 空态（未安装/未登录） ----
  // ⚠️ 标题**必须**按 `installed` 分叉：客户端已安装但没登录态时说「未检测到」，
  // 会让用户以为「应用认为我没装」（2026-09-24 用户报障），且与页签上的
  // 「已登录/未登录」自相矛盾。
  "wbAccounts.empty.notDetected": "未检测到 WorkBuddy {version}",
  "wbAccounts.empty.notLoggedInTitle": "WorkBuddy {version} 尚未登录",
  "wbAccounts.empty.possibleReasons": "可能原因：",
  "wbAccounts.empty.reasonNotInstalled": "未安装 {version} WorkBuddy 桌面客户端",
  "wbAccounts.empty.reasonNoLogin": "已安装但从未登录过（未生成认证文件）",
  "wbAccounts.empty.reasonNoSession": "客户端已安装，但当前无可用登录态",
  "wbAccounts.empty.expectedAuthFile": "期望认证文件：",
  "wbAccounts.empty.copyPath": "复制路径",
  "wbAccounts.empty.copyPathToast": "路径已复制",
  "wbAccounts.empty.recheck": "重新检测",
  "wbAccounts.empty.importLocal": "从本机导入",

  // ---- 当前登录账号（尚未保存进账号库） ----
  // 账号库为空 ≠ 没有登录态：客户端可能已登录、只是用户还没点过「从本机导入」。
  // 这时必须把「当前登录的是谁」显示出来，而不是给一张「未检测到」空态卡片。
  "wbAccounts.currentLogin.title": "当前登录：{name}",
  "wbAccounts.currentLogin.unsaved": "该账号尚未保存到账号库，因此不出现在下方列表中。保存后即可在列表里管理（切换 / 签到 / 积分）。",
  "wbAccounts.currentLogin.save": "保存到账号库",

  // ---- 账号卡片 ----
  "wbAccounts.card.forever": "长期有效",
  "wbAccounts.card.expirySuffix": "到期",
  "wbAccounts.card.arrivingSoon": "即将到达",
  "wbAccounts.card.remainingHM": "剩余 {hours} 小时 {minutes} 分钟",
  "wbAccounts.card.remainingH": "剩余 {hours} 小时",
  "wbAccounts.card.remainingM": "剩余 {minutes} 分钟",
  "wbAccounts.card.traveling": "旅行中",
  "wbAccounts.card.finished": "已结束",
  "wbAccounts.card.noBuddy": "无 Buddy",
  "wbAccounts.card.notTraveled": "未旅行",
  "wbAccounts.card.travelEst": "预计 {points}",
  "wbAccounts.card.currentWorkbuddy": "WorkBuddy 当前账号",
  "wbAccounts.card.currentIde": "CodeBuddy IDE 当前账号",
  "wbAccounts.card.currentCli": "CodeBuddy CLI 当前账号",
  "wbAccounts.card.currentPlugin": "{client} 当前账号",
  // ⚠️ 这句讲的是**本机没装编辑器扩展**（不是「尚未支持」）⇒ 必须指向「去装」这个正确动作；
  //    写成「尚未支持」会把用户引向错误判断（那是 `planned` 的口径）。
  "wbAccounts.card.guidePlugin": "未检测到 {client}：请先在编辑器里安装该 CodeBuddy 扩展，装好后回到这里即可切换",
  // P5 §2.5：程序位展示名 —— core `modules/programs.rs` 的 `ProgramSpec.label_key` 指向这几条。
  // ⚠️ **显示名两两不同**（门禁钉住）：`vscodeCnInject` 是**只读**探测槽，
  //    `vscodeCodebuddyPlugin` 是**可切换**槽 —— 两者都跟 VS Code 有关，名字若相同，
  //    用户根本分不清该点哪个（改造前这两条就重名）。措辞与后端 `adapter.label()` 逐字一致。
  "wbAccounts.program.codebuddyCli": "CodeBuddy CLI",
  "wbAccounts.program.codebuddyCnIde": "CodeBuddy CN IDE",
  "wbAccounts.program.vscodeCnInject": "VS Code CodeBuddy 插件",
  "wbAccounts.program.vscodeCodebuddyPlugin": "VS Code CodeBuddy 扩展",
  "wbAccounts.program.jetbrainsCodebuddyPlugin": "JetBrains CodeBuddy 插件",
  // ⚠️ **计划新增（尚未支持）** 的兜底说明：当前**没有** planned 条目（两条目标均已落地），
  //    但机制保留 ⇒ 措辞只能是「尚未支持」，**不许**用「未检测到客户端」—— 后者会**引导用户去装，
  //    而装了也没用**（原型断言 `verify-behaviors.cjs:1297-1302` 反向禁止）。
  "wbAccounts.program.plannedHint": "尚未支持",
  "wbAccounts.card.unnamed": "未命名账号",
  "wbAccounts.card.checkedIn": "已签到",
  // 原型 `accountStatus()`（`buddy-switch-prototype.html:2390-2395`）是**互斥的一枚**状态：
  // 凭据已过期 → 需重新登录 → 今日未签到 → （否则）已签到。
  "wbAccounts.card.notCheckedIn": "今日未签到",
  "wbAccounts.card.needRelogin": "需重新登录",
  "wbAccounts.card.tokenExpired": "凭据已过期",
  // 「优先」——原型 `.acct-chips` 里是**带文字**的 `badge info`（`:2508`），
  // 此前我们只摆一枚星形图标，与原型不同。
  "wbAccounts.card.priority": "优先",
  "wbAccounts.card.priorityTip": "建议优先使用",
  // 原型 `.acct-chips` 第三条（`:2507`）：`{X} 积分 {N} 天后过期`。
  "wbAccounts.card.expiringChip": "{amount} 积分 {days} 天后过期",
  "wbAccounts.card.toolsInUse": "使用中 {n} 个工具",
  // 头部「当前登录」徽章（原型 `accountCard` :2502）：绑定任一程序位即为当前登录。
  "wbAccounts.card.currentLogin": "当前登录",
  // 卡片指标行（原型 `.acct-metrics` :2510）：`积分 1,240/2,000` · `今日 42.1K` · `2 分钟前`。
  // ⚠️ 三项都有真实来源：积分（credit.resources）、今日 Token（get_account_token_today）、
  //    最后使用（AccountMeta.lastUsedAt）。缺值仍按诚实边界渲染：缺席 ⇒ 「—」（非 0）。
  "wbAccounts.card.metric.credits": "积分",
  // 原型该格只有「今日」两字（`<span>今日 <b>…</b></span>`），不是「今日 Token」。
  "wbAccounts.card.metric.todayTokens": "今日",
  // 第三格原型是**无标签的相对时间**；标签与绝对时刻改挂 `title`。
  "wbAccounts.card.metric.lastUsedTip": "最后使用 {time}",
  // 「今日 Token」= 下界（网关日志已到保留上限）时的前缀；`{value}` 已含紧凑计数（如 42.1K）。
  "wbAccounts.card.metric.atLeast": "≥{value}",
  // 「今日 Token」明细（tooltip）：客户端自身 CLI 用量 / 经本网关的用量，各自 输入·输出。
  "wbAccounts.card.metric.todayTokensClient": "客户端 入 {input} · 出 {output}",
  "wbAccounts.card.metric.todayTokensGateway": "网关 入 {input} · 出 {output}",
  // 下界说明：`gatewayTruncated` 时**必须**带上，只给数字不说口径就是误导。
  "wbAccounts.card.metric.todayTokensTruncated": "网关日志已达保留上限，网关用量为下界（实际更多）",
  "wbAccounts.card.editGroup": "设置分组",
  "wbAccounts.card.manageAria": "管理账号 {name}",
  "wbAccounts.card.moreActions": "更多账号操作",
  "wbAccounts.card.refreshToken": "刷新 Token",
  "wbAccounts.card.manualCheckin": "手动签到",
  "wbAccounts.card.editRemark": "编辑备注",
  "wbAccounts.card.deleteAccount": "删除账号",
  "wbAccounts.card.setCurrentWorkbuddy": "设为 WorkBuddy 当前账号",
  "wbAccounts.card.setCurrentWorkbuddyRestart": "设为 WorkBuddy 当前账号（会重启 WorkBuddy）",
  "wbAccounts.card.switchIde": "切换到 CodeBuddy IDE",
  "wbAccounts.card.switchIdeRestart": "切换到 CodeBuddy IDE（会重启 IDE）",
  "wbAccounts.card.ideNotDetected": "未检测到 CodeBuddy IDE",
  "wbAccounts.card.switchingIde": "正在切换 CodeBuddy IDE",
  "wbAccounts.card.setCurrentCli": "设为 CodeBuddy CLI 当前账号",
  "wbAccounts.card.switchingCli": "正在切换 CodeBuddy CLI 当前账号",
  "wbAccounts.card.connectCliFirst": "请先接入 CodeBuddy CLI",
  // P4：备注编辑器与「查看详情」入口。
  // 备注编辑器已迁入**共享**抽屉，占位符 / 无障碍标签改由 `shared.drawer.remark.*` 提供；
  // 卡片只保留「更多 → 编辑备注」这个入口（打开抽屉并进入编辑态）。
  "wbAccounts.card.viewDetail": "查看详情",
  "wbAccounts.card.selectAria": "选择账号 {name}",
  // 未检测到的程序位：**点击引导到设置**（§3.3 规则 1），而不是摆一枚点不动的灰按钮。
  "wbAccounts.card.guideIdeSetting": "未检测到 CodeBuddy IDE，点击前往设置配置 IDE 路径",
  "wbAccounts.card.guideCliSetting": "未接入 CodeBuddy CLI，点击前往设置完成接入",
  "wbAccounts.card.loadingCredits": "积分查询中…",
  "wbAccounts.card.waitingCredits": "等待积分数据…",
  "wbAccounts.card.creditFailed": "积分查询失败",
  "wbAccounts.card.addPlaintextAccount": "用 OAuth 扫码添加",
  "wbAccounts.card.creditPackFallback": "积分包",
  "wbAccounts.card.setCurrent": "设为当前",
  "wbAccounts.card.switching": "切换中…",
  "wbAccounts.card.cliCurrent": "CLI 当前",
  // P4：积分包明细从卡片内 Dialog 迁入**共享抽屉**，以下键改由 `AccountsPage` 装配
  // `DetailPack` 时使用。
  // ⚠️ 2026-10-12：卡片上的「大数字行 + 近期到期」整块已按原型删除（原型卡片只有
  // `.acct-top` / `.acct-chips` / `.acct-metrics` / `.acct-actions` 四块），
  // 故 `creditPacks` / `expireSoon` / `nextExpiry` / `creditsForever` / `updated` /
  // `expiringSoonTitle` / `resourceTitle` / `creditsUnit` / `noCredits` 一并删除。
  "wbAccounts.card.expired": "已到期",
  // 「N 天内到期」的文案键已删：临期不再用文字区分，改为 ≤3 天时把日期行**标黄**
  // （见 `DetailPack.expiringSoon` 的装配口径，AccountsPage）。
  "wbAccounts.card.expiresOn": "到期 {date}",
  "wbAccounts.card.used": "已用 {n}",

  // ---- 详情抽屉的信息区块（由 AccountsPage 装配后注入）----
  "wbAccounts.detail.section.account": "账号信息",
  "wbAccounts.detail.email": "邮箱",
  "wbAccounts.detail.uid": "UID",
  "wbAccounts.detail.id": "账号 ID",
  "wbAccounts.detail.tokenExpiry": "Token 到期",
  "wbAccounts.detail.expired": "已过期",
  "wbAccounts.detail.checkedIn": "今日签到",

  // ---- 切换账号弹窗 ----
  "wbAccounts.dialog.switchTitle": "切换到「{name}」",
  "wbAccounts.dialog.switchDesc": "切换会关闭并重启 WorkBuddy，认证文件将写入目标账号。",
  "wbAccounts.dialog.switchProgress": "正在切换账号…",
  "wbAccounts.dialog.migrateProgress": "正在迁移账号数据…",
  /** 切换结果摘要里多条片段的分隔符（中文用全角分号，英文用逗号）。 */
  "wbAccounts.dialog.listSeparator": "；",
  "wbAccounts.dialog.switchProcessing": "正在处理中，请勿关闭窗口",
  "wbAccounts.dialog.copySessions": "复制会话到目标账号",
  "wbAccounts.dialog.copyHintLoading": "正在加载会话…",
  "wbAccounts.dialog.copyHintErrorEmpty": "无法加载会话列表，暂不能复制",
  "wbAccounts.dialog.copyHintEmptyCurrent": "「{region}」当前账号没有会话，无法复制；若会话在另一版本，请切换上方的「数据来源版本」",
  "wbAccounts.dialog.copyHintNoData": "「{region}」客户端数据目录里没有会话数据（客户端可能刚重装或重置过），无法复制",
  "wbAccounts.dialog.copyHintNoCurrent": "未检测到当前登录账号，无法列出会话",
  "wbAccounts.dialog.copyHintCross": "把 {from} 当前账号勾选的会话以新 id 复制到 {to} 目标账号",
  "wbAccounts.dialog.copyHintSame": "将当前账号勾选的会话以新 id 复制给目标账号（云端归属目标）",
  "wbAccounts.dialog.migrateMemory": "迁移长期记忆",
  "wbAccounts.dialog.migrateMemoryDescSame": "合并当前账号的长期记忆到目标账号；按内容去重，只补差集，改写前自动备份原文",
  "wbAccounts.dialog.migrateMemoryDescCross": "把 {from} 当前账号的长期记忆合并到 {to} 目标账号；按内容去重，只补差集，改写前自动备份原文",
  "wbAccounts.dialog.migrateConnectors": "迁移连接器配置",
  "wbAccounts.dialog.migrateConnectorsDescSame": "合并当前账号的连接器配置到目标账号；同名条目递归合并、重复项去重，改写前自动备份原文",
  "wbAccounts.dialog.migrateConnectorsDescCross": "把 {from} 当前账号的连接器配置合并到 {to} 目标账号；同名条目递归合并、重复项去重，改写前自动备份原文",
  "wbAccounts.dialog.loadingSessions": "加载会话…",
  "wbAccounts.dialog.noSessionsCurrent": "当前账号暂无会话",
  "wbAccounts.dialog.noSessionsNoCurrent": "未检测到当前登录账号，无法列出会话",
  "wbAccounts.dialog.selectKind": "选择 {label}",
  "wbAccounts.dialog.collapse": "折叠 {label}",
  "wbAccounts.dialog.expand": "展开 {label}",
  "wbAccounts.dialog.selectFolder": "选择文件夹 {label}",
  "wbAccounts.dialog.collapseFolder": "折叠文件夹 {label}",
  "wbAccounts.dialog.expandFolder": "展开文件夹 {label}",
  "wbAccounts.dialog.ungrouped": "未分组",
  "wbAccounts.dialog.hasBody": "有正文",
  "wbAccounts.dialog.permissionTitle": "如何授权（只需 3 步）：",
  "wbAccounts.dialog.permissionStep1": "点击下方「打开完全磁盘访问」",
  "wbAccounts.dialog.permissionStep2": "把 BuddySwitch.app 从 Finder 拖进面板列表（即使没提示框也直接拖），打开它的开关",
  "wbAccounts.dialog.permissionStep3": "授权后这里会自动检测到，无需其他操作",
  "wbAccounts.dialog.openFullDisk": "打开完全磁盘访问",
  "wbAccounts.dialog.showInFinder": "在 Finder 中显示",
  "wbAccounts.dialog.checkNow": "立即检测",
  "wbAccounts.dialog.cancel": "取消",
  "wbAccounts.dialog.continue": "继续",
  "wbAccounts.dialog.confirmSwitch": "确认切换",
  "wbAccounts.dialog.permissionChecking": "检测中…",
  "wbAccounts.dialog.permissionOk": "✓ 授权成功，可以重新切换了",
  "wbAccounts.dialog.permissionFailDir": "✗ {error}（{dir}）",
  "wbAccounts.dialog.permissionFail": "✗ {error}",
  "wbAccounts.dialog.migratedMemory": "迁移记忆 +{n} 行（去重 {d} 行）",
  "wbAccounts.dialog.migratedMemorySimple": "迁移记忆 +{n} 行",
  "wbAccounts.dialog.migratedConnectors": "迁移连接器 +{k} 键（去重 {d} 项）",
  "wbAccounts.dialog.migratedConnectorsSimple": "迁移连接器 +{k} 键",
  "wbAccounts.dialog.memoryFail": "记忆迁移失败（{error}）",
  "wbAccounts.dialog.connectorFail": "连接器迁移失败（{error}）",
  "wbAccounts.dialog.backup": "备份: {path}",
  "wbAccounts.dialog.switchedWarn": "已切换至「{name}」，但部分迁移未完成",
  "wbAccounts.dialog.switchedOk": "已切换至「{name}」",
  "wbAccounts.dialog.switchedDefault": "WorkBuddy 已重启为目标账号。",
  "wbAccounts.dialog.copiedSessions": "已复制 {n} 个会话",
  "wbAccounts.dialog.skippedSessions": "跳过 {n} 个已存在副本",

  // ---- 切换账号：跨作用域迁移向导（P4 · 抽屉内三步向导）----
  // ⚠️ 能力边界如实说（§1.3 / §3.5 红线）：跨产品（WB→Trae）会话复制**后端不支持** ⇒
  //    跨产品来源 disabled + 原因；**绝不**写「已打通 WB→Trae」这类假承诺。
  "wbAccounts.wizard.stepsAria": "迁移步骤",
  "wbAccounts.wizard.stepTarget": "目标与来源",
  "wbAccounts.wizard.stepContent": "迁移内容",
  "wbAccounts.wizard.stepConfirm": "确认",
  // 本向导只切换 WorkBuddy 桌面客户端 ⇒ 程序位是**单值**（只读展示，不做假选择器）。
  "wbAccounts.wizard.programLabel": "目标程序位",
  "wbAccounts.wizard.programWorkbuddy": "WorkBuddy 桌面",
  // 第 1 步：来源账号（只列各作用域的**当前账号**，§1.3 结论 1）。
  "wbAccounts.wizard.sourceTitle": "从哪里迁移内容？",
  "wbAccounts.wizard.sourceDesc": "默认是当前登录账号。",
  "wbAccounts.wizard.crossShow": "从其他作用域选择…",
  "wbAccounts.wizard.crossHide": "仅看当前作用域",
  "wbAccounts.wizard.sourceCurrent": "当前登录账号",
  "wbAccounts.wizard.sourceBadge": "来源",
  "wbAccounts.wizard.sourceUnsupported": "该平台不支持会话复制（会话复制依赖 WorkBuddy 的会话存储与云端归属）",
  "wbAccounts.wizard.sourceUnsupportedBadge": "不支持会话复制",
  // 第 2 步：迁移内容（会话 / 记忆 / 连接器平级开关）+ 三层会话树。
  "wbAccounts.wizard.contentTitle": "要一并迁移哪些内容？",
  "wbAccounts.wizard.treeTitle": "选择要复制的会话",
  "wbAccounts.wizard.treeCount": "已选 {n} / {m}",
  "wbAccounts.wizard.treeSelectAll": "全选",
  "wbAccounts.wizard.treeSelectNone": "全不选",
  "wbAccounts.wizard.typeConversation": "对话",
  "wbAccounts.wizard.typeWorkspace": "工作区",
  // 会话树空态的出口（该账号确实没有会话时，用户能做的下一步）。
  "wbAccounts.wizard.emptyExit": "跳过会话复制",
  // 第 3 步：确认页（如实汇总 + 影响 + 不可逆提示）。
  "wbAccounts.wizard.confirmTarget": "目标账号",
  "wbAccounts.wizard.confirmSource": "来源账号",
  "wbAccounts.wizard.confirmProgram": "程序位",
  "wbAccounts.wizard.confirmContent": "迁移内容",
  "wbAccounts.wizard.confirmSessions": "会话（已选 {n} 个会话 / {d} 个目录）",
  "wbAccounts.wizard.confirmNoSessions": "会话（未选择任何会话）",
  "wbAccounts.wizard.confirmMemory": "记忆",
  "wbAccounts.wizard.confirmConnectors": "连接器",
  "wbAccounts.wizard.confirmNone": "不迁移",
  "wbAccounts.wizard.contentSeparator": "、",
  "wbAccounts.wizard.impact": "影响",
  "wbAccounts.wizard.impactText": "客户端将被重启，未保存的编辑器内容可能丢失。",
  "wbAccounts.wizard.irreversible": "迁移会写入目标账号的数据，此操作不可逆。",
  // 导航。
  "wbAccounts.wizard.back": "上一步",
  "wbAccounts.wizard.next": "下一步",
  "wbAccounts.wizard.run": "执行切换",

  // ---- 导入账号弹窗 ----
  "wbAccounts.dialog.importTitle": "导入账号",
  "wbAccounts.dialog.importItemFallback": "第 {index} 项",
  "wbAccounts.dialog.importDesc": "选择 JSON 文件，勾选要导入的账号。",
  "wbAccounts.dialog.chooseFile": "选择文件",
  "wbAccounts.dialog.parsing": "正在解析…",
  "wbAccounts.dialog.importCount": "共 {n} 个账号，已选 {m} 个",
  "wbAccounts.dialog.selectAll": "全选",
  "wbAccounts.dialog.deselectAll": "取消全选",
  "wbAccounts.dialog.missingToken": "缺少 token",
  "wbAccounts.dialog.importing": "导入中…",
  "wbAccounts.dialog.importSelected": "导入勾选账号",

  // ---- 导出账号弹窗 ----
  "wbAccounts.dialog.exportTitle": "导出账号",
  "wbAccounts.dialog.exportDesc": "勾选要导出的账号，导出为 JSON 文件。",
  "wbAccounts.dialog.exportSecurityTitle": "安全提示",
  "wbAccounts.dialog.exportSecurity": "导出文件含登录 token，等同密码，请勿上传网盘或发送给他人。",
  "wbAccounts.dialog.exportSaveTitle": "导出账号",
  "wbAccounts.dialog.noAccounts": "暂无账号可导出。",
  "wbAccounts.dialog.exportCount": "共 {n} 个账号，已选 {m} 个",
  "wbAccounts.dialog.exportDone": "完成",
  "wbAccounts.dialog.exportSelected": "导出勾选账号",
  "wbAccounts.dialog.revealWindows": "在资源管理器中显示",
  "wbAccounts.dialog.revealLinux": "在文件管理器中显示",
  "wbAccounts.dialog.revealMac": "在 Finder 中显示",

  // ---- OAuth 扫码登录弹窗 ----
  "wbAccounts.dialog.oauthTitle": "OAuth 扫码登录",
  "wbAccounts.dialog.oauthDesc": "在浏览器中打开验证链接，扫码授权后将自动采集账号并入库。",
  "wbAccounts.dialog.oauthStart": "开始扫码登录",
  "wbAccounts.dialog.oauthStarting": "正在发起登录…",
  "wbAccounts.dialog.oauthWaiting": "正在等待扫码授权，请在浏览器完成操作…",
  "wbAccounts.dialog.oauthCollected": "已采集账号：{name}",
  "wbAccounts.dialog.oauthLoginFail": "登录失败",
  "wbAccounts.dialog.oauthClose": "关闭",
  "wbAccounts.dialog.oauthDone": "完成",

  // ---- 删除账号确认框 ----
  "wbAccounts.dialog.deleteTitle": "删除账号",
  "wbAccounts.dialog.deleteDesc": "确定删除账号「{name}」？删除后可在回收站恢复。",
  "wbAccounts.dialog.deleteConfirm": "删除",

  // ---- 接入/升级 CLI 确认框 ----
  "wbAccounts.dialog.installTitleUpdate": "更新 CodeBuddy CLI 认证",
  "wbAccounts.dialog.installTitleUpgrade": "升级 CodeBuddy CLI helper",
  "wbAccounts.dialog.installTitleConnect": "接入 CodeBuddy CLI",
  "wbAccounts.dialog.installDescSettingsA": "将把当前账号的认证 Token 写入",
  "wbAccounts.dialog.installDescSettingsB": "的 env.CODEBUDDY_AUTH_TOKEN。其他配置会保留；更新只影响后续加载的会话，当前运行会话不会切换。是否继续？",
  "wbAccounts.dialog.installDescOtherA": "{verb}会自动写入 ~/.codebuddy-rotate/helper.cjs 并更新 ~/.codebuddy/settings.json 的 apiKeyHelper 配置，是否继续？",
  "wbAccounts.dialog.installVerbConnect": "接入",
  "wbAccounts.dialog.installVerbUpgrade": "升级",

  // ---- Toast ----
  "wbAccounts.toast.autoCheckinLoadFail": "自动签到配置加载失败",
  "wbAccounts.toast.autoTravelLoadFail": "自动旅行配置加载失败",
  "wbAccounts.toast.imported": "账号已导入",
  "wbAccounts.toast.importFail": "导入失败",
  "wbAccounts.toast.autoCheckinSaveFail": "自动签到设置保存失败",
  "wbAccounts.toast.autoTravelOn": "自动旅行已开启",
  "wbAccounts.toast.autoTravelDispatching": "正在按官方状态派发或领取",
  "wbAccounts.toast.autoTravelSaveFail": "自动旅行设置保存失败",
  "wbAccounts.toast.exportSuccess": "导出成功",
  "wbAccounts.toast.exportDetail": "已导出 {count} 个账号。文件含登录 token，等同密码，请勿上传网盘或发送给他人。",
  "wbAccounts.toast.importSuccess": "导入成功",
  "wbAccounts.toast.importOverwrite": "（覆盖 {n} 个）",
  "wbAccounts.toast.importDetail": "已导入 {imported} 个{overwrite}，跳过 {skipped} 个。token 可能已过期，切换后可能需要重新登录。",
  "wbAccounts.toast.deleteFail": "删除失败",
  "wbAccounts.toast.checkinSuccess": "签到成功",
  "wbAccounts.toast.checkedInToday": "今天已签到",
  "wbAccounts.toast.checkinFail": "签到失败",
  "wbAccounts.toast.tokenRefreshFail": "Token 刷新失败",
  "wbAccounts.toast.tokenRefreshed": "Token 已刷新",
  "wbAccounts.toast.needRelogin": "需重新登录",
  "wbAccounts.toast.checkinBatchSuccess": "{n} 个签到成功",
  "wbAccounts.toast.checkinBatchAlready": "{n} 个已签到",
  "wbAccounts.toast.checkinBatchFailed": "{n} 个失败",
  "wbAccounts.toast.checkinRetryFailed": "只重试失败项",
  "wbAccounts.toast.checkinNone": "无账号需要签到",
  "wbAccounts.toast.checkinFailBatch": "签到失败",
  "wbAccounts.toast.checkinDone": "签到完成",
  "wbAccounts.toast.batchCheckinFail": "批量签到失败",
  "wbAccounts.toast.creditsRefreshed": "积分到期情况已刷新",
  "wbAccounts.toast.switchCliLoading": "正在切换 CodeBuddy CLI…",
  "wbAccounts.toast.switchCliLoadingDesc": "正在将默认账号设为 {name}",
  "wbAccounts.toast.switchCliUpdated": "CodeBuddy CLI 默认账号已更新",
  "wbAccounts.toast.configUpdated": "配置已更新",
  "wbAccounts.toast.switchCliFail": "CodeBuddy CLI 切换失败",
  "wbAccounts.toast.switchIdeLoading": "正在切换 CodeBuddy IDE…",
  "wbAccounts.toast.switchIdeLoadingDesc": "将注入凭证并重启 CodeBuddy IDE",
  "wbAccounts.toast.switchIdeUpdated": "CodeBuddy IDE 已切换",
  "wbAccounts.toast.switchIdeFail": "CodeBuddy IDE 切换失败",
  // 编辑器内 CodeBuddy 插件（VS Code / JetBrains）：四个程序位共用一套文案（`{client}` 插值）。
  "wbAccounts.toast.switchPluginLoading": "正在切换 {client} 账号…",
  "wbAccounts.toast.switchPluginLoadingDesc": "将注入凭证并重启 {client}",
  "wbAccounts.toast.switchPluginUpdated": "{client} 已切换",
  "wbAccounts.toast.switchPluginFail": "{client} 切换失败",
  "wbAccounts.toast.cliIntegrationUpdated": "CodeBuddy CLI 接入已更新",
  "wbAccounts.toast.cliIntegrationFail": "CodeBuddy CLI 接入失败",
  "wbAccounts.toast.remarkSaved": "备注已保存",
  "wbAccounts.toast.remarkCleared": "备注已清除",
  "wbAccounts.toast.remarkSaveFail": "备注保存失败",
  "wbAccounts.toast.groupSaved": "分组已保存",
  "wbAccounts.toast.groupCleared": "已移出分组",
  "wbAccounts.toast.groupSaveFail": "分组保存失败",
  // P4：未检测到的程序位点击后，引导用户去设置页完成配置。
  "wbAccounts.toast.guideIdeSetting": "请到设置页配置 CodeBuddy IDE 路径",
  "wbAccounts.toast.guideCliSetting": "请到设置页完成 CodeBuddy CLI 接入",
  "wbAccounts.page.accounts": "账号",

  // =====================================================================
  // ---- P4 工具栏 / 表格（WorkBuddy 专属数据：选项标签 + 列头）----
  // =====================================================================
  // 壳的通用文案在 `shared.toolbar.*` / `shared.table.*`；这里只放 WB 的**选项与列头**。
  // ★ 2026-10-10：后端为 `AccountMeta` 记录了 `lastUsedAt` ⇒ 「最近使用」不再是假选项
  //   （决策 #3 的原判据是「无字段」，现已不成立）。
  "wbAccounts.toolbar.sort.priority": "积分优先级",
  "wbAccounts.toolbar.sort.name": "名称",
  "wbAccounts.toolbar.sort.expiry": "到期时间",
  "wbAccounts.toolbar.sort.recent": "最近使用",
  // 工具栏主操作（原型 `viewAccounts` :2428）：打开「添加账号」三选抽屉（原型 `drawerAdd()`）。
  "wbAccounts.toolbar.addAccount": "添加账号",
  "wbAccounts.toolbar.group.none": "不分组",
  "wbAccounts.toolbar.group.program": "按程序位",
  "wbAccounts.toolbar.group.status": "按状态",
  // ★ 2026-10-10：后端为 `AccountMeta` 记录了 `groupName`（自由文本）⇒ 支持「自定义组」分组。
  "wbAccounts.toolbar.group.custom": "自定义组",
  // 「自定义组」分桶里，空 / 缺省 `groupName` 落入的显式桶名。
  "wbAccounts.toolbar.groupBucket.ungrouped": "未分组",
  // 「按状态」分桶的状态集由本平台自备（决策 #8）。
  "wbAccounts.toolbar.statusBucket.normal": "正常",
  "wbAccounts.toolbar.statusBucket.notCheckedIn": "今日未签到",
  "wbAccounts.toolbar.statusBucket.expired": "凭据已过期",
  "wbAccounts.toolbar.statusBucket.needsRelogin": "需重新登录",
  // 该账号未挂在任何程序位上时的桶名。
  "wbAccounts.toolbar.programBucket.unbound": "未绑定程序位",
  "wbAccounts.table.account": "账号",
  "wbAccounts.table.status": "状态",
  "wbAccounts.table.credits": "积分",
  "wbAccounts.table.todayTokens": "今日 Token",
  "wbAccounts.table.expiry": "到期",
  "wbAccounts.table.programs": "程序位",
  "wbAccounts.table.current": "当前",

  // =====================================================================
  // 客户端限流台账（账号 × 模型 × 恢复时刻，**只读**观测）
  // =====================================================================
  // 数据源是客户端自身落盘的三张只读表 join（见 core `client_limits.rs`）——
  // **不依赖网关、不代理客户端流量**。排序与状态由后端定：仍受限 → 状态未知 → 已恢复。
  "wbAccounts.limits.title": "客户端限流",
  "wbAccounts.limits.subtitle": "从客户端本地数据自动汇总：哪个账号在哪个模型上被上游限流、何时恢复。",
  "wbAccounts.limits.refresh": "刷新",
  "wbAccounts.limits.windowLabel": "扫描范围",
  "wbAccounts.limits.window.7": "近 7 天",
  "wbAccounts.limits.window.30": "近 30 天",
  "wbAccounts.limits.window.all": "全部",
  "wbAccounts.limits.loading": "正在读取限流记录…",
  "wbAccounts.limits.loadFailed": "读取失败",
  "wbAccounts.limits.empty": "所选范围内没有限流记录。",
  "wbAccounts.limits.active": "仍受限",
  "wbAccounts.limits.recovered": "已恢复",
  // 「恢复时间未知」是**第三种状态**，既不是「仍受限」也不是「已恢复」。
  "wbAccounts.limits.unknown": "状态未知",
  "wbAccounts.limits.accountUnknown": "未知账号",
  "wbAccounts.limits.modelUnknown": "未知模型",
  "wbAccounts.limits.resetAt": "恢复于 {time}",
  "wbAccounts.limits.resetUnknown": "恢复时间未知",
  // ⚠️ 上游文案里的「恢复于」是**配额窗口**重置时刻，不是解锁时刻（实测报 429 后 16 分钟即恢复产出）。
  // 有实测实证时改显实证时刻，避免「已恢复」与「恢复于 明天 09:04」自相矛盾。
  "wbAccounts.limits.resetHint": "上游文案里的恢复时刻是**配额窗口**的重置时间，不等于解锁时刻；判状态以右侧徽章为准。",
  "wbAccounts.limits.recoveredByEvidence": "已恢复（{time} 实测有产出）",
  "wbAccounts.limits.occurredAt": "最近 {time}",
  "wbAccounts.limits.summary": "共 {total} 条 · 仍受限 {active} 条",
  "wbAccounts.limits.summaryUnknown": " · 状态未知 {unknown} 条",
  "wbAccounts.limits.summarySkipped": " · 已排除 {skipped} 条非限流错误",
  "wbAccounts.limits.skippedHint": "窗口内的网络类错误（502 网络不可达、ECONNRESET 断连等）不是上游限流，不计入本台账，这里只报数。",
  "wbAccounts.limits.occurrences": "{count} 条记录",
  "wbAccounts.limits.occurrencesHint": "该条目合并了 {count} 条原始记录（同一账号 · 模型 · 错误码 · 恢复时刻）。同一次上游限流会**扇出**到多个会话文件，故条数常大于 1。",
  "wbAccounts.limits.code": "错误码 {code}",
  "wbAccounts.limits.queued": "排队中",
  "wbAccounts.limits.queuedHint": "上游排队类错误（6020 / 6022）：模型资源紧张或已达使用上限，等待恢复。精确文案待确认，暂按 code 原样标注。",
  "wbAccounts.limits.session": "会话 {id}",
} as const;
