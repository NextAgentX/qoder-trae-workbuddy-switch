/**
 * 文案域：**上游客户端 settings.json 统一托管** —— 中文（键的权威之一）。
 *
 * 归属文件：`src/components/client-settings.tsx`。
 *
 * ## 边界（**极易与相邻域混淆**）
 *
 * 本域管的是**被本工具代理的上游客户端**（WorkBuddy 两版 + Trae 四个程序位）
 * 自己的 `settings.json`，**不是**本工具自己的设置：
 *
 * - `wbSettings` / `traePages` 里那些 `proxy*` / `traePath*` 是**本工具**的代理设置
 *   （落在 `~/.buddy-switch/**`），与本域**两个不同的文件**，只是名字撞车；
 * - `skillManager` 是另一套应用级资产（skills 目录）。
 *
 * 槽位路径由后端规格表（core `client_settings::CLIENT_SLOTS`）唯一决定，
 * 前端只渲染 `displayKey` 指向的文案，**不得**自行拼路径。
 *
 * 键前缀：`clientSettings.`
 */
export const zh = {
  "clientSettings.entry": "客户端设置",
  "clientSettings.title": "上游客户端设置",
  "clientSettings.description":
    "统一托管被代理客户端自身的 settings.json：读取、键值编辑、备份、恢复与跨槽位比对。",
  "clientSettings.webuiNote":
    "当前为远程（webui）模式：读写的是服务端机器上的客户端配置文件，不是本机。",

  "clientSettings.reload": "刷新",
  "clientSettings.loading": "加载中…",
  "clientSettings.error": "加载失败：{message}",

  "clientSettings.product.workbuddy": "WorkBuddy",
  "clientSettings.product.trae": "Trae",

  "clientSettings.slot.workbuddyCn": "WorkBuddy 国内版",
  "clientSettings.slot.workbuddyGlobal": "WorkBuddy 国际版",
  "clientSettings.slot.traeCodeCn": "Trae 国内版",
  "clientSettings.slot.traeCodeGlobal": "Trae 国际版",
  "clientSettings.slot.traeWorkCn": "TRAE SOLO CN",
  "clientSettings.slot.traeWorkGlobal": "TRAE SOLO",

  "clientSettings.status.installed": "已安装",
  "clientSettings.status.notInstalled": "未安装",
  "clientSettings.status.running": "运行中",
  "clientSettings.status.stopped": "未运行",

  "clientSettings.path": "路径",
  "clientSettings.format": "排版",
  "clientSettings.format.compact": "压缩（无换行）",
  "clientSettings.format.flat": "无缩进",
  "clientSettings.format.tab": "Tab",
  "clientSettings.format.spaces": "{n} 空格",
  "clientSettings.format.other": "自定义（{indent}）",

  "clientSettings.notInstalled.title": "该客户端未安装",
  "clientSettings.notInstalled.description":
    "找不到该槽位的 settings.json，无法读取或编辑。安装并至少运行一次该客户端后，这里会出现内容。",

  "clientSettings.keys": "共 {n} 个键",
  "clientSettings.raw": "原文",
  "clientSettings.raw.show": "查看原文",
  "clientSettings.raw.hide": "收起原文",

  "clientSettings.edit.title": "键值编辑",
  "clientSettings.edit.hint":
    "键支持点号路径（如 claw.channels.wechatmp.enabled）；写入会**保持原排版**，只改目标键那一行，并在写入前自动备份。",
  "clientSettings.edit.key": "键",
  "clientSettings.edit.keyPlaceholder": "点号路径，如 http.proxy",
  "clientSettings.edit.value": "新值",
  "clientSettings.edit.valuePlaceholder": "JSON 值，如 \"\" 或 true 或 {\"a\":1}",
  "clientSettings.edit.save": "写入",
  "clientSettings.edit.saving": "写入中…",
  "clientSettings.edit.done": "已写入 {keys}",
  "clientSettings.edit.unchanged": "内容与现状相同，未写入、未备份",
  "clientSettings.edit.invalidJson": "新值不是合法 JSON：{message}",
  "clientSettings.edit.emptyKey": "请先填写要编辑的键",
  "clientSettings.edit.backup": "改前原文已备份到 {path}",
  "clientSettings.edit.noBackup": "本次未产生备份（无实际改写或备份失败）",
  "clientSettings.edit.failed": "写入失败：{message}",

  "clientSettings.backups.title": "备份点",
  "clientSettings.backups.count": "共 {n} 个",
  "clientSettings.backups.empty": "还没有备份点。每次实际改写前会自动备份一份。",
  "clientSettings.backups.restore": "恢复",
  "clientSettings.backups.restoring": "恢复中…",
  "clientSettings.backups.done": "已恢复到 {id}（恢复前的当前文件已另存一份备份）",
  "clientSettings.backups.failed": "恢复失败：{message}",

  "clientSettings.diff.title": "差异比对",
  "clientSettings.diff.hint":
    "任选两个槽位比对，输出「仅 A 有 / 仅 B 有 / 值不同」三类（嵌套对象拍平成点号路径）。跨产品比对时，代理字段按语义映射表归并成一组。",
  "clientSettings.diff.a": "槽位 A",
  "clientSettings.diff.b": "槽位 B",
  "clientSettings.diff.run": "比对",
  "clientSettings.diff.running": "比对中…",
  "clientSettings.diff.onlyA": "仅 A 有",
  "clientSettings.diff.onlyB": "仅 B 有",
  "clientSettings.diff.different": "值不同",
  "clientSettings.diff.same": "相同 {n}",
  "clientSettings.diff.empty": "两个槽位没有差异。",
  "clientSettings.diff.failed": "比对失败：{message}",
  "clientSettings.diff.proxy": "代理（跨产品归并）",
  "clientSettings.diff.proxySame": "两侧等价",
  "clientSettings.diff.proxyDifferent": "两侧不同",
  "clientSettings.diff.proxyAbsent": "未设置",
} as const;