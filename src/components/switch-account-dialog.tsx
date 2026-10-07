/**
 * 跨作用域迁移向导（P4 T04）—— **抽屉内的三步向导**。
 *
 * ## 为什么从「单页模态」改成「抽屉内三步向导」（`P4-MIGRATION-ARCH.md` §3.5）
 *
 * 改造前是**单页模态 `Dialog`**：来源版本 Tabs + 复制会话开关 + 记忆 / 连接器开关 +
 * 三层会话树，**一步到底**。用户看不出「现在在配哪一段、还剩几步」，也**没有确认步**
 * （迁移会写目标账号数据，不可逆）。原型要求「切换账号 = 抽屉内三步向导
 * （目标与来源 → 迁移内容 → 确认）」。
 *
 * ## 抽屉而不是模态（§3.11 / 决策 #5）
 *
 * 用 Radix `Sheet`（`@radix-ui/react-dialog` 的侧滑封装）—— `role="dialog"` +
 * `aria-modal` + **焦点陷阱** + `Esc` + 关闭后**焦点归还**都是它的默认能力，
 * **不手写**（手写必然与 `ui/dialog.tsx` 漂移）。
 *
 * ## ★ 保留既有行为（不许因向导化而弄丢）
 *
 * - `doSwitch`：先 `migrateAccountData`（记忆 / 连接器）再 `switchAccount`（会话复制）；
 * - **统一重启确认**（`RestartConfirmDialog`，§3.15.3）—— 点「执行切换」不直接执行；
 * - **权限自愈**：结构化错误码 `permission.denied` ⇒ 每 2s 轮询授权状态并自动恢复；
 * - 进度提示（桌面走 Tauri 事件、webui 走 HTTP 轮询）；
 * - 「复制会话常开」的一次性初始化（`defaultsRef`）。
 *
 * ## 能力边界（§1.3 / §3.5 红线）
 *
 * 会话复制的来源 = 源 region 的**当前登录账号**（后端 `switchAccount` 无 `sourceAccountId`）；
 * 跨产品（WB→Trae）会话复制**后端不支持** ⇒ 来源只列各作用域**当前账号**，
 * 跨产品来源 **disabled + 原因**，**不写任何假承诺**。
 */

import { useEffect, useRef, useState } from "react";
import { ExternalLink, Inbox, Loader2 } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";

import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { accountIdentity } from "@/components/account-card";
import { RestartConfirmDialog } from "@/components/restart-confirm-dialog";
import { MigrationSummary } from "@/components/switch-account/migration-summary";
import { SessionTree } from "@/components/switch-account/session-tree";
import {
  scopeLabel,
  SourceAccountPicker,
  type SourceOption,
} from "@/components/switch-account/source-account-picker";
import { WizardSteps, type WizardStep } from "@/components/switch-account/wizard-steps";
import * as api from "@/lib/api";
import { decodeError } from "@/lib/error-code";
import { hasCapability } from "@/lib/platforms/registry";
import { REGIONS, regionLabel } from "@/lib/region";
import { SCOPES, scopeOf, scopeParts, type Scope } from "@/lib/scope";
import { useT, type Translate } from "@/lib/i18n";
import {
  buildSessionTree,
  selectAll as selectAllInTree,
  toggleDir,
  toggleType,
  treeSelectionStats,
  type SessionDirNode,
  type SessionTypeNode,
} from "@/lib/session-tree";
import type { AccountMeta, MigrateResult, Region, Session } from "@/lib/types";

/** WorkBuddy 桌面客户端的显示名（商标，中英写法相同 ⇒ 不放进词表）。 */
const WORKBUDDY_LABEL = "WorkBuddy";

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** 目标账号 */
  account: AccountMeta | null;
  /** 切换完成后刷新列表 */
  onDone?: () => void;
  /** 目标版本；缺省为国内版。 */
  region?: Region;
  /**
   * 「复制会话常开」（设置页 → 账号切换）。
   *
   * 由调用方注入而不是本组件自己读配置：调用方已经为了置顶读过一次，
   * 再读一次会出现两个页面各自缓存的配置互相打架。
   */
  copySessionsByDefault?: boolean;
}

/**
 * 「复制会话常开」的一次性初始化状态。
 *
 * 会话加载 effect 同时被 `sourceRegion` 变化触发，因此必须把「打开时套用默认值」
 * 与「每次重取会话都重来」区分开：否则用户手动取消勾选之后，只要切一下来源版本，
 * 勾选就会被悄悄恢复成全选。
 */
interface SwitchDefaults {
  /** 本次打开是否已经套用过默认值。 */
  applied: boolean;
  /** 会话回来后是否还要补一次「全选」（消费后置 false）。 */
  selectAll: boolean;
}

/** 迁移向导：WorkBuddy 桌面客户端的三步切换（目标与来源 → 迁移内容 → 确认）。 */
export function SwitchAccountDialog({
  open,
  onOpenChange,
  account,
  onDone,
  region,
  copySessionsByDefault = false,
}: Props) {
  const t = useT();
  const [step, setStep] = useState<WizardStep>(1);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [loadingSessions, setLoadingSessions] = useState(false);
  const [copySessions, setCopySessions] = useState(false);
  /** 把来源账号的长期记忆合并进目标账号（带去重）。 */
  const [migrateMemory, setMigrateMemory] = useState(false);
  /**
   * 把来源账号的连接器配置合并进目标账号（带去重）。
   *
   * 刻意与 `migrateMemory` 做成两个**平级**独立开关，而**不是**「主开关 + 两个子开关」：
   * 嵌套结构会引入「主开关开着、两个子项都关」的退化状态，需要额外为其定义语义；
   * 两个平级独立开关天然不存在该状态。二者都关时见 `doSwitch` —— 不发起迁移请求。
   */
  const [migrateConnectors, setMigrateConnectors] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  /** 展开的节点：类型 / 目录。默认全部展开（三层都可见）。 */
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  /**
   * 统一重启确认框是否打开（P8 T03 / §3.15.3）。
   *
   * 切换 WorkBuddy 会**先关再开**，且可能**丢失未保存的文件** ⇒ 点「执行切换」不再直接执行，
   * 而是先走**同一条**重启确认流程。真正的切换在 `RestartConfirmDialog` 的 `onConfirm` 里发生。
   */
  const [restartConfirmOpen, setRestartConfirmOpen] = useState(false);
  const [error, setError] = useState("");
  const [currentUid, setCurrentUid] = useState<string | null>(null);
  const [progress, setProgress] = useState("");
  /* ★ 权限自愈的**自动重试**：记住用户上次选的 `restart`（它来自确认框回调、**不是 state**），
     授权恢复后**自动重试那次切换** —— 原型 P3 要求「恢复即自动重试」，否则用户得手动再点一次。
     ⚠️ `permissionRetriedRef` 保证**只重试一次**：否则重试又失败 ⇒ 又触发轮询 ⇒ **无限循环**。 */
  const lastRestartRef = useRef<boolean | null>(null);
  const permissionRetriedRef = useRef(false);
  /** 降级 / 异常提示（索引库不可读、扫描结果不完整）。普通场景为 null。 */
  const [sessionWarning, setSessionWarning] = useState<string | null>(null);
  /**
   * 会话列表的来源：`db` / `scan` / `empty` / `no-dir`（见 `api.listSessions`）。
   *
   * `no-dir` 必须与 `empty` 分开显示 —— 前者是**数据目录空了**（客户端刚重装 / 重置），
   * 后者才是**账号没有会话**；混为一谈会把用户引向错误结论（2026-09-23 现场）。
   */
  const [sessionSource, setSessionSource] = useState<string | null>(null);
  /**
   * 数据**来源**版本（会话 / 记忆 / 连接器取自哪一版的当前登录账号）。
   * 缺省等于目标 `region` —— 即同版本内切换，行为与改造前完全一致。
   */
  const [sourceRegion, setSourceRegion] = useState<Region>(region ?? "cn");
  /** 「从其他作用域选择…」开关（原型 `wiz-cross`）。关时只列当前作用域。 */
  const [crossScope, setCrossScope] = useState(false);
  /** 各 region 的**当前登录账号**展示名（第 1 步「来源账号」用）。 */
  const [scopeCurrent, setScopeCurrent] = useState<Record<Region, string | null>>({
    cn: null,
    global: null,
  });
  const defaultsRef = useRef<SwitchDefaults>({ applied: false, selectAll: false });

  // 关闭时清掉本次打开的一次性状态，下次打开重新按配置初始化。
  useEffect(() => {
    if (!open) {
      defaultsRef.current = { applied: false, selectAll: false };
      // 顺带关掉重启确认框：父抽屉被外部关掉时，别把它单独留在屏幕上。
      setRestartConfirmOpen(false);
    }
  }, [open]);

  // 监听后端切换进度：桌面端走 Tauri 事件，webui 走 HTTP 轮询
  useEffect(() => {
    if (api.isWebui()) {
      const timer = setInterval(() => {
        void api.switchProgress().then((p) => {
          if (p.progress) setProgress(p.progress);
        });
      }, 600);
      return () => clearInterval(timer);
    }
    let unlisten: (() => void) | undefined;
    listen<{ message: string }>("switch-progress", (e) => {
      setProgress(e.payload.message);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  /**
   * 打开抽屉时复位一次性状态 —— 其中包含「数据来源版本回到目标版本」与「回到第 1 步」。
   *
   * ★ **`sourceRegion` 绝不能出现在本 effect 的依赖里**：本 effect 会把来源版本重置成
   * 目标版本，一旦被 `sourceRegion` 触发，用户刚点的「国际版」会在下一帧被弹回
   * 「国内版」—— 症状是「点了没反应」，于是跨版本迁移整条路都走不通
   * （2026-09-24 用户报障现场）。复位只发生在「打开」这一刻，之后来源版本由用户掌控。
   */
  useEffect(() => {
    if (!open) return;
    setStep(1);
    setCrossScope(false);
    setMigrateMemory(false);
    setMigrateConnectors(false);
    setSelected(new Set());
    setExpanded(new Set());
    setError("");
    setSourceRegion(region ?? "cn");
    setSessionSource(null);
  }, [open, account, region]);

  // 加载会话：打开时取一次；此后每次改「数据来源版本」都要重取。
  useEffect(() => {
    if (!open || !account) return;
    if (!defaultsRef.current.applied) {
      defaultsRef.current.applied = true;
      defaultsRef.current.selectAll = copySessionsByDefault;
      setCopySessions(copySessionsByDefault);
    }
    // 换了来源版本后，已勾选的 id 属于**另一个版本**的数据，必须清空
    // （否则会拿 A 版的会话 id 去 B 版复制）。
    setSelected(new Set());
    setLoadingSessions(true);
    // 首次进入时 `sourceRegion` 可能还是上一次的选择，复位后会再跑一次本 effect；
    // 用 cancelled 丢弃那次过期请求，避免短暂显示「另一个版本」的会话。
    let cancelled = false;
    api
      .listSessions(sourceRegion)
      .then((res) => {
        if (cancelled) return;
        setSessions(res.sessions);
        setCurrentUid(res.current);
        setSessionWarning(res.warning ?? null);
        setSessionSource(res.source ?? null);
        // 默认全部展开：类型层与目录层一次可见，用户一眼看到「三层」结构。
        const tree = buildSessionTree(res.sessions, t);
        setExpanded(new Set(tree.flatMap((node) => [node.key, ...node.dirs.map((dir) => dir.key)])));
        // 「复制会话常开」：默认全选，用户仍可逐条取消。
        if (defaultsRef.current.selectAll) {
          defaultsRef.current.selectAll = false;
          setSelected(new Set(res.sessions.map((session) => session.id)));
        }
      })
      .catch((e) => {
        if (!cancelled) setError(api.asError(e));
      })
      .finally(() => {
        if (!cancelled) setLoadingSessions(false);
      });
    return () => {
      cancelled = true;
    };
    // 刻意**不**把 `copySessionsByDefault` 放进依赖：它决定的是「打开瞬间的初值」，
    // 任何在抽屉开着时发生的值变化都不该反过来改动用户当前的选择。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, account, sourceRegion]);

  /**
   * 取「各 region 当前登录账号」的展示名（第 1 步「来源账号」用）。
   *
   * 只读 `getStatus`：**来源账号 = 该 region 的当前登录账号**（§1.3 结论 1）——
   * 因此这里**只**需要当前账号的名字，不需要账号库列表（也就不会出现「让用户任选账号」的误导）。
   * 跨作用域开关打开时顺带取另一个 WB region；Trae 侧不可选，无需取名。
   */
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    const regions: Region[] = crossScope ? [...REGIONS] : [region ?? "cn"];
    void Promise.all(
      regions.map(async (r): Promise<[Region, string | null]> => {
        try {
          const status = await api.getStatus(r);
          const name =
            status.current?.nickname || status.current?.email || status.current?.uid || null;
          return [r, name];
        } catch {
          return [r, null];
        }
      }),
    ).then((entries) => {
      if (cancelled) return;
      setScopeCurrent((prev) => {
        const next = { ...prev };
        for (const [r, name] of entries) next[r] = name;
        return next;
      });
    });
    return () => {
      cancelled = true;
    };
  }, [open, crossScope, region]);

  /* ── 派生值 ───────────────────────────────────────────────── */

  const targetRegion: Region = region ?? "cn";
  const crossRegion = sourceRegion !== targetRegion;
  const tree = buildSessionTree(sessions, t);
  const stats = treeSelectionStats(tree, selected);
  const sessionsEmpty = !loadingSessions && sessions.length === 0;
  const needsPermission = permissionDenied(error);
  // 空列表的成因必须分开说，否则会把用户引向错误结论：
  //   no-dir → 该版本的数据目录空了（客户端刚重装 / 重置）；empty → 该版本账号确实没有会话。
  const sourceLabel = regionLabel(sourceRegion);
  const copyHint = loadingSessions
    ? t("wbAccounts.dialog.copyHintLoading")
    : error && sessionsEmpty
      ? t("wbAccounts.dialog.copyHintErrorEmpty")
      : sessionsEmpty
        ? currentUid
          ? sessionSource === "no-dir"
            ? t("wbAccounts.dialog.copyHintNoData", { region: sourceLabel })
            : t("wbAccounts.dialog.copyHintEmptyCurrent", { region: sourceLabel })
          : t("wbAccounts.dialog.copyHintNoCurrent")
        : crossRegion
          ? t("wbAccounts.dialog.copyHintCross", {
              from: regionLabel(sourceRegion),
              to: regionLabel(targetRegion),
            })
          : t("wbAccounts.dialog.copyHintSame");

  const currentScope: Scope = scopeOf("workbuddy", targetRegion);
  const candidateScopes: readonly Scope[] = crossScope ? SCOPES : [currentScope];
  const sourceOptions: SourceOption[] = candidateScopes.map((scope) => {
    const parts = scopeParts(scope);
    return {
      scope,
      accountName: scopeCurrent[parts.region] ?? null,
      supportsSessionCopy: hasCapability(parts.product, "sessionCopy"),
    };
  });
  const selectedScope: Scope = scopeOf("workbuddy", sourceRegion);
  const programLabel = t("wbAccounts.wizard.programWorkbuddy");

  /* ── 交互 ─────────────────────────────────────────────────── */

  function toggleSession(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function toggleExpanded(key: string) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  function handleToggleType(node: SessionTypeNode) {
    setSelected((prev) => toggleType(node, prev));
  }

  function handleToggleDir(node: SessionDirNode) {
    setSelected((prev) => toggleDir(node, prev));
  }

  function handleSelectAll(on: boolean) {
    setSelected(selectAllInTree(tree, on));
  }

  /** 选来源作用域：只接受支持会话复制的产品（跨产品来源已被禁用，这里再兜一层）。 */
  function pickSource(scope: Scope) {
    const parts = scopeParts(scope);
    if (!hasCapability(parts.product, "sessionCopy")) return;
    setSourceRegion(parts.region);
  }

  function toggleCross() {
    const next = !crossScope;
    setCrossScope(next);
    // 关闭跨作用域时回到当前作用域 —— 否则会留下一个「看不见却仍生效」的来源（假状态）。
    if (!next) setSourceRegion(targetRegion);
  }

  function goNext() {
    setStep((prev) => (prev < 3 ? ((prev + 1) as WizardStep) : prev));
  }

  function goBack() {
    setStep((prev) => (prev > 1 ? ((prev - 1) as WizardStep) : prev));
  }

  async function doSwitch(restart: boolean) {
    if (!account) return;
    setBusy(true);
    setProgress(t("wbAccounts.dialog.switchProgress"));
    setError("");
    const parts: string[] = [];
    const warnings: string[] = [];
    try {
      // 先迁移记忆与连接器：只碰普通文件，不需要关闭 WorkBuddy；
      // 且目标账号此刻不是登录账号，其文件不会被占用，故在切换前做最安全。
      // 至少勾选一个迁移范围时才发请求：后端对空范围返回 400「未指定任何迁移范围」，
      // 两者都关时**完全不要调用**，否则用户会看到一个莫名其妙的错误。
      if (migrateMemory || migrateConnectors) {
        setProgress(t("wbAccounts.dialog.migrateProgress"));
        // 只请求勾选的分区；未请求的分区不会出现在响应里（见 summarizeMigration）。
        const mig = await api.migrateAccountData(account.id, {
          region,
          sourceRegion,
          memory: migrateMemory,
          connectors: migrateConnectors,
        });
        parts.push(...summarizeMigration(mig, warnings, t));
      }

      setProgress(t("wbAccounts.dialog.switchProgress"));
      const res = await api.switchAccount({
        accountId: account.id,
        region,
        // 由统一重启确认框决定（§3.15.3）：勾选时 true；取消勾选时 false
        // （后端此时只写认证文件，不关不启 WorkBuddy —— 见 `switch.rs`）。
        restart,
        // 只有「跨版本」时才显式带来源版本：同版本时它由后端缺省为目标 region，
        // 少传一个字段就少一处与后端默认值漂移的机会。
        sourceRegion: sourceRegion === region ? undefined : sourceRegion,
        copySessionIds: copySessions ? [...selected] : undefined,
      });
      const nickname =
        account.nickname || account.email || account.uid || t("wbAccounts.common.unknownAccount");
      if (res.sessionCopy?.copied.length) {
        parts.push(t("wbAccounts.dialog.copiedSessions", { n: res.sessionCopy.copied.length }));
      }
      if (res.sessionCopy?.skipped?.length) {
        parts.push(t("wbAccounts.dialog.skippedSessions", { n: res.sessionCopy.skipped.length }));
      }
      // 收集复制阶段每条副本的降级警告（如「索引不可写，已复制正文」）。
      const sessionWarnings = (res.sessionCopy?.copied ?? [])
        .map((c) => c.warning)
        .filter((w): w is string => Boolean(w));
      if (sessionWarnings.length) {
        warnings.push(...sessionWarnings);
      }
      if (res.backup) parts.push(t("wbAccounts.dialog.backup", { path: res.backup }));

      // ★ 明确结果（§3.15.3）：把「是否重启」放在最前，再列迁移 / 复制细节 ——
      // 用户取消勾选时，绝不能沿用「已重启」的说法（那是假承诺）。
      const restartNote = restart
        ? t("wbAccounts.dialog.switchedDefault")
        : t("sessions.restart.resultNotRestarted", { client: WORKBUDDY_LABEL });
      const description = [restartNote, ...parts].join(t("wbAccounts.dialog.listSeparator"));
      if (warnings.length) {
        // 部分迁移失败但未阻断切换：明确降级为警告，避免用户误以为全部成功。
        toast.warning(t("wbAccounts.dialog.switchedWarn", { name: nickname }), {
          description: `${warnings.join(t("wbAccounts.dialog.listSeparator"))}${t("shared.punct.period")}${description}`,
        });
      } else {
        toast.success(t("wbAccounts.dialog.switchedOk", { name: nickname }), { description });
      }
      onOpenChange(false);
      onDone?.();
    } catch (e) {
      setError(api.asError(e));
    } finally {
      setBusy(false);
      setProgress("");
    }
  }

  /** 打开系统设置授权面板（默认完全磁盘访问），供小白一键跳转。 */
  async function openPermissionSettings() {
    try {
      await api.openPermissionSettings("all_files");
    } catch (e) {
      // 打开失败时退化为提示
      setError(api.asError(e));
    }
  }

  /** 权限自检：确认完全磁盘访问是否生效。 */
  const [permCheck, setPermCheck] = useState<string | null>(null);
  async function runPermissionCheck() {
    setPermCheck(t("wbAccounts.dialog.permissionChecking"));
    try {
      const res = await api.checkAuthPermission();
      setPermCheck(
        res.ok
          ? `✓ ${res.message}`
          : t("wbAccounts.dialog.permissionFailDir", { error: res.error ?? "", dir: res.dir ?? "" }),
      );
    } catch (e) {
      setPermCheck(t("wbAccounts.dialog.permissionFail", { error: api.asError(e) }));
    }
  }

  /**
   * 判定「是否因缺少完全磁盘访问权限而失败」。
   *
   * ⚠️ **不能按当前界面语言去找中文关键词**：文案会随语言切换，一旦变成英文，
   * `error.includes("无权限")` 永远为假 ⇒ 授权引导整块**静默失效**。
   * 因此以**结构化错误码**（`permission.denied`，见 `lib/error-code.ts`）为准，
   * 中文子串只作为**旧版本后端**的兼容兜底。
   */
  function permissionDenied(raw: string): boolean {
    const decoded = decodeError(raw);
    return decoded.code === "permission.denied" || decoded.text.includes("无权限");
  }

  // 出现「无权限」错误时，自动每 2s 轮询一次授权状态；用户拖入 app 授权成功后自动恢复
  useEffect(() => {
    if (!permissionDenied(error)) return;
    let cancelled = false;
    let timer: number | undefined;
    const check = async () => {
      try {
        const res = await api.checkAuthPermission();
        if (res.ok) {
          if (!cancelled) {
            setPermCheck(t("wbAccounts.dialog.permissionOk"));
            setError("");
            /* ★ 授权恢复 ⇒ **自动重试上次的切换**（原型 P3 要求「恢复即自动重试」；
               此前只清错误、不重试 ⇒ 用户还得手动再点一次，与文件头注释的描述不符）。
               只重试**一次**（`permissionRetriedRef`），否则重试再失败 ⇒ 又进轮询 ⇒ **无限循环**。 */
            if (!permissionRetriedRef.current && lastRestartRef.current !== null) {
              permissionRetriedRef.current = true;
              void doSwitch(lastRestartRef.current);
            }
          }
          return;
        }
      } catch {
        /* 忽略中间态 */
      }
      if (!cancelled) timer = window.setTimeout(check, 2000);
    };
    check();
    return () => {
      cancelled = true;
      if (timer) window.clearTimeout(timer);
    };
  }, [error, t]);

  const targetName =
    account?.nickname || account?.email || account?.uid || t("wbAccounts.common.unknownAccount");
  const sourceName = scopeCurrent[sourceRegion] ?? null;
  const runDisabled = busy || (copySessions && selected.size === 0);

  return (
    <>
      <Sheet open={open} onOpenChange={onOpenChange}>
        <SheetContent
          showCloseButton={!busy}
          className="flex w-full flex-col gap-0 sm:max-w-lg"
          // 切换进行中不允许用遮罩 / Esc 关掉 —— 否则会留下「半途切换」的窗口。
          onInteractOutside={(e) => {
            if (busy) e.preventDefault();
          }}
          onEscapeKeyDown={(e) => {
            if (busy) e.preventDefault();
          }}
        >
          {busy && (
            <div className="absolute inset-0 z-50 flex flex-col items-center justify-center gap-3 bg-background/85 backdrop-blur-sm">
              <Loader2 className="size-8 animate-spin text-primary" />
              <p className="text-sm font-medium">
                {progress || t("wbAccounts.dialog.switchProgress")}
              </p>
              <p className="max-w-xs text-center text-xs text-muted-foreground">
                {t("wbAccounts.dialog.switchProcessing")}
              </p>
            </div>
          )}

          <SheetHeader className="shrink-0 gap-2">
            <SheetTitle>{t("wbAccounts.dialog.switchTitle", { name: targetName })}</SheetTitle>
            <SheetDescription>{t("wbAccounts.dialog.switchDesc")}</SheetDescription>
            <div className="pt-1">
              <WizardSteps step={step} />
            </div>
          </SheetHeader>

          <div className="min-h-0 flex-1 space-y-4 overflow-x-hidden overflow-y-auto p-5">
            {step === 1 && (
              <SourceAccountPicker
                options={sourceOptions}
                selectedScope={selectedScope}
                crossScope={crossScope}
                programLabel={programLabel}
                onPick={pickSource}
                onToggleCross={toggleCross}
              />
            )}

            {step === 2 && (
              <>
                <section className="space-y-2">
                  <h4 className="text-sm font-medium">{t("wbAccounts.wizard.contentTitle")}</h4>
                  <div className="space-y-1.5">
                    <MigrateOption
                      active={copySessions}
                      disabled={loadingSessions || sessions.length === 0}
                      dataAct="wiz-opt|sessions"
                      title={t("wbAccounts.dialog.copySessions")}
                      desc={copyHint}
                      warn={sessionsEmpty}
                      onToggle={() => setCopySessions((v) => !v)}
                    />
                    <MigrateOption
                      active={migrateMemory}
                      dataAct="wiz-opt|memory"
                      title={t("wbAccounts.dialog.migrateMemory")}
                      desc={
                        crossRegion
                          ? t("wbAccounts.dialog.migrateMemoryDescCross", {
                              from: regionLabel(sourceRegion),
                              to: regionLabel(targetRegion),
                            })
                          : t("wbAccounts.dialog.migrateMemoryDescSame")
                      }
                      onToggle={() => setMigrateMemory((v) => !v)}
                    />
                    <MigrateOption
                      active={migrateConnectors}
                      dataAct="wiz-opt|connectors"
                      title={t("wbAccounts.dialog.migrateConnectors")}
                      desc={
                        crossRegion
                          ? t("wbAccounts.dialog.migrateConnectorsDescCross", {
                              from: regionLabel(sourceRegion),
                              to: regionLabel(targetRegion),
                            })
                          : t("wbAccounts.dialog.migrateConnectorsDescSame")
                      }
                      onToggle={() => setMigrateConnectors((v) => !v)}
                    />
                  </div>
                  {/* 降级警告：索引库不可读 / 不完整。仍允许勾选（降级扫描已列出
                      projects 下的 jsonl），但必须明确告诉用户数据可能缺失。 */}
                  {sessionWarning && (
                    <div className="text-xs text-amber-700 dark:text-amber-400">
                      {sessionWarning}
                    </div>
                  )}
                </section>

                {copySessions && (
                  <section>
                    {loadingSessions ? (
                      /* 区块加载 ⇒ 骨架（不是「加载中」文字）。 */
                      <div className="space-y-2" aria-busy="true">
                        <Skeleton className="h-4 w-40" />
                        <Skeleton className="h-8 w-full" />
                        <Skeleton className="h-8 w-full" />
                        <Skeleton className="h-8 w-5/6" />
                      </div>
                    ) : sessions.length === 0 ? (
                      /* 空态四要素（含出口）：出口 = 「跳过会话复制」，即用户此刻能做的下一步。 */
                      <EmptyState
                        icon={Inbox}
                        title={
                          currentUid
                            ? t("wbAccounts.dialog.noSessionsCurrent")
                            : t("wbAccounts.dialog.noSessionsNoCurrent")
                        }
                        description={copyHint}
                        action={
                          <Button variant="outline" size="sm" onClick={() => setCopySessions(false)}>
                            {t("wbAccounts.wizard.emptyExit")}
                          </Button>
                        }
                      />
                    ) : (
                      <SessionTree
                        tree={tree}
                        selected={selected}
                        expanded={expanded}
                        stats={stats}
                        onToggleSession={toggleSession}
                        onToggleType={handleToggleType}
                        onToggleDir={handleToggleDir}
                        onToggleExpanded={toggleExpanded}
                        onSelectAll={handleSelectAll}
                      />
                    )}
                  </section>
                )}
              </>
            )}

            {step === 3 && account && (
              <MigrationSummary
                target={account}
                targetIdentity={accountIdentity(account)}
                sourceName={sourceName}
                sourceScopeLabel={scopeLabel(selectedScope, t)}
                programLabel={programLabel}
                stats={stats}
                migrate={{
                  sessions: copySessions,
                  memory: migrateMemory,
                  connectors: migrateConnectors,
                }}
              />
            )}

            {error && (
              <Alert
                variant={needsPermission ? "warning" : "destructive"}
                className="min-w-0 break-all"
              >
                <AlertDescription className="min-w-0 break-all">
                  <div className="min-w-0 break-all">{error}</div>
                  {needsPermission && (
                    <div className="mt-2 space-y-2">
                      <div className="rounded-md border bg-muted/60 p-3 text-xs text-muted-foreground">
                        <p className="mb-1 font-medium text-foreground">
                          {t("wbAccounts.dialog.permissionTitle")}
                        </p>
                        <ol className="list-decimal space-y-1 pl-4">
                          <li>{t("wbAccounts.dialog.permissionStep1")}</li>
                          <li>{t("wbAccounts.dialog.permissionStep2")}</li>
                          <li>{t("wbAccounts.dialog.permissionStep3")}</li>
                        </ol>
                      </div>
                      <div className="flex flex-wrap gap-2">
                        <Button variant="outline" size="sm" onClick={openPermissionSettings}>
                          <ExternalLink />
                          {t("wbAccounts.dialog.openFullDisk")}
                        </Button>
                        <Button
                          variant="outline"
                          size="sm"
                          onClick={() => void api.revealAppInFinder()}
                        >
                          {t("wbAccounts.dialog.showInFinder")}
                        </Button>
                        <Button variant="secondary" size="sm" onClick={runPermissionCheck}>
                          {t("wbAccounts.dialog.checkNow")}
                        </Button>
                      </div>
                    </div>
                  )}
                  {permCheck && <div className="mt-2 text-xs">{permCheck}</div>}
                </AlertDescription>
              </Alert>
            )}
          </div>

          <SheetFooter className="shrink-0">
            <div className="flex items-center gap-2">
              <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
                {t("wbAccounts.dialog.cancel")}
              </Button>
              <div className="flex-1" />
              {step > 1 && (
                <Button variant="outline" data-act="wiz-back" onClick={goBack} disabled={busy}>
                  {t("wbAccounts.wizard.back")}
                </Button>
              )}
              {step < 3 && (
                <Button data-act="wiz-next" onClick={goNext} disabled={busy}>
                  {t("wbAccounts.wizard.next")}
                </Button>
              )}
              {/* ★ 不再直接切换：先走**统一重启确认**（§3.15.3），由确认框决定是否重启。 */}
              {step === 3 && (
                <Button
                  data-act="wiz-run"
                  onClick={() => setRestartConfirmOpen(true)}
                  disabled={runDisabled}
                >
                  {busy ? <Loader2 className="animate-spin" /> : null}
                  {busy ? t("wbAccounts.card.switching") : t("wbAccounts.wizard.run")}
                </Button>
              )}
            </div>
          </SheetFooter>
        </SheetContent>
      </Sheet>

      {/* 统一重启确认（P8 T03）：WorkBuddy 桌面是「需重启」客户端 ⇒ `isCli = false`。
          真正的切换在 `onConfirm` 里发生（先关本框，再由本抽屉的遮罩显示进度）。 */}
      <RestartConfirmDialog
        open={restartConfirmOpen}
        onOpenChange={setRestartConfirmOpen}
        clientLabel={WORKBUDDY_LABEL}
        isCli={false}
        onConfirm={(restart) => {
          setRestartConfirmOpen(false);
          // ★ 记住这次的选择 + 重置「已自动重试」标记 ⇒ 授权恢复后可自动重试这一回。
          lastRestartRef.current = restart;
          permissionRetriedRef.current = false;
          void doSwitch(restart);
        }}
      />
    </>
  );
}

/** 一个「迁移内容」平级开关（会话 / 记忆 / 连接器）。 */
function MigrateOption({
  active,
  disabled = false,
  title,
  desc,
  warn = false,
  dataAct,
  onToggle,
}: {
  active: boolean;
  disabled?: boolean;
  title: string;
  desc: string;
  warn?: boolean;
  dataAct: string;
  onToggle: () => void;
}) {
  return (
    <div className="flex items-center justify-between gap-3 rounded-md border px-3 py-2.5">
      <div className="min-w-0 flex-1">
        <div className="text-sm font-medium">{title}</div>
        <div
          className={
            warn ? "text-xs text-amber-700 dark:text-amber-400" : "text-xs text-muted-foreground"
          }
        >
          {desc}
        </div>
      </div>
      <Switch
        checked={active}
        onCheckedChange={() => onToggle()}
        disabled={disabled}
        data-act={dataAct}
      />
    </div>
  );
}

/** 区分「计数对象」与「`{ error }`」两种分区结果。 */
function hasError(
  section: { error: string } | object | undefined,
): section is { error: string } {
  return !!section && typeof section === "object" && "error" in section;
}

/** 无需迁移时的静默分区（源账号本来就没有这类数据）。 */
function isNoop(section: { changed?: boolean; skipped?: boolean } | undefined): boolean {
  if (!section) return true;
  return section.changed === false || section.skipped === true;
}

/**
 * 把迁移报告压成一行可读摘要，并把分区级失败追加进 `warnings`。
 *
 * 分区失败（如只读文件、JSON 损坏）不影响切换本身，因此在界面降级为警告而非错误。
 */
function summarizeMigration(mig: MigrateResult, warnings: string[], t: Translate): string[] {
  const parts: string[] = [];

  if (hasError(mig.memory)) {
    warnings.push(t("wbAccounts.dialog.memoryFail", { error: mig.memory.error }));
  } else if (!isNoop(mig.memory) && mig.memory) {
    const { appended, skippedDuplicate } = mig.memory;
    parts.push(
      skippedDuplicate > 0
        ? t("wbAccounts.dialog.migratedMemory", { n: appended, d: skippedDuplicate })
        : t("wbAccounts.dialog.migratedMemorySimple", { n: appended }),
    );
  }

  if (hasError(mig.connectors)) {
    warnings.push(t("wbAccounts.dialog.connectorFail", { error: mig.connectors.error }));
  } else if (!isNoop(mig.connectors) && mig.connectors) {
    const { addedKeys, droppedDuplicateElements } = mig.connectors;
    parts.push(
      droppedDuplicateElements > 0
        ? t("wbAccounts.dialog.migratedConnectors", { k: addedKeys, d: droppedDuplicateElements })
        : t("wbAccounts.dialog.migratedConnectorsSimple", { k: addedKeys }),
    );
  }

  // 备份路径较长，只在确实产生备份时给出，供用户回滚。
  const backups: string[] = [];
  for (const section of [mig.memory, mig.connectors]) {
    if (section && !hasError(section) && section.backup) backups.push(section.backup);
  }
  if (backups.length) {
    parts.push(t("wbAccounts.dialog.backup", { path: backups.join(" / ") }));
  }

  return parts;
}
