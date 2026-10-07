/**
 * 账号搜索索引（**唯一**匹配规则，列表搜索与命令面板 P6 共用）。
 *
 * ## 为什么抽成独立模块
 *
 * 账号列表的搜索框（本设计）与命令面板（P6，`PLAN §3.13`）都要「按关键词找账号」。
 * 若两处各写一套匹配规则，同一个关键词在两处给出不同结果 —— 那是一种**静默错误**
 * （用户不会收到任何提示，只会觉得「这工具搜东西不一致」）。
 * 把「把哪些字段拼成可搜文本」与「命中判定」收敛到本模块，两处调用同一份实现。
 *
 * ## 归一规则
 *
 * 小写 + 去首尾空白。**不**做分词 / 模糊匹配：账号字段量小、前缀匹配足够，
 * 复杂匹配会让「为什么这个词搜不到」变成谜题。字段缺失（`null` / `undefined`）
 * 按空串处理，不参与拼接。
 *
 * ⚠️ 传入字段可能被后端吐成对象（`issue #2`），调用方**必须**先过
 * `displayText`（`@/lib/display-text`）再传进来 —— 本模块只接受字符串。
 */

/** 归一化：小写、去首尾空白。 */
function normalize(value: string): string {
  return value.trim().toLowerCase();
}

/**
 * 把账号的可搜索字段拼成一个**归一化**字符串（小写、去空白）。
 *
 * 字段之间用空格分隔，因此「昵称 + 邮箱」这类跨字段关键词不会被误命中；
 * 缺失字段按空串跳过（不产生多余空格）。
 */
export function accountSearchText(fields: {
  name?: string | null;
  email?: string | null;
  uid?: string | null;
  remark?: string | null;
  group?: string | null;
}): string {
  const parts: string[] = [];
  for (const value of [fields.name, fields.email, fields.uid, fields.remark, fields.group]) {
    if (typeof value === "string" && value.length > 0) parts.push(value);
  }
  return normalize(parts.join(" "));
}

/**
 * 命中判断。空查询恒 `true`（列表不过滤）。
 *
 * 判据是**子串**：`searchText` 与 `query` 都已归一，`includes` 即可。
 */
export function matchesAccountQuery(searchText: string, query: string): boolean {
  const q = normalize(query);
  if (!q) return true;
  return searchText.includes(q);
}
