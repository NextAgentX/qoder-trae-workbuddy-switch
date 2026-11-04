/**
 * 统一网关（第三个产品线）**跨作用域**的管理页 —— 网关页子导航第 7 项「统一网关」。
 *
 * 设计依据：`docs/specs/UNIFIED-GATEWAY-UI.md`（三张卡 / 状态矩阵 / 危险动作分档）。
 * 本组件落地**全部三张卡**：
 *   ① 统一网关（开关 / 监听 / 端口 / 命名空间启用集，按平台分组）
 *   ② 统一密钥（列表 + 创建 + 吊销 / 恢复 / 删除）
 *   ③ 命名空间与清单预览（逐段给出来源与原因 —— 非实时必有原因，一个总徽标糊过去是不允许的）
 *
 * ## ★ 它是**跨作用域**的
 *
 * 与「请求日志不做版本过滤」同性质：统一网关把两个产品线的模型合成一个入口，
 * 因此**上方的作用域选择器对它不生效**（卡 1 顶部有一行明确说明，否则用户切作用域没变化会当成 bug）。
 * 也因此这些命令**不进** `SCOPE_PARAMETERIZED`。
 *
 * ## 为什么状态放在组件内（`useState`）而不是 zustand
 *
 * 本页是**单点消费**（只有这一处渲染），且数据全部来自两条只读命令 + 三条写命令；
 * 引入一个 store 只会多一层需要同步的间接层。等出现第二个消费者时再提取 store
 * —— 与「命名空间镜像表」同一个判断标准。
 */

import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import { Copy, KeyRound, Loader2, Plus, RotateCcw, ShieldAlert, Trash2, Users } from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { DemoAction } from "@/components/demo-action";
import { KeySecretCell } from "@/components/gateway/key-secret-cell";
import { KeysBar } from "@/components/gateway/keys-bar";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Seg } from "@/components/ui/seg";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import * as api from "@/lib/api";
import { copyText } from "@/lib/clipboard";
import { gatewayRootUrl } from "@/lib/gateway-url";
import { formatDate } from "@/lib/format";
import { useT } from "@/lib/i18n";
import {
  KeyPermissionDrawer,
  KeyRowDetail,
  KeyScopeChip,
  KeyUsageCell,
  type KeyPermissionAccount,
} from "@/components/gateway/key-permission";
import type { KeyPolicy, KeyUsageSnapshot } from "@/lib/api";
import type { QuotaMode, QuotaPeriod, QuotaUnit, ScopeMode } from "@/lib/types";
import { accountDisplayName } from "@/components/account-card";
import { useAccountsStore } from "@/stores/accounts";
import { cn } from "@/lib/utils";

/* ─────────── 命名空间镜像表 ───────────
   与 `crates/buddy-switch-core/src/modules/gateway_namespace.rs` 的**表逐条对齐**。
   ⚠️ 只取**展示所需**的字段（分组用的平台名 + 客户端/程序位提示）；解析与合法性判定
   一律由后端负责 —— 前端**绝不**用它决定「这个前缀能不能用」（那是 fail-closed 的后端职责）。
   ⚠️ 派生层（前 4 条）由后端的 `Scope::all()` 生成；这里手写是**过渡**，等出现第二个消费者时
   提取为 `src/lib/gateway-namespaces.ts` 并加跨语言门禁（架构文档 §2.8 已登记）。 */
interface NamespaceMeta {
  id: string;
  /** 分组用（平台展示名）。 */
  platform: string;
  /** 提示用：客户端名 + 程序位。 */
  client: string;
  /** 是否属默认启用集（派生层）。 */
  defaultOn: boolean;
}

const NAMESPACE_META: readonly NamespaceMeta[] = [
  { id: "wb-cn", platform: "WorkBuddy", client: "WorkBuddy · 国内版", defaultOn: true },
  { id: "wb-global", platform: "WorkBuddy", client: "WorkBuddy · 国际版", defaultOn: true },
  { id: "trae-cn", platform: "Trae", client: "Trae CN（合并版）", defaultOn: true },
  { id: "trae-global", platform: "Trae", client: "Trae（合并版）", defaultOn: true },
  { id: "traework-cn", platform: "Trae", client: "TraeWork CN（老程序线）", defaultOn: false },
  { id: "traework-global", platform: "Trae", client: "TraeWork（老程序线）", defaultOn: false },
  { id: "traecode-cn", platform: "Trae", client: "TraeCode CN（遗留 IDE）", defaultOn: false },
  { id: "traecode-global", platform: "Trae", client: "TraeCode（遗留 IDE）", defaultOn: false },
  /* Qoder（第三个平台）的两个命名空间 —— 属**派生层**（平台 × 区域，由后端 `Scope::all()`
     生成），与上面四条一样进默认启用集。缺了这两条，后端下发的新命名空间会在界面上
     落到「其他」分组并显示原始 id（`metaOf` 的兜底），看起来像未知平台。 */
  { id: "qoder-cn", platform: "Qoder", client: "Qoder CN（桌面版）", defaultOn: true },
  { id: "qoder-global", platform: "Qoder", client: "Qoder（桌面版）", defaultOn: true },
];

/** 一条展示记录：镜像表里有则用它的说明，没有（后端新增了平台而前端还没跟上）则**原样显示 id** —— 不隐藏。 */
function metaOf(id: string): NamespaceMeta {
  return NAMESPACE_META.find((item) => item.id === id) ?? {
    id,
    platform: "其他",
    client: id,
    defaultOn: false,
  };
}

/** 平台展示顺序 = 镜像表里的出现顺序（新增平台时自动排到末尾，不会插队）。 */
function platformOrder(ids: readonly string[]): string[] {
  const seen: string[] = [];
  for (const id of ids) {
    const platform = metaOf(id).platform;
    if (!seen.includes(platform)) seen.push(platform);
  }
  return seen;
}

/**
 * 从密钥列表响应里取出**数组**。
 *
 * ## 为什么这里要容忍两种形状（实测事故）
 *
 * `list_unified_api_keys` 曾在桌面宿主返回**裸数组**、webui 宿主返回 `{ keys: [...] }`。
 * 前端按 `{ keys }` 取值时，拿到的不是 `undefined` 而是 **`Array.prototype.keys` 这个函数**
 * （数组的 `.keys` 是原型方法！）⇒ `setKeys(函数)` 被 React 当成 updater 调用
 * ⇒ `TypeError: Cannot convert undefined or null to object`，**整页白屏**。
 *
 * 形状现在由后端的 `unified::admin::keys_list_payload` **一处构造**保证一致；
 * 这里再做一次归一，是因为**形状漂移不该让整页白屏** —— 与「展示字段出口归一」
 * （`display_str` / `displayText`）同一条原则：脏值只该降级，不该炸树。
 */
function keysOf(payload: unknown): api.UnifiedApiKeyView[] {
  if (Array.isArray(payload)) return payload as api.UnifiedApiKeyView[];
  const inner = (payload as { keys?: unknown } | null | undefined)?.keys;
  return Array.isArray(inner) ? (inner as api.UnifiedApiKeyView[]) : [];
}

/** 代码样例里的模型 —— **带命名空间前缀**，这正是统一网关最容易踩的坑。 */
const SAMPLE_MODEL = "wb-cn:deepseek-v4.1-flash";

/**
 * 错误响应样例（原型 UNI_ERRORS）。
 *
 * 只放**能确认**的码与出路；不确定的一律不写（编错的错误码比没有更糟）。
 * 文案走 unifiedGateway.err.<key>.title|body，与抽屉逐条对应。
 *
 * ★ 2026-10-22：Trae 侧补齐 `/v1/responses`，原 `501 endpoint_not_supported_for_namespace`
 * 样例已删除（该码与 `LlmEndpoint::supports_trae()` 一起移除）。
 */
const UNIFIED_ERROR_SAMPLES = [
  { key: "bareModel", code: "400 namespace_prefix_required" },
  { key: "forbidden", code: "403 namespace_forbidden" },
  { key: "notEnabled", code: "400 namespace_not_enabled" },
  { key: "oldPort", code: "400 namespace_not_parsed_here" },
] as const;

/** 回环 / 局域网两个字面量（与三个既有网关页同款，避免各处写死字符串）。 */
const LOOPBACK = "127.0.0.1";
const LAN = "0.0.0.0";

/** 统一网关的分区（与 `GW_UNIFIED_SECTIONS` 一一对应）。 */
/** 空策略（抽屉未选中任何 Key 时的占位；不会被保存）。 */
const EMPTY_POLICY: KeyPolicy = {
  quotaMode: "unlimited",
  quotaLimit: 0,
  quotaUnit: "token",
  quotaPeriod: "none",
  scopeMode: "all",
  scopeAccounts: [],
  scopeGroups: [],
};

export type UnifiedGatewaySection = "overview" | "namespaces" | "keys" | "pool";

/**
 * 统一网关工作台 —— **按 `section` 只渲染一个分区**（导航归 `GatewayShell` 管）。
 *
 * ⚠️ 本组件**不持有导航状态**：分区真值在 URL（`?type=unified&sub=u-*`），
 * 由页面解析后以 `section` 传入。自持一份 state 就会出现「URL 与界面各说各话」。
 */
export function UnifiedGatewayView({
  section,
  onGoSection,
  className,
}: {
  section: UnifiedGatewaySection;
  /**
   * 分区之间的跳转（如「账号池」为空时引导去「命名空间」）。
   * ⚠️ **可选**：不传时相关出口**不渲染**，而不是渲染一枚点了没反应的按钮。
   */
  onGoSection?: (next: UnifiedGatewaySection) => void;
  className?: string;
}) {
  const t = useT();
  const [snapshot, setSnapshot] = useState<api.UnifiedGatewaySnapshot | null>(null);
  const [keys, setKeys] = useState<api.UnifiedApiKeyView[]>([]);
  /** 首屏加载：**只在从来没有数据时**用骨架 —— 保存后重读不该把用户正看着的配置换成骨架。 */
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [showMore, setShowMore] = useState(false);
  /** 端口输入框的**本地草稿**（提交前不写库；回车 / 失焦提交）。 */
  const [portDraft, setPortDraft] = useState("");
  const [createOpen, setCreateOpen] = useState(false);
  /**
   * 创建成功后的**二选一面板**（2026-11-04）：
   * 「保存到本软件」→ 明文落盘、列表出眼睛；「自行保存」→ 只有 hash。
   * 关闭面板（X / ESC / 点遮罩）等同「自行保存」。
   *
   * 不复用上面的 `saving`（那是配置保存的忙态）：两件事同时可能发生，
   * 共用一个布尔会让「保存配置」和「保存密钥」互相把按钮点亮 / 置灰。
   */
  const [pendingKey, setPendingKey] = useState<{ id: string; value: string; name: string } | null>(
    null,
  );
  const [keySaving, setKeySaving] = useState(false);

  /* ── issue #76：用量 / 权限两列 + 行内展开 + 权限抽屉（与另三条产品线同形） ── */
  const [expandedKeyId, setExpandedKeyId] = useState<string | null>(null);
  const [permTarget, setPermTarget] = useState<api.UnifiedApiKeyView | null>(null);
  const [permSaving, setPermSaving] = useState(false);
  const [usageMap, setUsageMap] = useState<Record<string, KeyUsageSnapshot>>({});

  /** 旧台账的 Key 没有策略字段 ⇒ 归一化成完整策略（缺省 = 改造前行为）。 */
  const policyOf = useCallback(
    (key: api.UnifiedApiKeyView): KeyPolicy => ({
      quotaMode: (key.quotaMode ?? "unlimited") as QuotaMode,
      quotaLimit: key.quotaLimit ?? 0,
      quotaUnit: (key.quotaUnit ?? "token") as QuotaUnit,
      quotaPeriod: (key.quotaPeriod ?? "none") as QuotaPeriod,
      scopeMode: (key.scopeMode ?? "all") as ScopeMode,
      scopeAccounts: key.scopeAccounts ?? [],
      scopeGroups: key.scopeGroups ?? [],
    }),
    [],
  );

  /* ★ 统一网关是**跨作用域**的：账号 uid 空间按**目标产品线**解读，
     而这里只能给出「当前作用域」的账号库 —— 抽屉文案里如实说明这一点
     （给一个空列表更糟：用户会以为「没有账号可选」）。 */
  const unifiedAccounts: KeyPermissionAccount[] = useMemo(() => {
    const rows = useAccountsStore.getState().accounts;
    return rows
      .map((account) => ({
        id: account.id ?? account.uid ?? "",
        name: accountDisplayName(account),
        group: typeof account.groupName === "string" ? account.groupName : undefined,
      }))
      .filter((account) => account.id);
  }, [permTarget]);

  /* 用量：**批量拉全部**（含不限额的 Key —— 分给同事的 key 即使不限额，
     你也想知道它被用了多少次）。一次请求而非 N 次往返。 */
  useEffect(() => {
    if (keys.length === 0) {
      setUsageMap({});
      return;
    }
    let cancelled = false;
    void (async () => {
      try {
        const all = await api.unifiedKeyUsageAll();
        if (!cancelled) setUsageMap(all ?? {});
      } catch {
        // 账本读失败不该让密钥表整块不可用 —— 静默降级成「不限」。
        if (!cancelled) setUsageMap({});
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [keys]);

  async function onSavePolicy(next: KeyPolicy) {
    if (!permTarget) return;
    setPermSaving(true);
    try {
      await api.setUnifiedKeyPolicy(permTarget.id, next);
      setPermTarget(null);
      await load();
      toast.success(t("wbStats.keyPerm.saved"), { description: permTarget.name });
    } catch (error) {
      toast.error(t("wbStats.keyPerm.saveFail"), { description: String(error) });
    } finally {
      setPermSaving(false);
    }
  }
  const [models, setModels] = useState<api.UnifiedModelsPreview | null>(null);
  /** 展开查看清单的命名空间（受控展开；不用 details/summary —— 本仓 AGENTS.md 禁止）。 */
  const [expandedNs, setExpandedNs] = useState<string[]>([]);
  /** 开放局域网的**危险确认**（与三个既有网关页同一文案、同一形态）。 */
  const [riskOpen, setRiskOpen] = useState(false);

  /**
   * 监听地址变更（下拉）。
   *
   * ★ **开放局域网是危险动作，必须两步确认** —— 与三个既有网关页**同一文案、同一形态**
   * （`ShieldAlert` + `destructive`）。改造前这里是一枚按钮、**点了直接生效**：
   * 同一件危险事（暴露到局域网 ⇒ 任何设备都能消耗你的额度）别处两步、这里一步，是安全缺陷。
   */
  function onBindChange(next: string) {
    if (!snapshot || next === snapshot.config.bind_addr) return;
    if (next === LOOPBACK) {
      void save({ bind_addr: LOOPBACK, allow_non_loopback: false });
      return;
    }
    setRiskOpen(true);
  }

  const load = useCallback(async () => {
    try {
      const [next, list, preview] = await Promise.all([
        api.getUnifiedGatewayConfig(),
        api.listUnifiedApiKeys(),
        api.getUnifiedGatewayModels(),
      ]);
      setSnapshot(next);
      setKeys(keysOf(list));
      setModels(preview);
      setPortDraft(String(next.config.port));
    } catch (error) {
      toast.error(t("unifiedGateway.loadFailed"), { description: String(error) });
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    void load();
  }, [load]);

  /**
   * 账号池总览（**跨平台只读汇总**）。与主快照**分开取**：只有「账号池」分区与命名空间表用得上，
   * 且失败不该把整页拖成错误态 —— 那两处显示「—」即可（**未知 ≠ 零**，本仓硬规矩）。
   */
  const [pools, setPools] = useState<api.UnifiedGatewayPool[] | null>(null);
  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const data = await api.getUnifiedGatewayPoolOverview();
        if (alive) setPools(data.pools);
      } catch {
        /* 静默：消费点用「—」表达「未知」，不把页面变成错误态。 */
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  /** 接入说明的协议 Tab（与独立网关同形：卡头一枚 Seg）。 */
  const [guideApi, setGuideApi] = useState<"openai" | "anthropic">("openai");
  /** 「看错误响应」抽屉。 */
  const [errorsOpen, setErrorsOpen] = useState(false);
  /** 展开「看池」的命名空间（与 `expandedNs` 分开记：两个展开是**不同的问题**）。 */
  const [poolOpen, setPoolOpen] = useState<string[]>([]);

  /**
   * 该命名空间背后的池。
   *
   * ⚠️ 用后端 `pools[].namespaces` **反查**，**不在前端另建「命名空间 → 池」映射表** ——
   * 那是后端 `relay_target_of` 的职责，写死一份必然漂移。
   * ⚠️ `pools === null`（未到 / 取失败）⇒ `undefined`，消费点一律显示「—」而**不是 0**
   * （未知 ≠ 零，本仓硬规矩）。
   */
  function poolOf(id: string): api.UnifiedGatewayPool | undefined {
    return pools?.find((pool) => pool.namespaces.includes(id));
  }

  /**
   * 该命名空间的**可用性** —— **四态，顺序即优先级**：
   * 未启用 → 池里没账号 → 池里没有可用账号 → 没识别到模型 → 可调用。
   *
   * ⚠️ 「已识别」只是清单识别过，**不等于可用** ⇒ 两个判据分开给，不合成一个总徽标。
   * ⚠️ 池数据未到 ⇒ `null`，消费点**不渲染**徽标（不给一个假的「可调用」）。
   */
  function healthOf(id: string): { key: string; tone: "ok" | "warn" | "bad" | "muted" } | null {
    if (!enabledIds.includes(id)) return { key: "disabled", tone: "muted" };
    const pool = poolOf(id);
    if (!pool) return null;
    if (pool.total === 0) return { key: "noAccount", tone: "bad" };
    if (pool.available === 0) return { key: "noAvailable", tone: "bad" };
    const rows = (models?.data ?? []).filter((row) => row["namespace"] === id);
    if (rows.length === 0) return { key: "noModels", tone: "warn" };
    return { key: "ready", tone: "ok" };
  }

  /**
   * 概览的**一句话结论** —— 用户不该自己把「N 个命名空间 / M 个可用账号」翻译成「现在能不能用」。
   *
   * 判据顺序（**顺序即优先级**）：未启用 → 没启用任何命名空间 → 池数据未到 → 全部不可用 →
   * 部分不可用 → 可用。与命名空间的「可用性」列**同源**（同一份 `pools`），不另算一套。
   * ⚠️ 池数据未到（`pools === null`）时**不猜** —— 给中性态「已启用」，而**不是**谎报「可用」。
   */
  function verdictOf(): {
    key: string;
    tone: "ok" | "warn" | "bad" | "muted" | "neutral";
    count: number;
  } {
    if (!snapshot?.config.enabled) return { key: "disabled", tone: "muted", count: 0 };
    if (enabledIds.length === 0) return { key: "noNamespace", tone: "warn", count: 0 };
    if (!pools) return { key: "enabled", tone: "neutral", count: 0 };
    const dead = enabledIds.filter((id) => {
      const pool = poolOf(id);
      return !pool || pool.available === 0;
    });
    if (dead.length === enabledIds.length) return { key: "dead", tone: "bad", count: dead.length };
    if (dead.length > 0) return { key: "partial", tone: "warn", count: dead.length };
    return { key: "ready", tone: "ok", count: 0 };
  }

  /** 保存配置：**先落库再更新内存态** —— 否则行会「假装切了」（状态矩阵里的既有约定）。 */
  const save = useCallback(
    async (patch: Partial<api.UnifiedGatewayConfigView>) => {
      if (!snapshot) return;
      const next = { ...snapshot.config, ...patch };
      setSaving(true);
      try {
        const result = await api.saveUnifiedGatewayConfig(next);
        setSnapshot({ ...snapshot, config: result.config, running: result.running, addr: result.addr });
        setPortDraft(String(result.config.port));
      } catch (error) {
        toast.error(t("unifiedGateway.saveFailed"), { description: String(error) });
        // 失败 ⇒ 回滚显示值（不留下「看起来改了」的假象）
        setPortDraft(String(snapshot.config.port));
      } finally {
        setSaving(false);
      }
    },
    [snapshot, t],
  );

  const enabledIds = snapshot?.namespaces ?? [];
  const allIds = snapshot?.allNamespaces ?? NAMESPACE_META.map((item) => item.id);
  const visibleIds = useMemo(
    () => (showMore ? allIds : allIds.filter((id) => metaOf(id).defaultOn)),
    [allIds, showMore],
  );
  const platforms = useMemo(() => platformOrder(visibleIds), [visibleIds]);
  const activeKeyCount = keys.filter((key) => !key.revoked).length;
  /**
   * 根地址（**不带** `/v1`）—— Anthropic 系客户端要的就是它。
   * ★ 两档语义由 `lib/gateway-url.ts` 唯一给出（#165）：`0.0.0.0` 是**监听**通配地址，
   * 不能当目标地址 ⇒ 展示 / 复制一律归一回环（与 Trae / Qoder 两页同款）。
   */
  const rootUrl = useMemo(
    () => gatewayRootUrl(snapshot?.config.bind_addr, snapshot?.config.port),
    [snapshot?.config.bind_addr, snapshot?.config.port],
  );
  /** OpenAI 系 Base URL（**含** `/v1`）—— 概览「接入地址」显示并复制的就是它。 */
  const baseUrl = `${rootUrl}/v1`;
  /**
   * 端口缺失（配置加载失败 / 尚未就绪）⇒ 不给出「看起来能用」的串：
   * 否则会渲染成 `http://127.0.0.1:/v1`，复制出去必然失败。
   */
  const endpointReady = Boolean(snapshot?.config.port);
  /** 概览的一句话结论（见 `verdictOf`）。 */
  const verdict = verdictOf();

  /** 接入说明的代码样例（两种协议的认证头不同 —— 把协议做成 Tab，认证头就不可能给错）。
      ★ 端点路径自带 `/v1` ⇒ 这里一律用**根地址**拼（用 `baseUrl` 会得到 `/v1/v1/...`）。 */
  const guideSnippet =
    guideApi === "anthropic"
      ? `curl ${rootUrl}/v1/messages \\
  -H "x-api-key: $BUDDY_SWITCH_KEY" \\
  -H "anthropic-version: 2023-06-01" \\
  -d '{"model":"${SAMPLE_MODEL}","max_tokens":256,"messages":[{"role":"user","content":"hi"}]}'`
      : `curl ${rootUrl}/v1/chat/completions \\
  -H "Authorization: Bearer $BUDDY_SWITCH_KEY" \\
  -d '{"model":"${SAMPLE_MODEL}","messages":[{"role":"user","content":"hi"}]}'`;

  /** 勾选 / 取消一个命名空间。★ 守卫：**至少保留一个**（取消最后一条时给 toast 并还原）。 */
  function toggleNamespace(id: string, next: boolean) {
    if (!snapshot) return;
    if (!next && enabledIds.length <= 1) {
      toast.error(t("unifiedGateway.keepOne"));
      return;
    }
    const nextIds = next
      ? allIds.filter((item) => item === id || enabledIds.includes(item))
      : enabledIds.filter((item) => item !== id);
    void save({ namespaces: nextIds });
  }

  function commitPort() {
    if (!snapshot) return;
    const parsed = Number.parseInt(portDraft, 10);
    if (!Number.isFinite(parsed) || parsed < 1 || parsed > 65535) {
      toast.error(t("unifiedGateway.portInvalid"));
      setPortDraft(String(snapshot.config.port));
      return;
    }
    if (parsed === snapshot.config.port) return;
    void save({ port: parsed });
  }

  async function onCreateKey(
    name: string,
    namespaces: string[],
    bizNamespace: string | null,
    customKey: string | null,
  ) {
    try {
      const result = await api.createUnifiedApiKey(name, namespaces, bizNamespace, customKey);
      if (result.ok === false) {
        // 失败时**不关对话框**：用户填的名称 / 授权集 / 自定义密钥都还在，改完可直接重试。
        toast.error(t("unifiedGateway.saveFailed"), { description: result.error });
        return;
      }
      // ★ 明文只随本次响应返回 ⇒ **先把面板打开，再刷新列表**。
      //   反过来写会让 load 的往返卡在「Key 已创建、面板还没弹」的中间态，
      //   期间切页 / 组件卸载就等于把明文丢了（`load` 自己吞异常，不会报错，丢了也不会有提示）。
      if (!result.record?.id || !result.key) {
        setCreateOpen(false);
        toast.error(t("unifiedGateway.saveFailed"));
        return;
      }
      setCreateOpen(false);
      setPendingKey({ id: result.record.id, value: result.key, name });
      // `load` 内部已把失败转成 toast，不会 reject ⇒ 列表刷新不会影响面板里的明文。
      await load();
    } catch (error) {
      toast.error(t("unifiedGateway.saveFailed"), { description: String(error) });
    }
  }

  /** 面板「保存到本软件」：写台账 → 刷新列表让眼睛出现。失败时保留面板（关掉等于丢明文）。 */
  async function savePendingKey() {
    if (!pendingKey) return;
    setKeySaving(true);
    try {
      await api.saveUnifiedApiKeyPlaintext(pendingKey.id, pendingKey.value);
      setPendingKey(null);
      // `load` 内部已把失败转成 toast（不 reject）⇒ 保存成功不会被刷新失败反报成失败。
      await load();
      toast.success(t("shared.gateway.keys.savedToApp"), { description: pendingKey.name });
    } catch (error) {
      // 失败必须留着面板：关掉就等于把明文丢了（保存没成功，就不该变成「自行保存」）。
      setPendingKey(pendingKey);
      toast.error(t("shared.gateway.keys.saveFailed"), { description: String(error) });
    } finally {
      setKeySaving(false);
    }
  }

  /** 面板「自行保存」/ 关闭：不落盘，明文随面板一起消失。 */
  function dismissPendingKey() {
    setPendingKey(null);
  }

  async function runKeyAction(action: "revoke" | "restore" | "delete", id: string) {
    try {
      if (action === "revoke") await api.revokeUnifiedApiKey(id);
      else if (action === "restore") await api.restoreUnifiedApiKey(id);
      else await api.deleteUnifiedApiKey(id);
      await load();
    } catch (error) {
      toast.error(t("unifiedGateway.saveFailed"), { description: String(error) });
    }
  }

  if (loading && !snapshot) {
    return (
      <div className={cn("space-y-4", className)}>
        <div className="h-[120px] animate-pulse rounded-xl bg-muted" />
        <div className="h-[240px] animate-pulse rounded-xl bg-muted" />
      </div>
    );
  }

  return (
    <div className={cn("space-y-4", className)}>
      {/* ── 分区 ① · 概览 ── */}
      {section === "overview" && (
      <>
      {/* ── 卡 1 · 统一网关 ── */}
      <Card className="gap-0 py-0">
        <div className="flex items-center justify-between gap-3 border-b border-border/60 px-5 py-3">
          <span className="text-sm font-semibold">{t("unifiedGateway.title")}</span>
          {/* ★ 「一句话结论」而不是「运行中 / 已停用」—— 后者只回答「进程在不在」，
              前者回答用户真正问的「现在能不能用」。判据与命名空间「可用性」列同源。 */}
          <Badge
            variant={
              verdict.tone === "ok"
                ? "success"
                : verdict.tone === "bad"
                  ? "destructive"
                  : verdict.tone === "warn"
                    ? "secondary"
                    : "outline"
            }
            className="rounded-md"
            title={t(
              `unifiedGateway.verdict.${verdict.key}Hint` as "unifiedGateway.verdict.readyHint",
            )}
          >
            {verdict.key === "partial"
              ? t("unifiedGateway.verdict.partial", { count: verdict.count })
              : t(`unifiedGateway.verdict.${verdict.key}` as "unifiedGateway.verdict.ready")}
          </Badge>
        </div>

        {/* 跨作用域说明：不写这行，用户切作用域没变化会当成 bug */}
        <div className="border-b border-border/60 bg-muted/40 px-5 py-2.5 text-xs text-muted-foreground">
          {t("unifiedGateway.crossScope")}
        </div>

        {/* 用哪个网关：两个类型的**分工**说清楚，避免用户在两个入口之间反复试。 */}
        <div className="border-b border-border/60 px-5 py-2.5 text-xs text-muted-foreground">
          {t("unifiedGateway.whichGateway")}
        </div>

        {/* 未启用态：关着开关时改配置，用户不确定有没有生效 ⇒ 如实说「会保存，启用后生效」。 */}
        {!snapshot?.config.enabled && (
          <div className="border-b border-border/60 px-5 py-2.5 text-xs text-warning">
            {t("unifiedGateway.notEnabled")}
          </div>
        )}

        {/* 接入地址（与两个既有网关卡同构的一行：值 + 复制出口）——
            改造前这一行只在摘要条里露了个 `addr`，没有完整 URL、也没有复制。
            ★ 值必须是**能直接填进 OpenAI 客户端**的串（含 `/v1`），与下方接入说明同口径（#165）。 */}
        <div className="flex items-center gap-3 border-b border-border/60 px-5 py-3">
          <div className="min-w-0 flex-1">
            <div className="text-xs font-medium">{t("unifiedGateway.addr")}</div>
            <div className="mt-0.5 break-all font-mono text-[11.5px] text-muted-foreground">
              {endpointReady ? baseUrl : "—"}
            </div>
          </div>
          <Button
            variant="outline"
            size="sm"
            disabled={!endpointReady}
            onClick={() => void copyText(baseUrl, t("wbStats.gateway.baseUrlCopied"))}
          >
            <Copy />
            {t("unifiedGateway.copyBaseUrl")}
          </Button>
        </div>

        <div className="flex items-center gap-3 border-b border-border/60 px-5 py-3">
          <div className="min-w-0 flex-1">
            <div className="text-xs font-medium">{t("unifiedGateway.enable")}</div>
            <div className="mt-0.5 text-[11.5px] text-muted-foreground">
              {t("unifiedGateway.enableHint")}
            </div>
          </div>
          {/* ★ 「开关开着 ≠ 端口上有监听」（issue #52：重启后只 manage 不 apply，
              开关是绿的、57892 却没人 accept）。所以把**真实监听状态**摆在开关旁边——
              与「独立网关」页 `wbStats.gateway.running` 同一套视觉，用户不必去 netstat。 */}
          {snapshot?.config.enabled && (
            <span
              className="flex items-center gap-1.5 text-xs"
              title={t(
                snapshot.running
                  ? "unifiedGateway.listeningHint"
                  : "unifiedGateway.notListeningHint",
              )}
            >
              <span
                className={cn(
                  "size-2 rounded-full",
                  snapshot.running ? "bg-success" : "bg-destructive",
                )}
              />
              <span
                className={cn(
                  "font-medium",
                  snapshot.running ? "text-success" : "text-destructive",
                )}
              >
                {t(
                  snapshot.running
                    ? "unifiedGateway.listening"
                    : "unifiedGateway.notListening",
                )}
              </span>
            </span>
          )}
          <Switch
            checked={Boolean(snapshot?.config.enabled)}
            disabled={saving}
            onCheckedChange={(next) => void save({ enabled: next })}
            aria-label={t("unifiedGateway.enable")}
          />
        </div>

        <div className="flex items-center gap-3 border-b border-border/60 px-5 py-3">
          <div className="min-w-0 flex-1">
            <div className="text-xs font-medium">{t("unifiedGateway.bindAddr")}</div>
            <div className="mt-0.5 font-mono text-[11.5px] text-muted-foreground">
              {snapshot?.config.bind_addr}
            </div>
          </div>
          {/* ★ 与三个既有网关页**同形**：下拉（不是按钮切换）+ 开放局域网走危险确认。
              按钮只能表达「下一步动作」，下拉能同时表达「当前值」。 */}
          <Select
            value={snapshot?.config.bind_addr ?? LOOPBACK}
            onValueChange={onBindChange}
            disabled={saving}
          >
            <SelectTrigger size="sm" className="w-52" aria-label={t("unifiedGateway.bindAddr")}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={LOOPBACK}>{t("unifiedGateway.loopback")}</SelectItem>
              <SelectItem value={LAN}>{t("unifiedGateway.lan")}</SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div className="flex items-center gap-3 border-b border-border/60 px-5 py-3">
          <div className="min-w-0 flex-1">
            <div className="text-xs font-medium">{t("unifiedGateway.port")}</div>
            <div className="mt-0.5 text-[11.5px] text-muted-foreground">
              {t("unifiedGateway.portHint")}
            </div>
          </div>
          <Input
            className="w-[86px] font-mono text-xs"
            value={portDraft}
            disabled={saving}
            inputMode="numeric"
            onChange={(event) => setPortDraft(event.target.value.replace(/[^\d]/g, ""))}
            onBlur={commitPort}
            onKeyDown={(event) => {
              if (event.key === "Enter") commitPort();
            }}
            aria-label={t("unifiedGateway.port")}
          />
        </div>
        {/* 裸模型名：统一网关**必须**带命名空间前缀，这是最容易踩的坑 ⇒ 给错误样例出口 */}
        <div className="flex items-center gap-3 px-5 py-3">
          <div className="min-w-0 flex-1">
            <div className="text-xs font-medium">{t("unifiedGateway.bareModel")}</div>
            <div className="mt-0.5 text-[11.5px] text-muted-foreground">
              {t("unifiedGateway.bareModelHint")}
            </div>
          </div>
          <Button variant="outline" size="sm" onClick={() => setErrorsOpen(true)}>
            {t("unifiedGateway.showErrors")}
          </Button>
        </div>
      </Card>
      {/* ── 接入说明 ──
          ★ 与「独立网关」的接入说明**同形**：卡头放协议 Tab，卡体放代码样例 + 说明段落。
          ★ 样例里的模型名**必须**带命名空间前缀 —— 这正是统一网关最容易踩的坑。 */}
      <Card className="gap-0 py-0">
        <div className="flex items-center justify-between gap-3 border-b border-border/60 px-5 py-3">
          <span className="text-sm font-semibold">{t("unifiedGateway.guideTitle")}</span>
          <Seg<"openai" | "anthropic">
            value={guideApi}
            onChange={setGuideApi}
            options={[
              { value: "openai", label: t("unifiedGateway.guideOpenai") },
              { value: "anthropic", label: t("unifiedGateway.guideAnthropic") },
            ]}
            ariaLabel={t("unifiedGateway.guideTitle")}
          />
        </div>
        <div className="px-5 py-3">
          <pre className="overflow-x-auto rounded-[10px] bg-muted p-3 font-mono text-[11.5px] leading-5">
            {guideSnippet}
          </pre>
          <p className="mt-2.5 text-[11.5px] leading-5 text-muted-foreground">
            {t("unifiedGateway.guideBase", { openai: baseUrl, anthropic: rootUrl })}
          </p>
          <p className="mt-1.5 text-[11.5px] leading-5 text-muted-foreground">
            {t("unifiedGateway.guideModels", { sample: SAMPLE_MODEL })}
          </p>
          <p className="mt-1.5 text-[11.5px] leading-5 text-muted-foreground">
            {t("unifiedGateway.guideResponses")}
          </p>
          <p className="mt-1.5 text-[11.5px] leading-5 text-muted-foreground">
            {t("unifiedGateway.guideKey")}
          </p>
        </div>
      </Card>
      </>
      )}

      {/* ── 分区 ② · 命名空间 ──
          勾选与逐段状态**在同一分区里**（设计文档 §2 的结论：拆开就要「改一处看两个地方」）。 */}
      {section === "namespaces" && (
      <>
      <Card className="gap-0 py-0">
        {/* 命名空间启用集：**按平台分组遍历渲染** —— 新增平台/命名空间时前端零改动 */}
        <div className="px-5 py-3">
          <div className="text-xs font-medium">{t("unifiedGateway.nsTitle")}</div>
          <div className="mt-0.5 text-[11.5px] text-muted-foreground">
            {t("unifiedGateway.nsHint")}
          </div>
          <div className="mt-3 space-y-3">
            {platforms.map((platform) => (
              <div key={platform}>
                <div className="mb-1.5 text-[11px] font-semibold text-muted-foreground">
                  {platform}
                </div>
                <div className="grid grid-cols-[repeat(auto-fill,minmax(190px,1fr))] gap-x-4 gap-y-1.5">
                  {visibleIds
                    .filter((id) => metaOf(id).platform === platform)
                    .map((id) => {
                      const meta = metaOf(id);
                      const checked = enabledIds.includes(id);
                      return (
                        <label
                          key={id}
                          className="flex cursor-pointer items-center gap-2 text-xs"
                          title={meta.client}
                        >
                          <Checkbox
                            checked={checked}
                            disabled={saving}
                            onCheckedChange={(next) => toggleNamespace(id, next === true)}
                          />
                          <code className="font-mono text-[11.5px]">{id}</code>
                          {/* 停用的命名空间在密钥表里被标红 —— 这里不给额外徽标，避免两处重复提示 */}
                        </label>
                      );
                    })}
                </div>
              </div>
            ))}
          </div>
          <Button
            variant="ghost"
            size="sm"
            className="mt-2"
            onClick={() => setShowMore((value) => !value)}
          >
            {showMore ? t("unifiedGateway.showLess") : t("unifiedGateway.showMore")}
          </Button>
        </div>
      </Card>
      {/* ── 卡 3 · 命名空间与清单预览 ──
          逐段给出**来源与原因**（非实时必有原因）—— 一个总徽标糊过去是不允许的：
          用户要能看出哪段是实时、哪段是兜底、为什么。 */}
      <Card className="gap-0 py-0">
        <div className="flex items-center justify-between gap-3 border-b border-border/60 px-5 py-3">
          <span className="text-sm font-semibold">{t("unifiedGateway.previewTitle")}</span>
          <span className="text-xs text-muted-foreground">
            {t("unifiedGateway.previewTotal", { count: models?.data.length ?? 0 })}
          </span>
        </div>
        <div className="divide-y divide-border/60">
          {enabledIds.length === 0 && (
            <div className="px-5 py-4 text-xs text-muted-foreground">
              {t("unifiedGateway.previewEmpty")}
            </div>
          )}
          {enabledIds.map((id) => {
            const meta = metaOf(id);
            const catalog = models?.namespaces[id];
            const expanded = expandedNs.includes(id);
            const rows = (models?.data ?? []).filter((row) => row["namespace"] === id);
            const pool = poolOf(id);
            const poolExpanded = poolOpen.includes(id);
            const health = healthOf(id);
            return (
              <div key={id} className="px-5 py-3">
                <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                  <code className="font-mono text-xs font-medium">{id}</code>
                  <span className="text-[11.5px] text-muted-foreground" title={meta.client}>
                    {meta.client}
                  </span>
                  {catalog && (
                    <Badge
                      variant={
                        catalog.source === "live"
                          ? "success"
                          : catalog.source === "cached"
                            ? "secondary"
                            : "destructive"
                      }
                      className="rounded-md"
                    >
                      {t(`unifiedGateway.source.${catalog.source}` as "unifiedGateway.source.live")}
                    </Badge>
                  )}
                  <span className="text-[11.5px] text-muted-foreground">
                    {t("unifiedGateway.previewCount", { count: rows.length })}
                  </span>

                  {/* ── 「账号池」：该段背后的池（跨平台只读汇总的一部分）。
                      ⚠️ 池数据未到时显示「—」而**不是「0 可用」**（未知 ≠ 零）。 */}
                  {pool ? (
                    <span className="flex items-center gap-1.5">
                      <Badge
                        variant={pool.available > 0 ? "secondary" : "destructive"}
                        className="rounded-md"
                      >
                        {t("unifiedGateway.pool.available", { count: pool.available })}
                      </Badge>
                      <span className="text-[11.5px] text-muted-foreground">
                        {t("unifiedGateway.nsPoolOf", { total: pool.total, target: pool.target })}
                      </span>
                      {/* 空池不给「看池」（无明细可展开）—— 说明在下面的展开块与说明段里 */}
                      {pool.total > 0 && (
                        <Button
                          variant="ghost"
                          size="sm"
                          aria-expanded={poolExpanded}
                          onClick={() =>
                            setPoolOpen((list) =>
                              list.includes(id) ? list.filter((item) => item !== id) : [...list, id],
                            )
                          }
                        >
                          {poolExpanded
                            ? t("unifiedGateway.pool.hide")
                            : t("unifiedGateway.pool.show")}
                        </Button>
                      )}
                    </span>
                  ) : (
                    <span className="text-[11.5px] text-muted-foreground">—</span>
                  )}

                  {/* ── 「可用性」：**四态，顺序即优先级**。这才是「能不能调通」的判据 ——
                      「已识别」只是清单识别过，**不等于可用**。 */}
                  {health && (
                    <Badge
                      variant={
                        health.tone === "ok"
                          ? "success"
                          : health.tone === "warn"
                            ? "secondary"
                            : health.tone === "bad"
                              ? "destructive"
                              : "outline"
                      }
                      className="rounded-md"
                      title={t(
                        `unifiedGateway.health.${health.key}Hint` as "unifiedGateway.health.readyHint",
                      )}
                    >
                      {t(`unifiedGateway.health.${health.key}` as "unifiedGateway.health.ready")}
                    </Badge>
                  )}
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() =>
                      setExpandedNs((list) =>
                        list.includes(id) ? list.filter((item) => item !== id) : [...list, id],
                      )
                    }
                  >
                    {expanded ? t("unifiedGateway.hideModels") : t("unifiedGateway.showModels")}
                  </Button>
                </div>
                {/* ★ 非实时必有原因：把接口给的原因**原样透出**，不在这里另编一套解释 */}
                {catalog?.note && (
                  <div className="mt-1 text-[11.5px] text-muted-foreground">{catalog.note}</div>
                )}
                {expanded && (
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    {rows.length === 0 ? (
                      <span className="text-[11.5px] text-muted-foreground">
                        {t("unifiedGateway.previewNoModels")}
                      </span>
                    ) : (
                      rows.map((row) => (
                        <code
                          key={String(row["id"])}
                          className="rounded-md border border-border bg-muted/40 px-1.5 py-0.5 font-mono text-[11px]"
                        >
                          {String(row["id"])}
                        </code>
                      ))
                    )}
                  </div>
                )}
                {/* 「看池」**就地展开** —— 不跳走：跳走会丢掉本页**唯一**的价值：跨平台视野。 */}
                {poolExpanded && pool && (
                  <div className="mt-2 rounded-md border border-border bg-muted/30 px-2.5 py-2">
                    {pool.accounts.length === 0 ? (
                      <span className="text-[11.5px] text-muted-foreground">
                        {t("unifiedGateway.pool.noAccount")}
                      </span>
                    ) : (
                      <table className="w-full text-left text-[11.5px]">
                        <thead className="text-muted-foreground">
                          <tr>
                            <th scope="col" className="py-1 pr-3 font-medium">
                              {t("unifiedGateway.pool.account")}
                            </th>
                            <th scope="col" className="py-1 pr-3 font-medium">
                              {t("unifiedGateway.pool.credits")}
                            </th>
                            <th scope="col" className="py-1 pr-3 font-medium">
                              {t("unifiedGateway.pool.state")}
                            </th>
                          </tr>
                        </thead>
                        <tbody>
                          {pool.accounts.map((account) => (
                            <tr key={account.uid} className="border-t border-border/60">
                              <td className="py-1 pr-3">{account.name}</td>
                              <td className="py-1 pr-3 tabular-nums">
                                {/* ★ 不可知时给「—」，**不给 0**（未知 ≠ 零）。 */}
                                {account.credits === null ? "—" : account.credits}
                              </td>
                              <td className="py-1 pr-3">{account.status}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    )}
                    <p className="mt-1.5 text-[11px] text-muted-foreground">
                      {t("unifiedGateway.pool.readOnly")}
                    </p>
                  </div>
                )}              </div>
            );
          })}
        </div>
      </Card>
      </>
      )}

      {/* ── 分区 ③ · 密钥 ── */}
      {section === "keys" && (
      <>
      {/* ── 卡 2 · 统一密钥 ── */}
      <div>
        {/* ★ 与「独立网关」的密钥分区**共用同一个工具条**（`KeysBar`），不再自己写卡头。
            ★ `DemoAction`：演示模式下拦住写操作 —— 独立网关侧三个密钥表都有，这里此前**漏了**，
              于是演示模式下统一网关的「创建密钥」是真能点的（不一致）。 */}
        <KeysBar total={keys.length} live={activeKeyCount}>
          <DemoAction>
            <Button size="sm" onClick={() => setCreateOpen(true)}>
              <Plus />
              {t("unifiedGateway.createKey")}
            </Button>
          </DemoAction>
        </KeysBar>

        <div className="min-w-0">
          {keys.length === 0 ? (
            <EmptyState
              icon={KeyRound}
              title={t("unifiedGateway.noKeys")}
              description={t("unifiedGateway.noKeysHint")}
              className="py-4"
            />
          ) : (
            <div className="min-w-0 overflow-x-auto">
              <table className="w-full min-w-[720px] text-left text-sm">
                <thead>
                  <tr className="text-xs text-muted-foreground">
                    <th className="pb-2 pr-4 font-medium">{t("unifiedGateway.colName")}</th>
                    <th className="pb-2 pr-4 font-medium">{t("unifiedGateway.colNamespaces")}</th>
                    <th className="pb-2 pr-4 font-medium">{t("unifiedGateway.colMasked")}</th>
                    <th className="pb-2 pr-4 font-medium">{t("unifiedGateway.colCreated")}</th>
                    <th className="pb-2 pr-4 font-medium">{t("unifiedGateway.colLastUsed")}</th>
                    {/* ★ issue #76：用量 / 权限两列（与另三条产品线同形，构件共用）。
                        注意顺序：**命名空间在最左**（最外层授权）→ 账号范围 → 配额，
                        与规范 §6.1 一致。 */}
                    <th className="pb-2 pr-4 font-medium">{t("wbStats.keyPerm.quota")}</th>
                    <th className="pb-2 pr-4 font-medium">{t("wbStats.keyPerm.scope")}</th>
                    <th className="pb-2 pr-4 font-medium">{t("unifiedGateway.colStatus")}</th>
                    <th className="pb-2 font-medium">{t("unifiedGateway.colActions")}</th>
                  </tr>
                </thead>
                <tbody>
                  {keys.map((key) => {
                    const policy = policyOf(key);
                    const expanded = expandedKeyId === key.id;
                    const scopeNames = policy.scopeAccounts.map(
                      (id) => unifiedAccounts.find((a) => a.id === id)?.name ?? id,
                    );
                    return (
                    <>
                    <tr
                      key={key.id}
                      className={cn(
                        "border-t border-border/60",
                        key.revoked && "opacity-60",
                        expanded && "bg-primary/[0.07]",
                      )}
                      onClick={() => setExpandedKeyId(expanded ? null : key.id)}
                    >
                      <td className="py-2 pr-4 font-medium">{key.name}</td>
                      <td className="py-2 pr-4">
                        {/* 授权集里的命名空间**逐条标状态**：已停用的标红并说明（跨卡联动的落点） */}
                        <span className="flex flex-wrap gap-1">
                          {key.namespaces.map((id) => {
                            const off = !enabledIds.includes(id);
                            return (
                              <Badge
                                key={id}
                                variant={off ? "destructive" : "secondary"}
                                className="rounded-md whitespace-nowrap"
                                title={off ? t("unifiedGateway.nsOff") : undefined}
                              >
                                {id}
                              </Badge>
                            );
                          })}
                          {key.bizNamespace && (
                            <Badge variant="outline" className="rounded-md whitespace-nowrap">
                              /biz → {key.bizNamespace}
                            </Badge>
                          )}
                        </span>
                      </td>
                      {/* ★ 掩码 + 眼睛（四条产品线共用 `KeySecretCell`）：
                          默认 `prefix••••••••suffix`，有明文时可点眼睛查看完整密钥。 */}
                      <td className="py-2 pr-4">
                        <KeySecretCell
                          plaintext={key.plaintext}
                          prefix={key.prefix}
                          suffix={key.suffix}
                        />
                      </td>
                      <td className="py-2 pr-4 text-xs text-muted-foreground">
                        {formatDate(key.createdAt)}
                      </td>
                      <td className="py-2 pr-4 text-xs text-muted-foreground">
                        {key.lastUsedAt ? formatDate(key.lastUsedAt) : t("unifiedGateway.neverUsed")}
                      </td>
                      <td className="py-2 pr-4">
                        <KeyUsageCell policy={policy} usage={usageMap[key.id] ?? null} />
                      </td>
                      <td className="py-2 pr-4">
                        <KeyScopeChip policy={policy} onOpenScope={() => setPermTarget(key)} />
                      </td>
                      <td className="py-2 pr-4">
                        <Badge
                          variant={key.revoked ? "secondary" : "success"}
                          className="rounded-md"
                        >
                          {key.revoked ? t("unifiedGateway.revoked") : t("unifiedGateway.active")}
                        </Badge>
                      </td>
                      {/* 操作列挡冒泡：点吊销/恢复不顺手展开明细。 */}
                      <td className="py-2" onClick={(event) => event.stopPropagation()}>
                        {/* ★ 已吊销 ⇒ **两个出口都在**：恢复（可逆，同一把 Key）+ 删除（不可逆）。
                            只留删除 = 逼用户为一次误吊销删掉重建、再改所有客户端配置。恢复在前。 */}
                        {key.revoked ? (
                          <span className="flex items-center gap-1">
                            <Button
                              variant="ghost"
                              size="sm"
                              onClick={() => void runKeyAction("restore", key.id)}
                            >
                              <RotateCcw />
                              {t("unifiedGateway.restore")}
                            </Button>
                            <Button
                              variant="ghost"
                              size="sm"
                              className="text-destructive hover:text-destructive"
                              onClick={() => void runKeyAction("delete", key.id)}
                            >
                              <Trash2 />
                              {t("unifiedGateway.delete")}
                            </Button>
                          </span>
                        ) : (
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => void runKeyAction("revoke", key.id)}
                          >
                            {t("unifiedGateway.revoke")}
                          </Button>
                        )}
                      </td>
                    </tr>
                    {expanded ? (
                      <tr key={`${key.id}-detail`} className="border-t border-border/60">
                        <td colSpan={9} className="p-0">
                          <KeyRowDetail
                            policy={policy}
                            usage={usageMap[key.id] ?? null}
                            accountNames={scopeNames}
                            onEditQuota={() => setPermTarget(key)}
                            onEditScope={() => setPermTarget(key)}
                          />
                        </td>
                      </tr>
                    ) : null}
                    </>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
        </div>
        {/* ★ issue #76：权限抽屉（改用量配额 + 可访问账号的唯一入口）。
            ⚠️ 命名空间授权**不在这里**（它是另一个维度，见台账注释）。
            ✅ 配额**双口径都生效**：本网关的 Key 被收敛成 `KeyActor` 带进目标产品线的
               中继，判定 / 白名单 / token 记账与三条产品线完全同一份实现。 */}

        <KeyPermissionDrawer
          open={permTarget !== null}
          onOpenChange={(open) => {
            if (!open) setPermTarget(null);
          }}
          keyName={permTarget?.name ?? ""}
          policy={permTarget ? policyOf(permTarget) : EMPTY_POLICY}
          usage={permTarget ? usageMap[permTarget.id] ?? null : null}
          accounts={unifiedAccounts}
          saving={permSaving}
          onSave={(next) => void onSavePolicy(next)}
        />

        {/* 说明段：与独立网关三张密钥表**同一段话**（抽到 `shared.gateway.keys.note`）。 */}
        <p className="mt-2.5 text-[11.5px] leading-5 text-muted-foreground">
          {t("shared.gateway.keys.note")}
        </p>
      </div>
      </>
      )}

      {/* ── 分区 ④ · 账号池 ──
          统一网关**没有自己的池** —— 它按模型名的命名空间前缀把请求分派到三条产品线各自的池，
          所以这一区是**跨平台的只读汇总**（独立网关那一区是「管理当前这一个池」）。
          ⚠️ 选号 / 粘性 / 恢复等**管理动作按产品线设**，本区只读 —— 摆一个改不动的控件就是假的。
          ⚠️ 数据未到（`pools === null`）时给骨架，**不给「0 可用」**（未知 ≠ 零）。 */}
      {section === "pool" && (
        <>
          {pools === null ? (
            <Skeleton className="h-24 w-full rounded-[10px]" />
          ) : pools.length === 0 ? (
            <EmptyState
              icon={Users}
              title={t("unifiedGateway.pool.empty")}
              description={t("unifiedGateway.pool.emptyHint")}
              action={
                /* 出口只在调用方传了 `onGoSection` 时渲染 —— 点了没反应的按钮比不摆更糟。 */
                onGoSection ? (
                  <Button size="sm" onClick={() => onGoSection("namespaces")}>
                    {t("unifiedGateway.subnav.namespaces")}
                  </Button>
                ) : undefined
              }
            />
          ) : (
            <>
              <div className="mb-3 flex flex-wrap items-center gap-2.5 rounded-[10px] bg-muted px-3 py-2.5">
                <Badge variant="secondary" className="rounded-md">
                  {t("unifiedGateway.pool.count", { count: pools.length })}
                </Badge>
                <span className="text-[11.5px] text-muted-foreground">
                  {t("unifiedGateway.pool.totals", {
                    available: pools.reduce((sum, pool) => sum + pool.available, 0),
                    total: pools.reduce((sum, pool) => sum + pool.total, 0),
                  })}
                </span>
                <div className="grow" />
                <span className="text-[11.5px] text-muted-foreground">
                  {t("unifiedGateway.pool.readOnly")}
                </span>
              </div>

              <div className="min-w-0 overflow-x-auto">
                <table className="w-full min-w-[560px] text-left text-sm">
                  <thead className="text-[11.5px] text-muted-foreground">
                    <tr>
                      <th scope="col" className="py-2 pr-4 font-medium">
                        {t("unifiedGateway.pool.account")}
                      </th>
                      <th scope="col" className="py-2 pr-4 font-medium">
                        {t("unifiedGateway.pool.credits")}
                      </th>
                      <th scope="col" className="py-2 pr-4 font-medium">
                        {t("unifiedGateway.pool.state")}
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {pools.map((pool) => (
                      <Fragment key={pool.target}>
                        {/* 分组行：池标识 + 指向它的已启用命名空间 + 五态计数**摊平**。
                            多条命名空间共用一个池时只出一条（后端已按池分组）。 */}
                        <tr className="bg-muted/40 text-[11.5px] text-muted-foreground">
                          <td colSpan={3} className="py-2 pr-4">
                            <div className="flex flex-wrap items-center gap-2">
                              <code className="font-mono">{pool.target}</code>
                              <span>{pool.namespaces.join(" / ")}</span>
                              <Badge
                                variant={pool.available > 0 ? "secondary" : "destructive"}
                                className="rounded-md"
                              >
                                {t("unifiedGateway.pool.available", { count: pool.available })}
                              </Badge>
                              <span>{t("unifiedGateway.pool.total", { count: pool.total })}</span>
                              {pool.cooling > 0 && (
                                <span>{t("unifiedGateway.pool.cooling", { count: pool.cooling })}</span>
                              )}
                              {pool.disabled > 0 && (
                                <span>{t("unifiedGateway.pool.disabled", { count: pool.disabled })}</span>
                              )}
                              {(pool.expired ?? 0) > 0 && (
                                <span>{t("unifiedGateway.pool.expired", { count: pool.expired ?? 0 })}</span>
                              )}
                              {(pool.zero_credits ?? 0) > 0 && (
                                <span>{t("unifiedGateway.pool.zeroCredits", { count: pool.zero_credits ?? 0 })}</span>
                              )}
                              {(pool.model_cooling ?? 0) > 0 && (
                                <span>{t("unifiedGateway.pool.modelCooling", { count: pool.model_cooling ?? 0 })}</span>
                              )}
                              {/* 在途占满也是**正交**计数（已含在 `available` 里）：
                                  不标出来，「4 个可用」就会被读成「现在能跑 4 个」——
                                  而请求可能直接被 503 `pool_busy` 挡回。 */}
                              {(pool.in_flight_full ?? 0) > 0 && (
                                <span title={t("unifiedGateway.pool.inFlightFullHint")}>
                                  {t("unifiedGateway.pool.inFlightFull", { count: pool.in_flight_full ?? 0 })}
                                </span>
                              )}
                            </div>
                          </td>
                        </tr>
                        {pool.accounts.length === 0 ? (
                          <tr>
                            <td colSpan={3} className="py-2 pr-4 text-[11.5px] text-muted-foreground">
                              {t("unifiedGateway.pool.noAccount")}
                            </td>
                          </tr>
                        ) : (
                          pool.accounts.map((account) => (
                            <tr key={account.uid} className="border-t border-border/60">
                              <td className="py-2 pr-4">{account.name}</td>
                              <td className="py-2 pr-4 tabular-nums">
                                {/* ★ 不可知时给「—」，**不给 0**（未知 ≠ 零）。 */}
                                {account.credits === null ? "—" : account.credits}
                              </td>
                              <td className="py-2 pr-4">
                                <Badge
                                  variant={account.status === "available" ? "secondary" : "outline"}
                                  className="rounded-md"
                                >
                                  {account.status}
                                </Badge>
                              </td>
                            </tr>
                          ))
                        )}
                      </Fragment>
                    ))}
                  </tbody>
                </table>
              </div>

              <p className="mt-2.5 text-[11.5px] leading-5 text-muted-foreground">
                {t("unifiedGateway.pool.note")}
              </p>
            </>
          )}
        </>
      )}


      {/* ★ 错误响应样例抽屉：**每一条都带出路** —— 只说「不支持」会让人反复试错。
          统一网关的坑集中在「模型名要带前缀」，所以这里逐条给出「怎么改」。 */}
      <Dialog open={errorsOpen} onOpenChange={setErrorsOpen}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>{t("unifiedGateway.errorsTitle")}</DialogTitle>
            <DialogDescription>{t("unifiedGateway.errorsHint")}</DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            {UNIFIED_ERROR_SAMPLES.map((sample) => (
              <div key={sample.code} className="rounded-[10px] border border-border bg-muted/30 px-3 py-2.5">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-xs font-medium">
                    {t(`unifiedGateway.err.${sample.key}.title` as "unifiedGateway.err.bareModel.title")}
                  </span>
                  <Badge variant="destructive" className="rounded-md font-mono">
                    {sample.code}
                  </Badge>
                </div>
                <p className="mt-1.5 whitespace-pre-line text-[11.5px] leading-5 text-muted-foreground">
                  {t(`unifiedGateway.err.${sample.key}.body` as "unifiedGateway.err.bareModel.body")}
                </p>
              </div>
            ))}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setErrorsOpen(false)}>
              {t("unifiedGateway.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <CreateKeyDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        namespaces={allIds}
        onConfirm={onCreateKey}
      />

      {/* 创建后的二选一面板（2026-11-04）——与另三条产品线同一形态：
          创建时不落盘明文，明文只随本次响应回来，关掉即永久丢失。
          故给复制按钮兜底；X / ESC / 遮罩 = 「自行保存」，不拦截。 */}
      <Dialog open={pendingKey !== null} onOpenChange={(open) => !open && dismissPendingKey()}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("shared.gateway.keys.createdTitle")}</DialogTitle>
            <DialogDescription>{t("shared.gateway.keys.createdDesc")}</DialogDescription>
          </DialogHeader>
          {pendingKey && (
            <div className="flex items-center gap-2 rounded-lg border border-border bg-muted/40 px-3 py-2.5">
              <code className="min-w-0 flex-1 select-all break-all font-mono text-xs">
                {pendingKey.value}
              </code>
              <Button
                variant="outline"
                size="sm"
                data-act="gw-key-panel-copy"
                onClick={() => void copyText(pendingKey.value)}
              >
                <Copy />
                {t("shared.gateway.keys.copy")}
              </Button>
            </div>
          )}
          <DialogFooter>
            <Button
              variant="outline"
              data-act="gw-key-panel-dismiss"
              onClick={dismissPendingKey}
              disabled={keySaving}
            >
              {t("shared.gateway.keys.saveSelf")}
            </Button>
            <Button
              data-act="gw-key-panel-save"
              onClick={() => void savePendingKey()}
              disabled={keySaving}
            >
              {keySaving && <Loader2 className="animate-spin" />}
              {t("shared.gateway.keys.saveToApp")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* ★ 非回环监听的危险确认 —— 与三个既有网关页**同一文案、同一形态**
          （`ShieldAlert` + `destructive` 确认按钮）。少了它，暴露局域网就是**一键生效**。 */}
      <Dialog open={riskOpen} onOpenChange={setRiskOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <ShieldAlert className="size-4 text-warning" />
              {t("unifiedGateway.allowLanTitle")}
            </DialogTitle>
            <DialogDescription>{t("unifiedGateway.allowLanDesc")}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRiskOpen(false)}>
              {t("unifiedGateway.cancel")}
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                setRiskOpen(false);
                // 开非回环必须同时置真，否则后端会拒绝（安全默认）
                void save({ bind_addr: LAN, allow_non_loopback: true });
              }}
            >
              {t("unifiedGateway.confirmOpen")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

/** 创建统一密钥（名称 + 命名空间多选 + 业务转发单选）。 */
function CreateKeyDialog({
  open,
  onOpenChange,
  namespaces,
  onConfirm,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  namespaces: readonly string[];
  onConfirm: (
    name: string,
    namespaces: string[],
    bizNamespace: string | null,
    customKey: string | null,
  ) => Promise<void>;
}) {
  const t = useT();
  const [name, setName] = useState("");
  /* 第三行：自定义密钥（留空 ⇒ 自动生成，填写 ⇒ 原样使用）。 */
  const [customKey, setCustomKey] = useState("");
  const [selected, setSelected] = useState<string[]>([]);
  const [biz, setBiz] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // 每次打开都重置：上一次的残留选择会让「新建」意外继承旧授权
  useEffect(() => {
    if (!open) return;
    setName("");
    setCustomKey("");
    setSelected([]);
    setBiz(null);
  }, [open]);

  /** 业务转发候选 = **已勾选里属于 WorkBuddy 的那些**（Trae 业务域不可配）。 */
  const bizOptions = selected.filter((id) => metaOf(id).platform === "WorkBuddy");

  function toggleNamespace(id: string, next: boolean) {
    const updated = next ? [...selected, id] : selected.filter((item) => item !== id);
    setSelected(updated);
    // 业务转发必须落在仍被授权的 WB 命名空间里 ⇒ 失效就清空
    if (biz && !updated.includes(biz)) setBiz(null);
  }

  async function submit() {
    if (selected.length === 0) return;
    setBusy(true);
    try {
      await onConfirm(
        name.trim() || t("unifiedGateway.unnamed"),
        selected,
        biz,
        customKey.trim() || null,
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("unifiedGateway.createKey")}</DialogTitle>
          <DialogDescription>{t("unifiedGateway.noKeysHint")}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="unified-key-name">{t("unifiedGateway.keyName")}</Label>
            <Input
              id="unified-key-name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder={t("unifiedGateway.keyNamePlaceholder")}
              spellCheck={false}
              autoComplete="off"
            />
          </div>

          <div className="space-y-2">
            <Label>{t("unifiedGateway.authorize")}</Label>
            <div className="space-y-3">
              {platformOrder(namespaces).map((platform) => (
                <div key={platform}>
                  <div className="mb-1.5 text-[11px] font-semibold text-muted-foreground">
                    {platform}
                  </div>
                  <div className="grid grid-cols-[repeat(auto-fill,minmax(190px,1fr))] gap-x-4 gap-y-1.5">
                    {namespaces
                      .filter((id) => metaOf(id).platform === platform)
                      .map((id) => (
                        <label
                          key={id}
                          className="flex cursor-pointer items-center gap-2 text-xs"
                          title={metaOf(id).client}
                        >
                          <Checkbox
                            checked={selected.includes(id)}
                            onCheckedChange={(next) => toggleNamespace(id, next === true)}
                          />
                          <code className="font-mono text-[11.5px]">{id}</code>
                        </label>
                      ))}
                  </div>
                </div>
              ))}
            </div>
            {selected.length === 0 && (
              <p className="text-[11.5px] text-muted-foreground">
                {t("unifiedGateway.authorizeRequired")}
              </p>
            )}
          </div>

          <div className="space-y-2">
            <Label htmlFor="unified-key-biz">{t("unifiedGateway.bizTitle")}</Label>
            <Select
              value={biz ?? "__none__"}
              onValueChange={(value) => setBiz(value === "__none__" ? null : value)}
            >
              <SelectTrigger id="unified-key-biz" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none__">{t("unifiedGateway.bizNone")}</SelectItem>
                {bizOptions.map((id) => (
                  <SelectItem key={id} value={id}>
                    {id}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-[11.5px] text-muted-foreground">{t("unifiedGateway.bizHint")}</p>
          </div>

          {/* ★ 末尾「自定义密钥」：留空 ⇒ 按 `sk-uni-` 规则自动生成；填写 ⇒ 原样使用。 */}
          <div className="space-y-2">
            <Label htmlFor="unified-key-custom">{t("shared.gateway.keys.customLabel")}</Label>
            <Input
              id="unified-key-custom"
              value={customKey}
              onChange={(event) => setCustomKey(event.target.value)}
              placeholder={t("shared.gateway.keys.customPlaceholder")}
              spellCheck={false}
              autoComplete="off"
              className="font-mono"
            />
            <p className="text-[11.5px] leading-5 text-muted-foreground">
              {t("shared.gateway.keys.customHint")}
            </p>
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            {t("unifiedGateway.cancel")}
          </Button>
          <Button onClick={() => void submit()} disabled={busy || selected.length === 0}>
            {busy && <Loader2 className="animate-spin" />}
            {t("unifiedGateway.create")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
