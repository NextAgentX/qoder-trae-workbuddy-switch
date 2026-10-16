/**
 * 文案域：**Trae 产品线 · 页面**（中文，键的权威之一）。
 *
 * 归属文件：
 * `src/pages/TraeAccountsPage.tsx`、`src/pages/TraeSettingsPage.tsx`、
 * `src/pages/TraeTokenStatsPage.tsx`、`src/pages/TraeCreditsPage.tsx`、
 * `src/pages/TraeApiServicePage.tsx`
 *
 * ⚠️ 命名空间用 `trae`（不是 `traePages`）：页面与组件两个域共用同一前缀、
 * 靠二级段区分（`trae.page.*` / `trae.comp.*`），因为两者本就是同一产品的文案。
 *
 * 键前缀：`trae.page.`
 */
export const zh = {
  // =====================================================================
  // TraeAccountsPage.tsx —— 「账号管理」页
  // =====================================================================
  "trae.page.accounts.title": "账号",
  "trae.page.accounts.subtitle": "管理 {line} 账号的登录凭据、签到与登录态切换。与 WorkBuddy 的账号库彼此独立。",
  "trae.page.accounts.loadFailed": "无法读取 Trae 数据",

  // ---- 通用 action 结果提示 ----
  "trae.page.accounts.actionDone": "{label}完成",
  "trae.page.accounts.actionFailed": "{label}失败",
  "trae.page.accounts.actionCheckinAll": "签到并刷新积分",
  "trae.page.accounts.actionSwitch": "切换{line}账号",
  "trae.page.accounts.actionSaveLogin": "保存登录态",
  // 保存的目标只能是「该账号此刻登录着的程序位」；一个都没有时如实报错，不静默跳过。
  "trae.page.accounts.saveLoginNoProgram": "该账号当前没有登录在任何 Trae 客户端上，无法保存登录态",
  "trae.page.accounts.actionRefreshJwt": "刷新 JWT",
  "trae.page.accounts.actionThaw": "解除冷却",
  "trae.page.accounts.actionDelete": "删除账号",
  // P4：最小批量条的动作标签（走 actionDone / actionFailed 组合）。
  "trae.page.accounts.actionBatchCheckin": "批量签到",
  "trae.page.accounts.actionRefreshCredits": "刷新积分",
  // 备注单独一套提示（不走 actionDone）：保存与清空是两件不同的事，
  // 统一成「编辑备注完成」会让「我到底是删掉了还是写上了」看不出来。
  "trae.page.accounts.remarkSaved": "备注已保存",
  "trae.page.accounts.remarkCleared": "备注已清除",
  "trae.page.accounts.remarkSaveFailed": "备注保存失败",

  // ---- 合并旧产品线账号库 ----
  "trae.page.accounts.mergeDone": "已合并旧产品线账号库",
  "trae.page.accounts.mergeSummary": "并入 {added} 个账号（保留现有 {kept} 个）",
  "trae.page.accounts.mergeGroups": "分组 {count} 个",
  "trae.page.accounts.mergeBackup": "旧数据已备份到 {path}",
  "trae.page.accounts.mergeBackupFailed": "⚠️ 备份失败，请先手动复制数据目录",
  "trae.page.accounts.mergeFailed": "合并旧账号库失败",
  "trae.page.accounts.mergeSnapshotsDone": "已搬运遗留登录态快照",
  "trae.page.accounts.mergeGuide.title": "升级引导：Trae 双客户端已合并",
  "trae.page.accounts.mergeGuide.desc": "官方已把 Trae Work 与 Trae Code 合并为单一客户端，本工具随之收敛为「每个区域一个 Trae 客户端」，并在本次启动自动完成存量数据迁移。",
  "trae.page.accounts.mergeGuide.copied": "已搬入 {count} 个账号的登录态快照（合并前保存的），原目录原样保留、不会被删除。",
  "trae.page.accounts.mergeGuide.kept": "{count} 个账号在合并后的客户端里已有更新的快照，已保留较新的那份。",
  "trae.page.accounts.mergeGuide.none": "本机没有需要搬运的历史快照。",
  "trae.page.accounts.mergeGuide.errors": "有 {count} 个账号的快照没能搬入，切换它们时仍会提示「登录态快照不存在」。",
  "trae.page.accounts.mergeGuide.adviceTitle": "建议：逐个账号重新登录并保存一次",
  "trae.page.accounts.mergeGuide.advice": "旧快照来自旧客户端的设备标识。若某个账号切换后提示未登录或登错人，请先在 Trae 客户端里手动登录该账号，再回到本页对它点「保存登录态」重建快照 —— 此后切换就会稳定复现。",
  "trae.page.accounts.mergeGuide.close": "知道了",
  "trae.page.accounts.mergeSnapshots": "搬入 {count} 个账号的登录态快照（客户端合并前保存的）",
  "trae.page.accounts.mergeSnapshotsFailed": "⚠️ {count} 个快照未能搬运，详见运行日志",

  // ---- 签到（全部 / 单个）----
  "trae.page.accounts.creditsRefreshFailed": "积分刷新失败",
  "trae.page.accounts.noNeedCheckin": "没有需要签到的账号",
  "trae.page.accounts.checkinNotRun": "未执行签到",
  "trae.page.accounts.checkinSkippedDesc": "{name} 已被跳过（今日已签、冷却中或凭据过期）",
  "trae.page.accounts.alreadyCheckedToday": "今天已签到",
  "trae.page.accounts.checkinFailed": "签到失败",
  "trae.page.accounts.checkinFailedDesc": "{name}：{reason}",
  "trae.page.accounts.checkinSuccess": "签到成功",
  "trae.page.accounts.checkinSuccessDesc": "{name}：+{delta} 积分",
  // ---- 批量签到的结果摘要（口径对齐 WorkBuddy 的 `wbAccounts.toast.checkinBatch*`）----
  "trae.page.accounts.checkinBatchSuccess": "{n} 个签到成功",
  "trae.page.accounts.checkinBatchAlready": "{n} 个已签到",
  "trae.page.accounts.checkinBatchFailed": "{n} 个失败",
  // ★ B4：部分失败时 toast 的 action —— 可「只重试失败项」，不必把成功的一起重跑。
  "trae.page.accounts.checkinRetryFailed": "重试失败项",

  // ---- 导入本机账号 ----
  "trae.page.accounts.importLocalDone": "已导入本机账号",
  "trae.page.accounts.importLocalUid": "{name} · UID {uid}",
  "trae.page.accounts.importLocalFailed": "导入本机账号失败",

  // ---- 「添加与迁移账号」横幅 ----
  "trae.page.accounts.addTitle": "添加与迁移账号",
  "trae.page.accounts.addSubtitle": "快速接入新账号，或从已有环境恢复",
  "trae.page.accounts.oauthLogin": "OAuth 网页登录",
  "trae.page.accounts.pasteJwt": "粘贴 JWT",
  "trae.page.accounts.pasteJwtTitle": "手动粘贴 Cloud-IDE-JWT（兜底方式，优先用上方网页登录）",
  "trae.page.accounts.exportTitle": "导出账号备份",

  // ---- 首屏空态（`EmptyTraeCard`）----
  // ⚠️ 2026-10-10：原「客户端环境说明行」（`client` / `runtime` / `checkedTodayLabel` /
  //    `availableCredits` / `todayEarned` / `jwtExpiring` / `cooling` / `clearCooldownAll` /
  //    `dataDirLabel` …）已随那条细说明**一并删除**（用户要求去掉，原型骨架里也没有它）。
  //    这里只保留仍被 `EmptyTraeCard` 引用的键。
  "trae.page.accounts.dataDirMissing": "（尚未生成，请先启动一次 Trae 并登录）",

  // ---- 账号工具栏 ----
  "trae.page.accounts.sectionTitle": "账号",
  "trae.page.accounts.countAria": "{count} 个账号",
  /* ★ 列表区**不再有可见标题**（原型 `viewAccounts` 的列表区没有「账号 N」标题，
     计数由页头副标题表达）⇒ 语义改由 `section` 的 `aria-label` 承担，与 WorkBuddy 一致。 */
  "trae.page.accounts.sectionAria": "账号列表（{count} 个）",
  // ⚠️ 2026-10-10：工具栏上的「分组筛选」下拉已删除（用户要求工具栏与 WorkBuddy 一致），
  //    随之删除 `groupFilterAria` / `allAccounts` / `ungrouped` / `groupItem` 四条文案。
  "trae.page.accounts.fallbackGroup": "分组",
  // P4 工具栏：排序 / 分组选项（由 `availableSorts` / `availableGroups` 数据驱动，不摆假选项）
  "trae.page.accounts.toolbar.sort.priority": "积分优先级",
  "trae.page.accounts.toolbar.sort.name": "名称",
  "trae.page.accounts.toolbar.sort.expiry": "到期时间",
  "trae.page.accounts.toolbar.group.none": "不分组",
  "trae.page.accounts.toolbar.group.program": "按程序位",
  "trae.page.accounts.toolbar.group.status": "按状态",
  "trae.page.accounts.toolbar.group.group": "按自定义组",
  "trae.page.accounts.toolbar.statusBucket.normal": "正常",
  "trae.page.accounts.toolbar.statusBucket.notCheckedIn": "今日未签到",
  "trae.page.accounts.toolbar.statusBucket.expired": "凭据过期",
  "trae.page.accounts.toolbar.statusBucket.cooling": "冷却中",
  "trae.page.accounts.toolbar.programBucket.unbound": "未绑定程序位",
  "trae.page.accounts.toolbar.groupBucket.default": "未分组",
  // P4 表格列头
  "trae.page.accounts.table.account": "账号",
  "trae.page.accounts.table.status": "状态",
  "trae.page.accounts.table.credits": "积分",
  "trae.page.accounts.table.expiry": "到期",
  "trae.page.accounts.table.programs": "程序位",
  "trae.page.accounts.table.current": "当前",
  "trae.page.accounts.autoCheckin": "自动签到",
  "trae.page.accounts.autoCheckinSaving": "正在保存自动签到设置",
  "trae.page.accounts.skipChecked": "跳过已签到",
  "trae.page.accounts.skipCheckedAria": "跳过今日已签到账号",
  "trae.page.accounts.checkinSettingsSaving": "正在保存签到设置",
  "trae.page.accounts.checkinRefreshAll": "签到并刷新全部账号积分",
  "trae.page.accounts.autoCheckinSaveFailed": "自动签到设置保存失败",
  "trae.page.accounts.settingsSaveFailed": "设置保存失败",
  "trae.page.accounts.switchIncomplete": "切换未完成",

  // ---- 签到结果卡 ----
  "trae.page.accounts.resultTitle": "签到结果",
  "trae.page.accounts.resultProgress": "正在处理 {index} / {total}",
  "trae.page.accounts.resultLine": "{name}：{status}",
  "trae.page.accounts.resultSkipped": "已签到",
  "trae.page.accounts.resultSuccess": "成功",
  "trae.page.accounts.resultFailed": "失败",
  "trae.page.accounts.resultEmpty": "本轮没有需要签到的账号（今日已签到、冷却中或凭据过期）。",

  // ⚠️ 2026-10-10：原「上次签到卡」（`lastTitle` / `lastUnknownTime` / `lastNotToday` /
  //    `lastTruncated` / `lastCooldownDetail` / `lastCooldownItem` / `lastCooldownPermanent` /
  //    `lastCooldownSep`）已随那张卡**一并删除**（用户要求去掉）。
  //    批量签到结果改由共享 `CheckinResultBar` 就地呈现（工具栏与列表之间），不另开卡片。

  // ---- 账号增删改提示 ----
  "trae.page.accounts.accountAdded": "已添加账号",
  "trae.page.accounts.autoRenewSuffix": " · 支持自动续期",
  "trae.page.accounts.exportedCount": "已导出 {count} 个账号",
  "trae.page.accounts.importDone": "导入完成",
  "trae.page.accounts.importSummary": "新增 {added} · 覆盖 {overwritten} · 跳过 {skipped}",
  "trae.page.accounts.deleteTitle": "删除账号",
  "trae.page.accounts.deleteBody": "确定删除账号「{name}」？登录态快照会保留，可在「设置 → 登录态快照」中单独清理。删除后可在回收站恢复。",
  "trae.page.accounts.cancel": "取消",
  "trae.page.accounts.delete": "删除",
  // 账号名与 userId 都取不到时（记录损坏 / 刚被删除）的兜底显示名。
  "trae.page.accounts.unknownAccount": "未知账号",

  // ---- 粘贴 JWT 添加账号 ----
  "trae.page.accounts.addDialogTitle": "粘贴 JWT 添加账号",
  "trae.page.accounts.addDialogDesc": "兜底方式，仅在无法使用网页登录时使用。填 `Cloud-IDE-JWT …` 令牌（可从代理日志或旧版客户端的明文存储中取得）。用户名留空时按 UID 尾部自动命名。",
  "trae.page.accounts.addDialogNote": "注意：这种方式拿到的账号没有 refresh_token，JWT 过期后需要重新粘贴；网页登录的账号可以自动续期。",
  "trae.page.accounts.addDialogName": "账号名（可选）",
  "trae.page.accounts.addDialogNamePlaceholder": "例如 主号",
  "trae.page.accounts.addDialogGroup": "分组（可选）",
  "trae.page.accounts.ungroupedPlaceholder": "未分组",
  "trae.page.accounts.needJwt": "请粘贴 JWT",
  "trae.page.accounts.addFailed": "添加失败",
  "trae.page.accounts.add": "添加",

  // ---- 空态卡 ----
  "trae.page.accounts.emptyTitle": "还没有 Trae 账号",
  "trae.page.accounts.emptyReasons": "可能原因：",
  "trae.page.accounts.emptyReasonNotInstalled": "未安装 Trae 桌面客户端",
  "trae.page.accounts.emptyReasonNoSession": "客户端已安装，但当前无可用登录态",
  "trae.page.accounts.emptyReasonEncryptedLead": "客户端已登录，但凭据是加密存储的",
  "trae.page.accounts.emptyReasonEncryptedMid": "——Trae 1.107.x 起不再把明文 JWT 写入",
  "trae.page.accounts.emptyReasonEncryptedTail": "，因此本机导入读不到它。这是**当前版本最常见**的原因。",
  "trae.page.accounts.emptyReasonEmpty": "账号库为空，且尚未通过网页登录或备份文件添加过账号",
  "trae.page.accounts.emptyRecommendLabel": "推荐做法：",
  "trae.page.accounts.emptyRecommendBody": "用「OAuth 网页登录」在浏览器里登录一次并授权。应用会自己接住回调，无需查找任何本地文件，也无需粘贴令牌——这是唯一不受加密存储影响的方式。",
  "trae.page.accounts.emptyRecheck": "重新检测",
  "trae.page.accounts.emptyImportHint": "仅适用于旧版客户端或凭据仍是明文的安装；1.107.x 起通常读不到",
  "trae.page.accounts.emptyImportTry": "尝试从本机导入",
  "trae.page.accounts.emptyDetectedDir": "已探测的登录态目录：",
  "trae.page.accounts.emptyExpectedDir": "客户端数据目录（预期位置）：",
  "trae.page.accounts.emptyNoDataDir": "尚未探测到客户端数据目录",
  "trae.page.accounts.emptyDirNote": "该文件在新版客户端里已不再保存明文凭据；仅供确认客户端数据目录位置。",
  "trae.page.accounts.emptyLaunchClient": "启动客户端",

  // =====================================================================
  // TraeSettingsPage.tsx —— 「设置」页
  // =====================================================================
  "trae.page.settings.title": "设置",
  "trae.page.settings.subtitle": "客户端定位、本地端口、签到策略，以及登录态快照与运行日志。Trae 的配置与 WorkBuddy 分开保存，互不影响。",
  "trae.page.settings.loadFailed": "无法读取 Trae 配置",
  "trae.page.settings.cancel": "取消",
  "trae.page.settings.refresh": "刷新",
  "trae.page.settings.copy": "复制",
  "trae.page.settings.exportCsv": "导出 CSV",
  "trae.page.settings.save": "保存",
  "trae.page.settings.saveFailed": "保存失败",
  "trae.page.settings.unknown": "未知",
  "trae.page.settings.running": "运行中",
  "trae.page.settings.notRunning": "未运行",
  /* `trae.page.settings.loading` 已删除（P3）：文字型加载态改用 `<Skeleton>` 占位，
     键不再被引用 —— 按本仓约定（`zh.ts` 头注释第 3 条）「只有真正被引用的键才留下」。 */
  "trae.page.settings.actionDone": "{label}完成",
  "trae.page.settings.actionFailed": "{label}失败",

  // ---- Trae 客户端 ----
  "trae.page.settings.clientGroup": "Trae 客户端",
  "trae.page.settings.installLabel": "安装",
  "trae.page.settings.detected": "已检测到",
  "trae.page.settings.detectedVersion": "已检测到 v{version}",
  "trae.page.settings.notDetected": "未检测到",
  "trae.page.settings.runLabel": "运行",
  "trae.page.settings.platform": "平台 {name}",
  "trae.page.settings.autoDetectPath": "自动探测到的安装",
  "trae.page.settings.autoDetectDataDir": "自动探测到的数据目录",
  "trae.page.settings.dirMissing": "目录不存在",
  "trae.page.settings.multiLead": "同机装有多个 Trae 产品线（",
  "trae.page.settings.multiSep": "、",
  "trae.page.settings.multiTail": " 等）时，按最近活跃的那个自动选定。",
  "trae.page.settings.pathLabel": "客户端可执行文件路径",
  "trae.page.settings.pathDesc": "仅作用于【{region}】。留空即自动探测，会覆盖系统盘与各非系统盘的常见安装目录，无需手动填写；仅在装在非常规位置时指定。国内版与国际版各存各的，互不覆盖。",
  "trae.page.settings.pathPlaceholder": "留空则自动探测常见安装位置",
  "trae.page.settings.pathStale": "【{region}】这里填的路径已失效（客户端升级或合并后搬到了新位置），当前实际使用的是自动探测到的：{path}。点「保存」清空本栏即可恢复自动探测。",

  // ---- 端口与网络 ----
  "trae.page.settings.networkGroup": "端口与网络",
  "trae.page.settings.proxyPortLabel": "本地代理端口",
  "trae.page.settings.proxyPortDesc": "本地 MITM 代理的监听端口；登录态捕获依赖它。",
  "trae.page.settings.apiPortLabel": "API 网关端口",
  "trae.page.settings.apiPortDesc": "默认 7864，与 WorkBuddy 网关（57891）错开——两者都占用 /v1/chat/completions。",
  "trae.page.settings.proxyDomainsLabel": "代理解密域名",
  "trae.page.settings.proxyDomainsDesc": "逗号分隔。只有这些域名会被本地代理解密以捕获登录态；其余流量走加密隧道直连。",

  // ---- 自动签到（排程）----
  "trae.page.settings.autoCheckinGroup": "自动签到",
  "trae.page.settings.autoCheckinEnable": "启用自动签到",
  "trae.page.settings.autoCheckinDesc": "到点由后台调度器签一轮，覆盖国内版与国际版的全部 Trae 账号；应用启动时若当天还没签也会补一轮。默认关闭 —— 打开后本程序会主动访问 Trae 上游。",
  "trae.page.settings.checkinHoursLabel": "触发时间",
  "trae.page.settings.checkinHoursDesc": "每天在这些整点各签一轮（0-23，可多个）。需要先打开上面的开关；手动签一轮用账号页工具栏的刷新按钮。",
  "trae.page.settings.autoCheckinSaveFailed": "自动签到设置保存失败",
  "trae.page.settings.jwtRefreshGroup": "自动刷新 JWT",
  "trae.page.settings.jwtRefreshEnable": "启用自动续期",
  "trae.page.settings.jwtRefreshDesc": "JWT 剩余不足 24 小时时，用该账号的 refresh_token 自动换一份新的。到点由后台调度器执行，应用启动时若还没续也会补一轮。只有网页登录的账号有 refresh_token；默认开启，关掉后只能到账号卡片菜单里手动「刷新 JWT」。",
  "trae.page.settings.jwtRefreshHoursLabel": "检查时间",
  "trae.page.settings.jwtRefreshHoursDesc": "每天在这些整点各检查一轮（0-23，可多个）。只对快过期的账号发请求，没到期的一律跳过。",
  "trae.page.settings.jwtRefreshSaveFailed": "JWT 续期设置保存失败",

  // ---- 签到策略 ----
  "trae.page.settings.checkinGroup": "签到策略",
  "trae.page.settings.skipCheckedLabel": "跳过今日已签到账号",
  "trae.page.settings.skipCheckedDesc": "避免对同一账号重复请求 claim，减少配额浪费与风控暴露。",
  "trae.page.settings.skipExpiredLabel": "跳过 JWT 已过期账号",
  "trae.page.settings.skipExpiredDesc": "失效凭据必然返回 401；跳过可避免把账号打入永久冷却。",
  "trae.page.settings.retryLabel": "网络失败重试次数",
  "trae.page.settings.retryDesc": "仅对网络层异常重试；业务失败（如额度限制）不会重试。",
  "trae.page.settings.logRetentionGroup": "日志保留",
  "trae.page.settings.logRetentionLabel": "日志保留天数",
  "trae.page.settings.logRetentionDesc": "超期的日志行会在启动时裁剪；无日期前缀的外部输出一律保留。",

  // ---- 设备标识 ----
  "trae.page.settings.deviceGroup": "设备标识",
  "trae.page.settings.resetLabel": "重置设备标识",
  "trae.page.settings.resetDesc": "依次处理 6 层客户端设备标识：machineid 文件、storage.json 遥测与设备 ID、aha/TinyStorage、注册表 MachineGuid（仅 Windows）、WebView 追踪数据。操作前请确保已保存登录态快照。",
  "trae.page.settings.reset": "重置",
  "trae.page.settings.resetResultPrefix": "上次结果：",
  "trae.page.settings.resetLayers": "{done} / {total} 项生效",
  "trae.page.settings.resetConfirm": "确认重置",
  "trae.page.settings.resetDialogDesc": "将改写 Trae 客户端的机器码与设备标识文件。建议先关闭 Trae，并确认已保存登录态快照。在非 Windows 平台上，注册表 MachineGuid 一层会明确标记为「平台不支持」，不会伪装成功。",
  "trae.page.settings.resetDone": "设备标识重置完成",
  "trae.page.settings.resetFailed": "重置失败",
  "trae.page.settings.stepOk": "已处理",
  "trae.page.settings.stepUnsupported": "平台不支持",
  "trae.page.settings.stepSkipped": "已跳过",

  // ---- 平台能力 ----
  "trae.page.settings.capabilitiesGroup": "平台能力",
  "trae.page.settings.capClientDetection": "客户端检测",
  "trae.page.settings.capProcessControl": "进程控制",
  "trae.page.settings.capScheduledTask": "定时任务",
  "trae.page.settings.capMachineGuid": "MachineGuid 重置",
  "trae.page.settings.capabilityUnsupported": "（不支持）",
  "trae.page.settings.capabilitiesNote": "标记为「不支持」的能力会明确给出「在哪支持 + 为什么这里不行」，而不是伪装成功。",
  "trae.page.settings.capabilityRow": "{label}（仅 {on}）：{reason}",

  // ---- 关于 ----
  "trae.page.settings.aboutGroup": "关于",
  "trae.page.settings.aboutTitle": "Trae 支持项目",
  "trae.page.settings.aboutDesc": "账号切换、签到与设备标识能力参考开源实现，并按本工具的架构重写为原生 Rust。",
  "trae.page.settings.aboutUnknownSite": "站点地址未知",

  // ---- 运行日志 ----
  "trae.page.settings.logsGroup": "运行日志",
  "trae.page.settings.kindAll": "全部",
  "trae.page.settings.kindApp": "运行",
  "trae.page.settings.kindCheckin": "签到",
  "trae.page.settings.kindSwitch": "切换",
  "trae.page.settings.allDates": "全部日期",
  "trae.page.settings.searchPlaceholder": "搜索关键字…",
  "trae.page.settings.search": "查询",
  "trae.page.settings.logsReset": "重置",
  "trae.page.settings.autoRefreshAria": "自动刷新日志",
  "trae.page.settings.autoRefresh": "自动刷新",
  "trae.page.settings.totalCount": "共 {count} 条",
  "trae.page.settings.kindCount": "{label} {count}",
  "trae.page.settings.logsLoadFailed": "读取日志失败",
  "trae.page.settings.detailLabel": "日志明细",
  "trae.page.settings.showing": "显示 {shown} / {total} 条",
  "trae.page.settings.logsEmptyTitle": "暂无日志",
  "trae.page.settings.logsEmptyBody": "日志文件还不存在或当前筛选条件下没有匹配行。运行一次签到或切换账号后，这里会开始出现记录。",
  "trae.page.settings.noTime": "（无时间）",
  "trae.page.settings.noteFallback": "只读取本机纯文本运行日志。",
  "trae.page.settings.sourceExists": "存在",
  "trae.page.settings.sourceMissing": "未生成",
  "trae.page.settings.logDir": "日志目录：{dir}",
  "trae.page.settings.csvHeader": "时间\t类型\t内容",

  // ---- 网关请求日志 ----
  "trae.page.settings.gatewayGroup": "网关请求日志",
  "trae.page.settings.statusAll": "全部状态",
  "trae.page.settings.statusOk": "成功 (2xx)",
  "trae.page.settings.statusClient": "客户端错误 (4xx)",
  "trae.page.settings.statusServer": "服务端错误 (5xx)",
  "trae.page.settings.gwSearchPlaceholder": "搜索账号 / 模型 / 端点 / 错误…",
  "trae.page.settings.gwClear": "清空",
  "trae.page.settings.gwCleared": "日志已清空",
  "trae.page.settings.gwLoadFailed": "读取网关日志失败",
  "trae.page.settings.gwSummary": "共 {count} 条{filtered}{tokens}。网关未启用时这里是空的——请求日志只由网关写入。",
  "trae.page.settings.gwFiltered": "（已从 {total} 条中筛选）",
  "trae.page.settings.gwTokens": " · 合计 {tokens} Token",
  "trae.page.settings.colTime": "时间",
  "trae.page.settings.colAccount": "账号",
  "trae.page.settings.colModel": "模型",
  "trae.page.settings.colStatus": "状态",
  "trae.page.settings.colLatency": "耗时",
  "trae.page.settings.colToken": "Token",
  "trae.page.settings.gwEmptyTitle": "暂无网关请求日志",
  "trae.page.settings.gwEmptyBody": "在「API 服务」页启用 Trae 网关后，外部客户端发来的每一次请求都会记在这里。",
  "trae.page.settings.gwFooter": "点击任意一行查看完整字段。网关默认只记元数据（模型、状态、耗时、token 数）。",
  "trae.page.settings.gwDetailTitle": "请求详情",
  "trae.page.settings.gwDetailDesc": "网关只记录元数据。若需要正文，请到「API 服务」页开启「记录请求正文」。",
  "trae.page.settings.gwDetailCopied": "详情已复制",

  // ---- 登录态快照 ----
  "trae.page.settings.profilesGroup": "登录态快照",
  "trae.page.settings.clientLabel": "客户端",
  "trae.page.settings.currentAccount": "当前账号",
  "trae.page.settings.programLabel": "程序位",
  "trae.page.settings.snapshotCount": "快照 {count} 份",
  "trae.page.settings.dataDirLabel": "客户端数据目录：",
  "trae.page.settings.backupLabel": "备份当前登录态",
  "trae.page.settings.backupDesc": "把客户端此刻的登录态存到指定槽位；每个账号一份快照，精确复制 {count} 类登录态核心文件。槽位名建议用账号 UID；last 是切换流程自动使用的兜底槽位。",
  "trae.page.settings.slotPlaceholder": "槽位名（账号 UID）",
  "trae.page.settings.backup": "备份",
  "trae.page.settings.profilesEmpty": "还没有快照。切换账号时会自动为当前账号保存一份。",
  "trae.page.settings.fallbackSlot": "兜底槽位",
  "trae.page.settings.fileCount": "{count} 个文件",
  "trae.page.settings.updatedAt": "更新于 {time}",
  "trae.page.settings.restore": "恢复",
  "trae.page.settings.delete": "删除",
  "trae.page.settings.profilesFooter": "快照保存在 Buddy Switch 自己的数据目录下，与 Trae 客户端目录分离，因此删除快照不会影响正在使用的登录态。",
  "trae.page.settings.restoreTitle": "恢复到该快照",
  "trae.page.settings.restoreBodyLead": "将用「{slot}」的快照覆盖 Trae 客户端当前的登录态。",
  "trae.page.settings.restoreBodyStrong": "当前登录态不会被自动保存",
  "trae.page.settings.restoreBodyTail": "，如需保留请先在上面备份。建议先关闭 Trae 客户端。",
  "trae.page.settings.restoreConfirm": "确认恢复",
  "trae.page.settings.deleteTitle": "删除快照",
  "trae.page.settings.deleteBody": "将删除槽位「{slot}」的 {count} 个文件（{size}）。账号记录不受影响，但该槽位将无法再恢复。",
} as const;
