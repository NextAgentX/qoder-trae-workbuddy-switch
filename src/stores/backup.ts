/**
 * 全局备份页状态（P7 T03）—— `selection / creds / preview / result / pending`。
 *
 * ## 职责边界
 *
 * 本 store **只做状态容器 + 纯逻辑转发**：勾选切换与预估口径在 `lib/backup-selection.ts`
 * （可被门禁跑真代码），localStorage 段在 `lib/app-settings-snapshot.ts`。真正的
 * **写操作**（导出 / 预览 / 导入 / 保存自动备份配置）留在组件里 —— 它们要 `api.ts`，
 * 而 store 引入 API 运行时会让「纯状态」变成「什么都依赖」，也会挡住门禁加载。
 *
 * ## 默认值即安全默认
 *
 * - 勾选：4 作用域 × 3 分区**全选**（D14）；
 * - 凭据：**关**（D15）—— 备份文件默认不含 token；
 * - 冲突策略：**保留本地**（D16）—— 幂等的成立条件（连导两次第二次全「跳过」）。
 */

import { create } from "zustand";

import { readAppSettingsSnapshot } from "@/lib/app-settings-snapshot";
import {
  buildSelectionState,
  defaultBackupSelection,
  toggleScopeAll,
  toggleScopePart,
  type BackupPart,
  type BackupSelection,
} from "@/lib/backup-selection";
import type { BackupSelectionState } from "@/lib/api";
import type { Scope } from "@/lib/scope";
import type {
  AutoBackupConfig,
  AutoBackupEntry,
  BackupConflictPolicy,
  ImportPreview,
  ImportSummary,
} from "@/lib/types";

/** 冲突策略默认值：**保留本地**（D16）。 */
export const DEFAULT_CONFLICT_POLICY: BackupConflictPolicy = "keepLocal";

interface BackupState {
  /** 逐作用域 × 分区的三态勾选。 */
  selection: BackupSelection;
  /** 是否包含登录凭据（默认关，D15）。 */
  creds: boolean;
  /** 导入冲突策略（默认保留本地，D16）。 */
  conflict: BackupConflictPolicy;
  /** 导入预览（`null` = 尚未选择文件）。 */
  preview: ImportPreview | null;
  /** 导入结果（`null` = 尚未导入）。 */
  result: ImportSummary | null;
  /** 导出 / 导入进行中（按钮内 `<Loader2>`，**不是**区块加载态）。 */
  pending: boolean;
  /** 自动备份配置（`null` = 尚未读取）。 */
  autoConfig: AutoBackupConfig | null;
  /** 自动备份 / 导入前快照列表（最新在前）。 */
  autoBackups: AutoBackupEntry[];

  togglePart: (scope: Scope, part: BackupPart) => void;
  toggleScope: (scope: Scope) => void;
  setCreds: (value: boolean) => void;
  setConflict: (policy: BackupConflictPolicy) => void;
  setPreview: (preview: ImportPreview | null) => void;
  setResult: (result: ImportSummary | null) => void;
  setPending: (pending: boolean) => void;
  setAutoConfig: (config: AutoBackupConfig | null) => void;
  setAutoBackups: (list: AutoBackupEntry[]) => void;
  /** 清掉「这一次导入」的预览与结果（重新选文件时用）。 */
  resetImport: () => void;
  /** 回到出厂默认（全选 / 凭据关 / 保留本地）。 */
  reset: () => void;
  /** 组装后端入参：勾选 + 凭据开关 + **localStorage 段快照**。 */
  selectionState: () => BackupSelectionState;
}

/** 把快照收窄成后端入参需要的 `Record<string, unknown>`（丢掉 `undefined`）。 */
function snapshotRecord(): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(readAppSettingsSnapshot())) {
    if (typeof value === "string") out[key] = value;
  }
  return out;
}

export const useBackupStore = create<BackupState>((set, get) => ({
  selection: defaultBackupSelection(),
  creds: false,
  conflict: DEFAULT_CONFLICT_POLICY,
  preview: null,
  result: null,
  pending: false,
  autoConfig: null,
  autoBackups: [],

  togglePart: (scope, part) =>
    set((state) => ({ selection: toggleScopePart(state.selection, scope, part) })),
  toggleScope: (scope) => set((state) => ({ selection: toggleScopeAll(state.selection, scope) })),
  setCreds: (creds) => set({ creds }),
  setConflict: (conflict) => set({ conflict }),
  setPreview: (preview) => set({ preview }),
  setResult: (result) => set({ result }),
  setPending: (pending) => set({ pending }),
  setAutoConfig: (autoConfig) => set({ autoConfig }),
  setAutoBackups: (autoBackups) => set({ autoBackups }),

  resetImport: () => set({ preview: null, result: null, pending: false }),

  reset: () =>
    set({
      selection: defaultBackupSelection(),
      creds: false,
      conflict: DEFAULT_CONFLICT_POLICY,
      preview: null,
      result: null,
      pending: false,
    }),

  selectionState: () => buildSelectionState(get().selection, get().creds, snapshotRecord()),
}));
