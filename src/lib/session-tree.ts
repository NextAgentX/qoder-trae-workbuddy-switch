/**
 * 会话树数据适配层（P4 T01）—— **纯函数**，无 React 依赖，可门禁 / 可单测。
 *
 * ## 为什么从 `switch-account-dialog.tsx` 抽出来
 *
 * 迁移向导的第 2 步要展示**三层会话树**（类型 → 目录 → 会话）并给出「已选 N / M」计数
 * （`docs/redesign/P4-MIGRATION-ARCH.md` §1.4）。改造前这套逻辑（`buildSessionTree` /
 * `sessionFolderLabel` / `groupSessionsByFolder` / `selectionState`）内联在 870 行的
 * 弹窗组件里，**选择集与渲染耦合**，既不好测，也无法在「确认页」复用同一份口径。
 *
 * 抽出来后，**计数口径只有一处**（{@link treeSelectionStats}）—— 会话树上的「已选 N / M」
 * 与确认页的「已选 N 个会话 / M 个目录」读的是同一个函数，因此两处**不可能漂移**
 * （§1.5「确认页数字必须等于实际会执行的操作」）。
 *
 * ## 三层的口径（§1.4）
 *
 * 1. **类型层**：`Session.isPlayground` → 对话 / 工作区（i18n）；
 * 2. **目录层**：`Session.cwd` 的**末段**（{@link sessionFolderLabel}）—— ⚠️ 目录名**不是字段**，
 *    是从 `cwd` 推导出来的，别当成独立字段；
 * 3. **会话层**：`Session.title`。
 *
 * ⚠️ **字段归属**（本项目已三次踩坑）：`Session` 只有 `id / title / cwd / updatedAt /
 * hasHistory / isPlayground`，**没有** `msgs` / `lastUsedAt` / `replicaCount`。
 *
 * ## `displayText` 白屏防线（§5）
 *
 * `title` / `cwd` 在类型上是 `string`，但运行时可能被客户端存成对象（AI 生成的展示名带
 * emoji 时会出现 `{"zh": "…"}`）。对象直接进 React 树会抛
 * `Objects are not valid as a React child` ⇒ **整棵树卸载 ⇒ 白屏**。
 * 因此叶节点在这里就过一遍 {@link displayText}，把脏值收敛成 `string`。
 */

import { displayText } from "@/lib/display-text";
import type { Translate } from "@/lib/i18n";
import type { Session } from "@/lib/types";

/** 会话叶节点（三层树的第三层）。 */
export interface SessionLeaf {
  /** `Session.id` —— 稳定 key，也是选择集里的成员。 */
  id: string;
  /** 会话标题（已过 `displayText` 归一，脏值收敛为空串）。 */
  title: string;
  hasHistory: boolean;
  updatedAt: number;
}

/** 目录节点（三层树的第二层）。 */
export interface SessionDirNode {
  /** 稳定 key：`t{ti}/d{di}`。 */
  key: string;
  /** 目录名（`cwd` 末段；空 cwd 为「未分组」）。 */
  label: string;
  sessions: SessionLeaf[];
}

/** 类型节点（三层树的第一层）。 */
export interface SessionTypeNode {
  /** 稳定 key：`t{ti}`。 */
  key: string;
  /** 类型名（对话 / 工作区）。 */
  label: string;
  dirs: SessionDirNode[];
}

/**
 * 选中统计 —— 确认页的「已选 N 个会话 / M 个目录」即 `{selected, dirsSelected}`。
 *
 * ⚠️ `dirsSelected` = **被选中会话覆盖的目录数**，**不是**全部目录数（§1.4）。
 */
export interface TreeSelectionStats {
  /** 已选会话数（= `selected.size` 在树内的实际命中数）。 */
  selected: number;
  /** 全部可选会话数。 */
  total: number;
  /** 被选中会话覆盖的目录数。 */
  dirsSelected: number;
}

/** 目录分组（内部用，`Session[]` 形态，转成叶节点后即为 {@link SessionDirNode}）。 */
interface FolderGroup {
  key: string;
  label: string;
  sessions: Session[];
}

/**
 * 目录名：`cwd` 的最后一段（对齐 WorkBuddy 侧栏的文件夹名）。
 *
 * 空 cwd 归入「未分组」。`cwd` 过 `displayText` 防白屏（脏值会让 `split` 抛 TypeError）。
 */
export function sessionFolderLabel(cwd: string, t: Translate): string {
  const normalized = displayText(cwd)?.trim().replace(/[\\/]+$/, "") ?? "";
  if (!normalized) return t("wbAccounts.dialog.ungrouped");
  const parts = normalized.split(/[\\/]/);
  return parts[parts.length - 1] || normalized;
}

/** 按工作目录分组；文件夹顺序跟会话一样按最近活动排（先出现的目录在前）。 */
function groupSessionsByFolder(sessions: Session[], t: Translate): FolderGroup[] {
  const groups = new Map<string, Session[]>();
  const order: string[] = [];
  for (const session of sessions) {
    const key = displayText(session.cwd)?.trim() || "__none__";
    let list = groups.get(key);
    if (!list) {
      list = [];
      groups.set(key, list);
      order.push(key);
    }
    list.push(session);
  }
  return order.map((key) => ({
    key,
    label: key === "__none__" ? t("wbAccounts.dialog.ungrouped") : sessionFolderLabel(key, t),
    sessions: groups.get(key) ?? [],
  }));
}

/** `Session` → 叶节点（过 `displayText` 防白屏）。 */
function toLeaf(session: Session): SessionLeaf {
  return {
    id: session.id,
    title: displayText(session.title) ?? "",
    hasHistory: Boolean(session.hasHistory),
    updatedAt: session.updatedAt,
  };
}

/**
 * 构建三层会话树：**对话 / 工作区** → **目录** → **会话**。
 *
 * 类型层来自 `Session.isPlayground`（`true` ⇒ 对话，`false` ⇒ 工作区）；
 * 类型顺序固定为「对话在前、工作区在后」（与 WorkBuddy 侧栏一致）。
 * 两个类型各自按 `cwd` 分组 —— 因此**每一层都是三层结构**，不存在「某个类型平铺」的特例
 * （那会让「三层」这条契约出现例外分支）。
 */
export function buildSessionTree(sessions: Session[], t: Translate): SessionTypeNode[] {
  const tasks = sessions.filter((session) => session.isPlayground);
  const spaces = sessions.filter((session) => !session.isPlayground);
  const groups: { label: string; sessions: Session[] }[] = [];
  if (tasks.length > 0) {
    groups.push({ label: t("wbAccounts.wizard.typeConversation"), sessions: tasks });
  }
  if (spaces.length > 0) {
    groups.push({ label: t("wbAccounts.wizard.typeWorkspace"), sessions: spaces });
  }
  return groups.map((group, typeIndex) => ({
    key: `t${typeIndex}`,
    label: group.label,
    dirs: groupSessionsByFolder(group.sessions, t).map((folder, dirIndex) => ({
      key: `t${typeIndex}/d${dirIndex}`,
      label: folder.label,
      sessions: folder.sessions.map(toLeaf),
    })),
  }));
}

/** 收集树里的全部会话 id（`selectAll` 用）。 */
function treeSessionIds(tree: SessionTypeNode[]): string[] {
  const ids: string[] = [];
  for (const type of tree) {
    for (const dir of type.dirs) {
      for (const leaf of dir.sessions) ids.push(leaf.id);
    }
  }
  return ids;
}

/**
 * 统计选中情况 —— **唯一的计数口径**。
 *
 * `selected` = 命中数，`total` = 全部会话数，`dirsSelected` = 被选中会话覆盖的目录数。
 * 「全不选 ⇒ 0」「全选 ⇒ total」是它的直接推论（断言 4）。
 */
export function treeSelectionStats(
  tree: SessionTypeNode[],
  selected: Set<string>,
): TreeSelectionStats {
  let total = 0;
  let selectedCount = 0;
  let dirsSelected = 0;
  for (const type of tree) {
    for (const dir of type.dirs) {
      let dirHasSelection = false;
      for (const leaf of dir.sessions) {
        total += 1;
        if (selected.has(leaf.id)) {
          selectedCount += 1;
          dirHasSelection = true;
        }
      }
      if (dirHasSelection) dirsSelected += 1;
    }
  }
  return { selected: selectedCount, total, dirsSelected };
}

/**
 * 三态勾选状态：`allOn` / `someOn`（半选）。
 *
 * 半选用于 `aria-checked="mixed"`（§3.11）—— 类型 / 目录两级都要。
 */
export function selectionState(
  ids: string[],
  selected: Set<string>,
): { allOn: boolean; someOn: boolean } {
  const on = ids.filter((id) => selected.has(id)).length;
  return { allOn: ids.length > 0 && on === ids.length, someOn: on > 0 && on < ids.length };
}

/** 对一组 id 做「全开 ⇒ 全关，否则全开」的翻转（类型 / 目录共用）。 */
function toggleIds(ids: string[], selected: Set<string>): Set<string> {
  const next = new Set(selected);
  const { allOn } = selectionState(ids, selected);
  if (allOn) {
    for (const id of ids) next.delete(id);
  } else {
    for (const id of ids) next.add(id);
  }
  return next;
}

/** 切换一个类型节点下的全部会话。 */
export function toggleType(type: SessionTypeNode, selected: Set<string>): Set<string> {
  return toggleIds(
    type.dirs.flatMap((dir) => dir.sessions.map((leaf) => leaf.id)),
    selected,
  );
}

/** 切换一个目录节点下的全部会话。 */
export function toggleDir(dir: SessionDirNode, selected: Set<string>): Set<string> {
  return toggleIds(
    dir.sessions.map((leaf) => leaf.id),
    selected,
  );
}

/**
 * 全选 / 全不选。
 *
 * `on = false` ⇒ **空集**（不是「保留原样」）；`on = true` ⇒ 树里全部 id。
 * 这两条正是断言 4 的「全不选 → 计数归零，再全选 → 恢复」。
 */
export function selectAll(tree: SessionTypeNode[], on: boolean): Set<string> {
  if (!on) return new Set();
  return new Set(treeSessionIds(tree));
}
