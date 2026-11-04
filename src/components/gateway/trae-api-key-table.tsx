import { useCallback, useEffect, useState } from "react";
import { Copy, KeyRound, Loader2, RotateCcw, Trash2 } from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { DemoAction } from "@/components/demo-action";
import { KeySecretCell } from "@/components/gateway/key-secret-cell";
import { KeysBar } from "@/components/gateway/keys-bar";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import * as api from "@/lib/api";
import { copyText } from "@/lib/clipboard";
import { formatDate } from "@/lib/format";
import { useT } from "@/lib/i18n";
import { traeRegionLabelOf, traeVariantLabel } from "@/lib/trae-types";
import { allPrograms } from "@/lib/trae-variant-status";
import type { TraeApiKeyRecord, TraeVariantId } from "@/lib/trae-types";
import {
  KeyPermissionDrawer,
  KeyRowDetail,
  KeyScopeChip,
  KeyUsageCell,
  type KeyPermissionAccount,
} from "@/components/gateway/key-permission";
import type { KeyPolicy, KeyUsageSnapshot } from "@/lib/api";
import type { QuotaMode, QuotaPeriod, QuotaUnit, ScopeMode } from "@/lib/types";
import { cn } from "@/lib/utils";

/**
 * 归属取值集合 = **2 个变体**（每区域一个合并版客户端，issue #28 收敛；见
 * `allPrograms()` 的说明）。
 *
 * ## ⚠️ 2026-09-30 由「区域」改为「程序位」（issue #4 的实测结论）
 *
 * 2026-09-21 曾把它收成两个区域，理由是「程序位不决定 Key 能用哪些账号」——
 * 那句话在**账号**维度上仍然成立（池是区域级的，`pool.rs::sync_for` →
 * `entries_for_region`），但它**不完整**：程序位还决定两件事，
 * 而这两件事恰好是「Key 能不能调到某个模型」的全部：
 *
 * 1. `/v1/models` 列**哪份客户端清单**（TraeWork 与 TraeCode 是两份不同的缓存）；
 * 2. 请求体里的 **`function`** —— 上游按 function 做白名单（2026-09-30 实测），
 *    `solo_work_lite` 只认 TraeWork 那批，TraeCode 的 `glm-5.3-flash` 一律
 *    `4001 param is invalid`。
 *
 * ⇒ 只列区域时，**建不出 TraeCode 的 Key**，那条产品线的模型永远调不动
 * （正是 issue #4 后续报障里「看得见、调不动」的另一半）。
 *
 * 选项集合取自共享兜底表（`allPrograms()`，与账号卡片、模型卡同源），
 * **不在本文件里另立一份会漂移的清单**。
 */
const PROGRAMS = allPrograms();

/**
 * 标识 → **可选变体**：区域标识与四个退役标识一律落到该区域的合并版客户端。
 *
 * ★★ issue #28 收敛：每个区域只剩一个程序位（合并版），退役标识（`trae_work` /
 * `trae_cn` / `trae_code` / `global` / `global_trae_code`）在**老 Key 记录**里仍有，
 * 但它们现在全部指向合并版客户端（与 `traeVariantLabel` 的退役显示规则同向：
 * 显示 `TraeWork CN` 会指向一个已被卸载的客户端）。
 *
 * 用途有两处，都不能省：
 * 1. `Select` 的 value 必须是选项集合里的值，否则下拉显示空白（页面只有区域维度）；
 * 2. 归属列展示 —— 否则「两把行为完全相同的 Key」一个显示「国内版」、一个显示「TraeWork」。
 */
function normalizeProgramId(variant: TraeVariantId): TraeVariantId {
  switch (variant) {
    case "cn":
    case "trae":
    case "trae_cn":
    case "trae_code":
    case "trae_work":
      return "trae_merged";
    case "global":
    case "global_trae_code":
      return "global_trae_merged";
    default:
      return variant;
  }
}

/**
 * 归属列展示用：程序位名。
 *
 * 兜底走 `traeVariantLabel()` —— 认不出的取值宁可显示它的本名，
 * 也不要在界面上替后端发明一个程序位。
 */
function programLabelOf(variant: TraeVariantId): string {
  const normalized = normalizeProgramId(variant);
  return PROGRAMS.find((program) => program.variant === normalized)?.label
    ?? traeVariantLabel(variant);
}

/* P3：本地 `formatDate` 已移到 `@/lib/format`（本版是更健壮的那个，被取为基准）。 */

/**
 * Trae 多 Key 列表 + 创建 / 吊销 / 删除（对齐 WorkBuddy `gateway/api-key-table.tsx` 骨架）。
 *
 * ## 与 WorkBuddy 版的两处刻意差异
 *
 * 1. **「归属程序位」列的取值是程序位**（区域 × 程序，共 4 个）：Key 绑定 `variant`，
 *    由它同时决定**走哪个区域的账号池**（池是区域级的）与**列哪份客户端清单 + 请求体带
 *    哪个 `function`**（见 `PROGRAMS` 的说明）。展示走 `programLabelOf()`（程序位名），
 *    badge 的 `title` 里带归属区域。
 * 2. **不搬 `Region` / `useGatewayStore`**：那是 WorkBuddy 网关的 store 耦合。
 *    本组件自持数据（无 store），列表直接调 `list_trae_api_keys`。
 *
 * 明文只在创建时随响应返回一次（`create_trae_api_key`）；列表接口下发的是可空 `plaintext`
 * —— 用户选「保存到本软件」后才有值，才有眼睛可点，否则只给掩码。
 *
 * ## 2026-10-15 骨架对齐原型：卡头 → `KeysBar` 工具条 + 裸表
 *
 * 原型（`viewGateway()` 的 `keys` 分支）= **工具条 + 裸表 + 说明段**，没有卡头。
 * 本组件此前套了一整张 `<Card>`（卡头放「API Key」标题 + 创建按钮）：标题与左栏分区
 * 导航重复，已去掉；创建出口挪进 `KeysBar` 的 `children` —— 它与统一网关侧**共用同一个
 * 构件**，两个类型的密钥分区同形。三条产品线的差异仍只走数据与文案键。
 *
 * 原型表下的**说明段**（掩码口径 / 累计口径 / 吊销可逆）在表下方渲染，走**三条产品线
 * 共用**的键 `shared.gateway.keys.note`（各侧差异只在表里的「归属程序位」列）；
 * 空态时不附说明段（原型如此：空态只给 `emptyBlock`）。
 */
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

export function TraeApiKeyTable({
  className,
  /**
   * 创建时的默认归属（由页面传入当前 `?line=`，缺省 `cn` = 国内版）。
   *
   * 传进来的可能是**区域标识**（页面只有区域维度），故一律经
   * [`normalizeProgramId`] 归一成程序位 —— 否则 `Select` 的 value 与选项对不上，
   * 下拉会显示空白。
   */
  defaultVariant = "cn",
  /** 数据变化（创建 / 吊销 / 删除）后的回调，供页面刷新网关状态里的 Key 前缀。 */
  onChanged,
}: {
  className?: string;
  defaultVariant?: TraeVariantId;
  onChanged?: () => void;
}) {
  const t = useT();
  const [keys, setKeys] = useState<TraeApiKeyRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [createOpen, setCreateOpen] = useState(false);
  const [name, setName] = useState("");
  const [variant, setVariant] = useState<TraeVariantId>(() => normalizeProgramId(defaultVariant));
  /** 第三行：自定义密钥（留空 ⇒ 自动生成，填写 ⇒ 原样使用）。 */
  const [customKey, setCustomKey] = useState("");
  const [creating, setCreating] = useState(false);
  /**
   * 创建成功后的**二选一面板**（2026-11-04）：
   * 「保存到本软件」→ 明文落盘、列表出眼睛；「自行保存」→ 只有 hash。
   * 关闭面板（X / ESC / 点遮罩）等同「自行保存」。
   */
  const [pending, setPending] = useState<{ id: string; value: string; name: string } | null>(null);
  const [saving, setSaving] = useState(false);
  const [revokeTarget, setRevokeTarget] = useState<TraeApiKeyRecord | null>(null);
  /**
   * ★ 恢复目标：与 `revokeTarget` **互逆**，与 WorkBuddy 侧 `api-key-table.tsx` **同一规则** ——
   * 已吊销的行必须同时给出「恢复」与「删除」两条出口（吊销只置 `revoked_at`，明文未变）。
   */
  const [restoreTarget, setRestoreTarget] = useState<TraeApiKeyRecord | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<TraeApiKeyRecord | null>(null);
  const [busy, setBusy] = useState(false);

  /* ── issue #76：用量 / 权限两列 + 行内展开 + 权限抽屉（与 WB 侧同形） ── */
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [permTarget, setPermTarget] = useState<TraeApiKeyRecord | null>(null);
  const [permSaving, setPermSaving] = useState(false);
  const [usageMap, setUsageMap] = useState<Record<string, KeyUsageSnapshot>>({});

  /** 旧台账的 Key 没有策略字段 ⇒ 归一化成完整策略（缺省 = 改造前行为）。 */
  const policyOf = useCallback(
    (key: TraeApiKeyRecord): KeyPolicy => ({
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

  /* 账号选择器数据源：**当前程序位**的账号库（Trae 两条产品线账号分家）。
     ★ 展示名走 `accountDisplayName`（昵称 → 脱敏邮箱 → UID·x），不得裸回落 email。 */
  const [traeAccounts, setTraeAccounts] = useState<KeyPermissionAccount[]>([]);
  useEffect(() => {
    if (!permTarget) return;
    let cancelled = false;
    void (async () => {
      try {
        // `getTraeAccounts` 返回 `{ accounts, groups, ... }`（不是裸数组）——
        // 它内部把 variant 折成区域（账号数据是区域纯的）。
        const overview = await api.getTraeAccounts(permTarget.variant);
        if (cancelled) return;
        setTraeAccounts(
          (overview.accounts ?? [])
            .map((account) => ({
              // ★ Trae 的账号标识是 `userId`（不是 uid/id）；`name` 已是展示名，
              //   不含邮箱 ⇒ 无需再过 `accountDisplayName`（那是给 AccountMeta 用的）。
              id: account.userId,
              name: account.name,
              // Trae 的分组是 **id**（`groupId`），WB 是自由文本 `groupName` ——
              // 两者在 `scopeGroups` 里都只当「分组标识」用，语义一致。
              group: account.groupId ?? undefined,
            }))
            .filter((account) => account.id),
        );
      } catch {
        if (!cancelled) setTraeAccounts([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [permTarget]);

  /* 用量：**只拉「限额」的 Key**（不限额显示「不限」，拉它没意义）。 */
  useEffect(() => {
    const limited = keys.filter((key) => (key.quotaMode ?? "unlimited") === "limited");
    if (limited.length === 0) {
      setUsageMap({});
      return;
    }
    let cancelled = false;
    void (async () => {
      const pairs = await Promise.all(
        limited.map(async (key) => {
          try {
            return [key.id, await api.traeKeyUsage(key.id)] as const;
          } catch {
            return null;
          }
        }),
      );
      if (cancelled) return;
      const next: Record<string, KeyUsageSnapshot> = {};
      for (const pair of pairs) {
        if (pair) next[pair[0]] = pair[1];
      }
      setUsageMap(next);
    })();
    return () => {
      cancelled = true;
    };
  }, [keys]);

  async function onSavePolicy(next: KeyPolicy) {
    if (!permTarget) return;
    setPermSaving(true);
    try {
      await api.setTraeKeyPolicy(permTarget.id, next);
      setPermTarget(null);
      await load();
      toast.success(t("wbStats.keyPerm.saved"), { description: permTarget.name });
    } catch (e) {
      toast.error(t("wbStats.keyPerm.saveFail"), { description: api.asError(e) });
    } finally {
      setPermSaving(false);
    }
  }

  /** 拉取列表。创建 / 吊销 / 删除后都重新拉，保证列表与后端一致。 */
  const load = useCallback(async () => {
    setLoading(true);
    try {
      const result = await api.listTraeApiKeys();
      setKeys(result.keys ?? []);
    } catch (e) {
      // 演示模式 / 后端不可用：保持空列表，不清空已有数据。
      toast.error(t("trae.gateway.key.loadFailed"), { description: api.asError(e) });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  function openCreate() {
    setName("");
    setVariant(normalizeProgramId(defaultVariant));
    setCustomKey("");
    setCreateOpen(true);
  }

  async function onCreate() {
    const trimmed = name.trim();
    if (!trimmed) {
      toast.error(t("trae.gateway.key.nameRequired"));
      return;
    }
    setCreating(true);
    try {
      // 第三行可选自定义密钥：空 ⇒ 后端自动生成；非空 ⇒ 原样使用。
      const result = await api.createTraeApiKey(trimmed, variant, customKey.trim() || null);
      if (result.ok === false) {
        // 带上原因：自定义密钥撞车时后端会说清「被谁占了」，只报「创建失败」等于没说。
        toast.error(t("trae.gateway.key.createFailed"), { description: result.error });
        return;
      }
      // ★ 明文只随本次响应返回 ⇒ **先把面板打开，再刷新列表**。
      //   反过来写会让 load 的网络往返卡在「Key 已创建、面板还没弹」的中间态，
      //   期间切页 / 组件卸载就等于把明文丢了（`load` 自己吞异常，不会报错，丢了也不会有提示）。
      if (!result.record?.id || !result.key) {
        setCreateOpen(false);
        toast.error(t("trae.gateway.key.createFailed"));
        return;
      }
      setCreateOpen(false);
      setPending({ id: result.record.id, value: result.key, name: trimmed });
      // `load` 内部已把失败转成 toast，不会 reject ⇒ 列表刷新不会影响面板里的明文。
      await load();
      onChanged?.();
    } catch (e) {
      toast.error(t("trae.gateway.key.createFailed"), { description: api.asError(e) });
    } finally {
      setCreating(false);
    }
  }

  /** 面板「保存到本软件」：写台账 → 刷新列表让眼睛出现。失败时保留面板（关掉等于丢明文）。 */
  async function savePending() {
    if (!pending) return;
    setSaving(true);
    try {
      await api.saveTraeApiKeyPlaintext(pending.id, pending.value);
      setPending(null);
      // `load` 内部已把失败转成 toast（不 reject）⇒ 保存成功不会被刷新失败反报成失败。
      await load();
      onChanged?.();
      toast.success(t("shared.gateway.keys.savedToApp"), { description: pending.name });
    } catch (e) {
      // 失败必须留着面板：关掉就等于把明文丢了（保存没成功，就不该变成「自行保存」）。
      setPending(pending);
      toast.error(t("shared.gateway.keys.saveFailed"), { description: api.asError(e) });
    } finally {
      setSaving(false);
    }
  }

  /** 面板「自行保存」/ 关闭：不落盘，明文随面板一起消失。 */
  function dismissPending() {
    setPending(null);
  }

  async function confirmRevoke() {
    if (!revokeTarget) return;
    setBusy(true);
    try {
      await api.revokeTraeApiKey(revokeTarget.id);
      toast.success(t("trae.gateway.key.revoked"), { description: revokeTarget.name });
      setRevokeTarget(null);
      await load();
      onChanged?.();
    } catch (e) {
      toast.error(t("trae.gateway.key.revokeFailed"), { description: api.asError(e) });
    } finally {
      setBusy(false);
    }
  }

  async function confirmRestore() {
    if (!restoreTarget) return;
    setBusy(true);
    try {
      await api.restoreTraeApiKey(restoreTarget.id);
      toast.success(t("trae.gateway.key.restored"), { description: restoreTarget.name });
      setRestoreTarget(null);
      await load();
      onChanged?.();
    } catch (e) {
      toast.error(t("trae.gateway.key.restoreFailed"), { description: api.asError(e) });
    } finally {
      setBusy(false);
    }
  }

  async function confirmDelete() {
    if (!deleteTarget) return;
    setBusy(true);
    try {
      await api.deleteTraeApiKey(deleteTarget.id);
      toast.success(t("trae.gateway.key.deleted"), { description: deleteTarget.name });
      setDeleteTarget(null);
      await load();
      onChanged?.();
    } catch (e) {
      toast.error(t("trae.gateway.key.deleteFailed"), { description: api.asError(e) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className={cn(className)}>
      {/* 工具条与统一网关**共用同一个构件**：计数 + 说明 + 创建出口（此前只有空态才有出口）。 */}
      <KeysBar total={keys.length} live={keys.filter((key) => !key.revoked).length}>
        <DemoAction>
          <Button size="sm" onClick={openCreate}>
            <KeyRound />
            {t("trae.gateway.key.create")}
          </Button>
        </DemoAction>
      </KeysBar>

      {loading && keys.length === 0 ? (
        /* 区块**首次加载** ⇒ 用 `<Skeleton>` 占位（§3.6）。
           ⚠️ 此前这里是「文字型『加载中』+ `<Loader2>`」—— 那是**按钮操作中**态的语言，
           两者不可混用（本仓同一条规则在 `ApiServicePage` / `ModelList` 都有注释钉着）。 */
        <div className="space-y-2 py-4" aria-busy="true">
          <Skeleton className="h-4 w-40" />
          <Skeleton className="h-4 w-64" />
          <Skeleton className="h-4 w-52" />
        </div>
      ) : keys.length === 0 ? (
        <EmptyState icon={KeyRound} title={t("trae.gateway.key.empty")} className="py-4" />
      ) : (
        <div className="min-w-0 overflow-x-auto">
          <table className="w-full min-w-[640px] text-left text-sm">
            <thead>
              <tr className="text-xs text-muted-foreground">
                <th className="pb-2 pr-4 font-medium">{t("trae.gateway.key.name")}</th>
                <th className="pb-2 pr-4 font-medium">{t("trae.gateway.key.col.variant")}</th>
                <th className="pb-2 pr-4 font-medium">{t("trae.gateway.key.col.prefix")}</th>
                <th className="pb-2 pr-4 font-medium">{t("trae.gateway.key.col.createdAt")}</th>
                <th className="pb-2 pr-4 font-medium">{t("trae.gateway.key.col.lastUsed")}</th>
                {/* ★ issue #76：用量 / 权限两列（与 WB 侧同形，构件共用）。 */}
                <th className="pb-2 pr-4 font-medium">{t("wbStats.keyPerm.quota")}</th>
                <th className="pb-2 pr-4 font-medium">{t("wbStats.keyPerm.scope")}</th>
                <th className="pb-2 pr-4 font-medium">{t("trae.gateway.key.col.status")}</th>
                <th className="pb-2 font-medium">{t("trae.gateway.key.col.actions")}</th>
              </tr>
            </thead>
            <tbody>
              {keys.map((key) => {
                const revoked = key.revoked;
                const policy = policyOf(key);
                const expanded = expandedId === key.id;
                const scopeNames = policy.scopeAccounts.map(
                  (id) => traeAccounts.find((a) => a.id === id)?.name ?? id,
                );
                return (
                  <>
                  <tr
                    key={key.id}
                    className={cn("border-t border-border/60", expanded && "bg-primary/[0.07]")}
                    onClick={() => setExpandedId(expanded ? null : key.id)}
                  >
                    <td className="py-2 pr-4 font-medium">{key.name}</td>
                    <td className="py-2 pr-4">
                      <Badge
                        variant="secondary"
                        className="rounded-md"
                        title={traeRegionLabelOf(key.variant)}
                      >
                        {programLabelOf(key.variant)}
                      </Badge>
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
                    <td className="py-2 pr-4 text-xs text-muted-foreground">{formatDate(key.createdAt)}</td>
                    <td className="py-2 pr-4 text-xs text-muted-foreground">
                      {key.lastUsedAt ? formatDate(key.lastUsedAt) : t("trae.gateway.key.neverUsed")}
                    </td>
                    <td className="py-2 pr-4">
                      <KeyUsageCell policy={policy} usage={usageMap[key.id] ?? null} />
                    </td>
                    <td className="py-2 pr-4">
                      <KeyScopeChip policy={policy} onOpenScope={() => setPermTarget(key)} />
                    </td>
                    <td className="py-2 pr-4">
                      {revoked ? (
                        <Badge variant="secondary" className="rounded-md text-muted-foreground">
                          {t("trae.gateway.key.statusRevoked")}
                        </Badge>
                      ) : (
                        <Badge variant="success" className="rounded-md">
                          {t("trae.gateway.key.statusActive")}
                        </Badge>
                      )}
                    </td>
                    {/* 操作列挡冒泡：点吊销/恢复不顺手展开明细。 */}
                    <td className="py-2" onClick={(event) => event.stopPropagation()}>
                      {/* ★ 已吊销 ⇒ **两个出口都在**：恢复（可逆）+ 删除（不可逆）。
                          与 WorkBuddy 侧同形同序（恢复在前、删除在后）。 */}
                      {revoked ? (
                        <div className="flex items-center gap-1">
                          <Button variant="ghost" size="sm" onClick={() => setRestoreTarget(key)}>
                            <RotateCcw />
                            {t("trae.gateway.key.restore")}
                          </Button>
                          <Button
                            variant="ghost"
                            size="sm"
                            className="text-destructive hover:text-destructive"
                            onClick={() => setDeleteTarget(key)}
                          >
                            <Trash2 />
                            {t("trae.gateway.key.delete")}
                          </Button>
                        </div>
                      ) : (
                        <Button variant="ghost" size="sm" onClick={() => setRevokeTarget(key)}>
                          {t("trae.gateway.key.revoke")}
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

      {/* ★ issue #76：权限抽屉（改用量配额 + 可访问账号的唯一入口）。 */}
      <KeyPermissionDrawer
        open={permTarget !== null}
        onOpenChange={(open) => {
          if (!open) setPermTarget(null);
        }}
        keyName={permTarget?.name ?? ""}
        policy={permTarget ? policyOf(permTarget) : EMPTY_POLICY}
        usage={permTarget ? usageMap[permTarget.id] ?? null : null}
        accounts={traeAccounts}
        saving={permSaving}
        onSave={(next) => void onSavePolicy(next)}
      />

      {/* 说明段（原型 `<p class="tiny muted" style="margin:10px 2px 0">`）：掩码口径 /
          累计口径 / 吊销可逆。三条产品线共用一条键，差异只在表里的「归属程序位」列。 */}
      {keys.length > 0 && (
        <p className="mt-2.5 text-[11.5px] leading-5 text-muted-foreground">
          {t("shared.gateway.keys.note")}
        </p>
      )}

      {/* 创建对话框 */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("trae.gateway.key.create")}</DialogTitle>
            <DialogDescription>{t("trae.gateway.key.createDesc")}</DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="trae-key-name">{t("trae.gateway.key.name")}</Label>
              <Input
                id="trae-key-name"
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder={t("trae.gateway.key.namePlaceholder")}
                spellCheck={false}
                autoComplete="off"
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="trae-key-variant">{t("trae.gateway.key.variant")}</Label>
              <Select value={variant} onValueChange={(value) => setVariant(value as TraeVariantId)}>
                <SelectTrigger id="trae-key-variant" className="w-full" aria-label={t("trae.gateway.key.variant")}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {PROGRAMS.map((program) => (
                    <SelectItem key={program.variant} value={program.variant}>
                      {program.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            {/* ★ 第三行：可选自定义密钥（空 ⇒ 自动生成；非空 ⇒ 原样使用）。 */}
            <div className="space-y-2">
              <Label htmlFor="trae-key-custom">{t("shared.gateway.keys.customLabel")}</Label>
              <Input
                id="trae-key-custom"
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
            <Button variant="outline" onClick={() => setCreateOpen(false)} disabled={creating}>
              {t("trae.gateway.key.cancel")}
            </Button>
            <Button onClick={() => void onCreate()} disabled={creating}>
              {creating && <Loader2 className="animate-spin" />}
              {t("trae.gateway.key.createSubmit")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 创建后的二选一面板（2026-11-04）——与 WB 侧同一形态：
          创建时不落盘明文，明文只随本次响应回来，关掉即永久丢失。
          故给复制按钮兜底；X / ESC / 遮罩 = 「自行保存」，不拦截。 */}
      <Dialog open={pending !== null} onOpenChange={(open) => !open && dismissPending()}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("shared.gateway.keys.createdTitle")}</DialogTitle>
            <DialogDescription>{t("shared.gateway.keys.createdDesc")}</DialogDescription>
          </DialogHeader>
          {pending && (
            <div className="flex items-center gap-2 rounded-lg border border-border bg-muted/40 px-3 py-2.5">
              <code className="min-w-0 flex-1 select-all break-all font-mono text-xs">
                {pending.value}
              </code>
              <Button
                variant="outline"
                size="sm"
                data-act="gw-key-panel-copy"
                onClick={() => void copyText(pending.value)}
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
              onClick={dismissPending}
              disabled={saving}
            >
              {t("shared.gateway.keys.saveSelf")}
            </Button>
            <Button
              data-act="gw-key-panel-save"
              onClick={() => void savePending()}
              disabled={saving}
            >
              {saving && <Loader2 className="animate-spin" />}
              {t("shared.gateway.keys.saveToApp")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 吊销确认 */}
      <Dialog open={revokeTarget !== null} onOpenChange={(open) => !open && setRevokeTarget(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("trae.gateway.key.revokeTitle")}</DialogTitle>
            <DialogDescription>
              {t("trae.gateway.key.revokeDesc", { name: revokeTarget?.name ?? "" })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRevokeTarget(null)} disabled={busy}>
              {t("trae.gateway.key.cancel")}
            </Button>
            <Button variant="destructive" onClick={() => void confirmRevoke()} disabled={busy}>
              {t("trae.gateway.key.revoke")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 恢复确认（可逆动作 ⇒ 主按钮非 destructive） */}
      <Dialog open={restoreTarget !== null} onOpenChange={(open) => !open && setRestoreTarget(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("trae.gateway.key.restoreTitle")}</DialogTitle>
            <DialogDescription>
              {t("trae.gateway.key.restoreDesc", { name: restoreTarget?.name ?? "" })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRestoreTarget(null)} disabled={busy}>
              {t("trae.gateway.key.cancel")}
            </Button>
            <Button onClick={() => void confirmRestore()} disabled={busy}>
              {t("trae.gateway.key.restore")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 删除确认 */}
      <Dialog open={deleteTarget !== null} onOpenChange={(open) => !open && setDeleteTarget(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("trae.gateway.key.deleteTitle")}</DialogTitle>
            <DialogDescription>
              {t("trae.gateway.key.deleteDesc", { name: deleteTarget?.name ?? "" })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteTarget(null)} disabled={busy}>
              {t("trae.gateway.key.cancel")}
            </Button>
            <Button variant="destructive" onClick={() => void confirmDelete()} disabled={busy}>
              {t("trae.gateway.key.delete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
