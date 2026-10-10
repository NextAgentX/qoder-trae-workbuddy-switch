/**
 * 文案域：**全局备份与恢复**（P7 §3.14）。
 *
 * 域划分见 `src/locales/zh.ts` 的组合入口。改动本文件请遵守那里的三条约定。
 *
 * ## ★ 一条红线（§3.5 / §7 #2）
 *
 * 备份信封在 `includesCredentials=false` 时**剥离了凭据** ⇒ 它**本就不完整**。
 * 因此文案**不得**写「备份完整」，必须写「凭据不含在内，导入后需重新登录」
 * （见 `backup.credentials.notice`）。说「完整」即**假承诺** —— 用户会以为换机后
 * 直接可用，实际必须重新登录。zh + en 必须同步（en 见 `backup.en.ts`）。
 */
export const zh = {
  // ---- 导出 ----
  "backup.export.title": "导出备份",
  "backup.export.intro":
    "一次覆盖全部作用域。默认不含凭据 —— 文件里不会出现任何 token 字段。",
  "backup.export.scopeAria": "全选 {scope}",
  "backup.export.partAria": "{scope} 的{part}",
  "backup.export.part.accounts": "账号",
  "backup.export.part.config": "配置",
  "backup.export.part.gateway": "网关",
  "backup.export.credsLabel": "包含登录凭据（token）",
  "backup.export.credsDesc": "开启后文件可被他人直接用来登录你的账号",
  "backup.export.credsWarning":
    "文件将包含明文凭据，请只保存在可信位置，不要通过聊天工具发送。",
  "backup.export.credsConfirmTitle": "确认导出含凭据的备份？",
  "backup.export.credsConfirmBody":
    "文件将包含明文 token，任何拿到文件的人都能直接登录你的账号。请只保存在可信位置。",
  "backup.export.credsConfirmOk": "仍然导出",
  "backup.export.credsConfirmCancel": "取消",
  "backup.export.estimate":
    "预计 ~{size} KB · {accounts} 个账号 · {scopes} 个作用域 · {parts} 个分区",
  "backup.export.credentialsOn": "含凭据",
  "backup.export.credentialsOff": "不含凭据",
  "backup.export.button": "导出为文件…",
  "backup.export.buttonCreds": "导出（含凭据）…",
  "backup.export.emptyHint": "至少勾选一个分区才能导出",
  "backup.export.saved": "已导出：{path}",
  "backup.export.error": "导出失败",

  // ---- ★ 凭据文案红线（§7 #2）：不得写「备份完整」 ----
  "backup.credentials.notice": "凭据不含在内，导入后需重新登录",

  // ---- 导入 ----
  "backup.import.title": "导入备份",
  "backup.import.intro":
    "选择文件后先预览，确认新增 / 覆盖 / 跳过数量再落盘。导入前会自动快照当前状态。",
  "backup.import.choose": "选择备份文件…",
  "backup.import.reading": "正在读取…",
  "backup.import.previewError": "预览失败",
  "backup.import.error": "导入失败",
  "backup.import.preview.head": "将导入 {kind}",
  "backup.import.preview.meta": "formatVersion {version} · {exportedAt} · {credentials}",
  "backup.import.preview.credentialsIncluded": "含凭据",
  "backup.import.preview.credentialsExcluded": "不含凭据",
  "backup.import.preview.counts": "新增 {added} · 覆盖 {overwritten} · 跳过 {skipped} · 冲突 {conflicts}",
  "backup.import.preview.rejected": "该分区被拒绝（未知作用域）",
  "backup.import.preview.configs": "配置：{count} 项将被覆盖（{items}）",
  "backup.import.preview.relogin": "该文件不含凭据 ⇒ 导入后 {count} 个账号需重新登录。",
  "backup.import.preview.noScopes": "该文件不包含任何作用域分区。",
  "backup.import.conflict.label": "冲突处理",
  "backup.import.conflict.desc": "同一账号（按 uid 匹配）已存在时怎么办",
  "backup.import.conflict.keepLocal": "保留本地",
  "backup.import.conflict.overwrite": "覆盖",
  "backup.import.conflict.keepBoth": "保留两者",
  "backup.import.cancel": "取消",
  "backup.import.run": "开始导入",
  "backup.import.result.title": "导入结果",
  "backup.import.result.summary": "新增 {added} · 覆盖 {overwritten} · 跳过 {skipped}",
  "backup.import.result.failed": "{count} 个分区导入失败",
  "backup.import.result.retry": "重试失败项",
  "backup.import.result.retryDone": "失败分区已重试导入",
  "backup.import.result.retryPartial": "仍有 {count} 个分区导入失败",
  "backup.import.result.snapshot": "已自动快照导入前状态，可在「从自动备份恢复」中回退。",
  "backup.import.result.done": "完成",

  // ---- 自动备份 ----
  "backup.auto.title": "自动备份",
  "backup.auto.enableLabel": "启用定时自动备份",
  "backup.auto.enableDesc":
    "每天自动备份一次，保留最近 7 份。关闭后不再产生新备份，已产生的保留。",
  "backup.auto.listTitle": "备份记录",
  "backup.auto.empty": "暂无自动备份",
  "backup.auto.emptyBody": "定时备份每天产生一份，导入前也会自动快照；产生后在此列出。",
  "backup.auto.kind.auto": "定时",
  "backup.auto.kind.preImport": "导入前",
  "backup.auto.kind.other": "其他",
  "backup.auto.restore": "从此备份恢复",
  "backup.auto.restoreTitle": "从备份恢复？",
  "backup.auto.restoreBody": "恢复会覆盖当前数据。导入前会自动快照，可再次回退。",
  "backup.auto.restoreOk": "恢复",
  "backup.auto.restoreCancel": "取消",
  "backup.auto.saved": "自动备份设置已保存",
  "backup.auto.saveFailed": "保存失败",
  "backup.auto.loadFailed": "加载失败",
  "backup.auto.restored": "已从备份恢复",
  "backup.auto.restoreFailed": "恢复失败",
  "backup.auto.refresh": "刷新",
} as const;
