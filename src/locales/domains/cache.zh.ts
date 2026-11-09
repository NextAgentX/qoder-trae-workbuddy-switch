/**
 * 文案域：**清除缓存**（设置页「备份与恢复」分区的缓存清理卡片）。
 *
 * 域划分见 `src/locales/zh.ts` 的组合入口。改动本文件请遵守那里的三条约定。
 *
 * ## ★ 一条红线
 *
 * 清除是**不可逆**操作：确认弹窗必须列出具体类别与体积，并且如实说明
 * 「账号、配置与用量账本不受影响」——不写「安全」「放心」这类无法兑现的词。
 * zh + en 必须同步（en 见 `cache.en.ts`）。
 */
export const zh = {
  "cache.title": "清除缓存",
  "cache.intro":
    "数据目录里可再生的缓存与备份，按类别勾选后清除。清除不可逆；账号、配置与用量账本永不受影响。",
  "cache.loadFailed": "扫描失败",
  "cache.refresh": "重新扫描",
  "cache.total": "合计 {size}",
  "cache.storePath": "数据目录：{path}",
  "cache.entries": "{count} 个文件",
  "cache.clear": "清除所选…",
  "cache.clearNone": "请先勾选要清除的类别",
  "cache.confirmTitle": "确认清除所选缓存？",
  "cache.confirmBody":
    "将删除以下类别共 {size}，此操作不可逆。账号、配置与用量账本不受影响。",
  "cache.confirmOk": "确认清除",
  "cache.confirmCancel": "取消",
  "cache.clearing": "正在清除…",
  "cache.cleared": "已释放 {size}",
  "cache.clearedPartial": "已释放 {size}，{count} 个项目未能清除",
  "cache.clearFailed": "清除失败",
  "cache.aria": "清除{category}",

  // ---- 四类（id 与 core `cache.rs` 的类别常量一一对应，勿单方面改）----
  "cache.category.sessionBackups": "会话复制备份",
  "cache.category.sessionBackupsDesc":
    "每次会话复制 / 分歧处理前的整库快照。清除后失去对应回滚点；当前会话与账号不受影响。",
  "cache.category.snapshots": "备份快照",
  "cache.category.snapshotsDesc":
    "自动备份、导入前快照、认证文件与客户端设置的留底。清除后失去对应恢复入口。",
  "cache.category.caches": "可再生缓存",
  "cache.category.cachesDesc":
    "会话指纹、官方用量、模型目录等接口与扫描缓存。清除后下次访问重新取数。",
  "cache.category.logs": "运行日志",
  "cache.category.logsDesc": "网关、签到与轮换日志。清除后失去排查用的历史记录。",
};
