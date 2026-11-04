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
import { normalizeQoderApiKeys, type QoderApiKeyRecord } from "@/lib/qoder-gateway";
import { REGIONS, regionDescriptor } from "@/lib/region";
import { scopeOf } from "@/lib/scope";
import type { Region } from "@/lib/types";
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
 * Qoder 多 Key 列表 + 创建 / 吊销 / 恢复 / 删除（对齐 Trae `trae-api-key-table.tsx` 骨架）。
 *
 * ## 与 Trae 版的唯一差别：归属轴是**区域**而不是程序位
 *
 * Qoder 没有程序位轴（一个区域一本账号库、一套端点），网关**按 Key 的归属区域选池**
 * ⇒ Key 的归属就是区域，共两个取值（国内版 / 国际版）。这不是「少了一个维度」，
 * 而是「这个平台的维度就是区域」：把程序位选择器搬过来会是**假控件**。
 *
 * ## 三态动作（与另两条产品线**同形**）
 *
 * **吊销**（置 `revokedAt`，可逆）→ 已吊销的行**必须同时给「恢复」与「删除」两个出口**；
 * **恢复**（清 `revokedAt`，同一把 Key 明文未变 ⇒ 客户端无需改配置）；
 * **删除**（仅已吊销可删，物理删除）。只给删除等于逼用户为一次误吊销删掉重建。
 *
 * 明文只在创建时随响应返回一次（`create_qoder_api_key`）；列表接口下发可空 `plaintext`
 * —— 用户选「保存到本软件」后才有值，才有眼睛可点，否则只有掩码（前缀 + 末 4 位）。
 *
 * ## 2026-10-15 骨架对齐原型：卡头 → `KeysBar` 工具条 + 裸表
 *
 * 原型（`viewGateway()` 的 `keys` 分支）= **工具条 + 裸表 + 说明段**，没有卡头。
 * 本组件此前套了一整张 `<Card>`（卡头放「API Key」标题 + 创建按钮）：标题与左栏分区
 * 导航重复，已去掉；创建出口挪进 `KeysBar` 的 `children`（三条产品线**共用同一个构件**）。
 *
 * 原型表下的**说明段**（掩码口径 / 累计口径 / 吊销可逆）在表下方渲染，走**三条产品线
 * 共用**的键 `shared.gateway.keys.note`（各侧差异只在表里的「归属」列）；
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

export function QoderApiKeyTable({
  className,
  /** 创建时的默认归属（由页面传入当前作用域的区域；缺省 `cn` = 国内版）。 */
  defaultRegion = "cn",
  /** 数据变化（创建 / 吊销 / 恢复 / 删除）后的回调，供页面刷新网关状态里的 Key 前缀。 */
  onChanged,
}: {
  className?: string;
  defaultRegion?: Region;
  onChanged?: () => void;
}) {
  const t = useT();
  const [keys, setKeys] = useState<QoderApiKeyRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [createOpen, setCreateOpen] = useState(false);
  const [name, setName] = useState("");
  const [region, setRegion] = useState<Region>(defaultRegion);
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
  const [revokeTarget, setRevokeTarget] = useState<QoderApiKeyRecord | null>(null);
  const [restoreTarget, setRestoreTarget] = useState<QoderApiKeyRecord | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<QoderApiKeyRecord | null>(null);
  const [busy, setBusy] = useState(false);

  /* ── issue #76：用量 / 权限两列 + 行内展开 + 权限抽屉（与 WB / Trae 同形） ── */
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [permTarget, setPermTarget] = useState<QoderApiKeyRecord | null>(null);
  const [permSaving, setPermSaving] = useState(false);
  const [usageMap, setUsageMap] = useState<Record<string, KeyUsageSnapshot>>({});
  const [qoderAccounts, setQoderAccounts] = useState<KeyPermissionAccount[]>([]);

  /** 旧台账的 Key 没有策略字段 ⇒ 归一化成完整策略（缺省 = 改造前行为）。 */
  const policyOf = useCallback(
    (key: QoderApiKeyRecord): KeyPolicy => ({
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

  /* 账号选择器：**当前 Key 归属区域**的账号库（Qoder 两个区域分家）。
     ★ 展示名走 `accountDisplayName` 的等价口径：`nickname → 脱敏邮箱`由后端给
     （`QoderAccountView.name` 已是展示名）；分组是自由文本 `group`。 */
  useEffect(() => {
    if (!permTarget) return;
    let cancelled = false;
    void (async () => {
      try {
        const view = await api.getQoderAccounts(
          `qoder-${permTarget.region}` as Parameters<typeof api.getQoderAccounts>[0],
        );
        if (cancelled) return;
        setQoderAccounts(
          (view.accounts ?? [])
            .map((account) => ({
              id: account.uid,
              name: account.name || account.uid,
              group: account.group ?? undefined,
            }))
            .filter((account) => account.id),
        );
      } catch {
        if (!cancelled) setQoderAccounts([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [permTarget]);

  /* 用量：只拉「限额」的 Key。 */
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
            return [key.id, await api.qoderKeyUsage(key.id)] as const;
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
      await api.setQoderKeyPolicy(permTarget.id, next);
      setPermTarget(null);
      await load();
      toast.success(t("wbStats.keyPerm.saved"), { description: permTarget.name });
    } catch (e) {
      toast.error(t("wbStats.keyPerm.saveFail"), { description: api.asError(e) });
    } finally {
      setPermSaving(false);
    }
  }

  /** 拉取列表。创建 / 吊销 / 恢复 / 删除后都重新拉，保证列表与后端一致。 */
  const load = useCallback(async () => {
    setLoading(true);
    try {
      const result = await api.listQoderApiKeys();
      setKeys(normalizeQoderApiKeys(result));
    } catch (e) {
      // 演示模式 / 后端不可用：保持空列表，不清空已有数据。
      toast.error(t("qoder.gateway.key.loadFailed"), { description: api.asError(e) });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  function openCreate() {
    setName("");
    setRegion(defaultRegion);
    setCustomKey("");
    setCreateOpen(true);
  }

  async function onCreate() {
    const trimmed = name.trim();
    if (!trimmed) {
      toast.error(t("qoder.gateway.key.nameRequired"));
      return;
    }
    setCreating(true);
    try {
      // Key 的归属是**区域**，但命令面统一收 `scope`（与另两条产品线同一纪律：
      // 作用域是唯一的区域载体）。这里把当前区域升成 `qoder-*` 作用域。
      // 第三行可选自定义密钥：空 ⇒ 后端自动生成；非空 ⇒ 原样使用。
      const result = await api.createQoderApiKey(
        scopeOf("qoder", region),
        trimmed,
        customKey.trim() || null,
      );
      if (result.ok === false) {
        // 带上原因：自定义密钥撞车时后端会说清「被谁占了」。
        toast.error(t("qoder.gateway.key.createFailed"), { description: result.error });
        return;
      }
      // ★ 明文只随本次响应返回 ⇒ **先把面板打开，再刷新列表**。
      //   反过来写会让 load 的网络往返卡在「Key 已创建、面板还没弹」的中间态，
      //   期间切页 / 组件卸载就等于把明文丢了（`load` 自己吞异常，不会报错，丢了也不会有提示）。
      if (!result.record?.id || !result.key) {
        setCreateOpen(false);
        toast.error(t("qoder.gateway.key.createFailed"));
        return;
      }
      setCreateOpen(false);
      setPending({ id: result.record.id, value: result.key, name: trimmed });
      // `load` 内部已把失败转成 toast，不会 reject ⇒ 列表刷新不会影响面板里的明文。
      await load();
      onChanged?.();
    } catch (e) {
      toast.error(t("qoder.gateway.key.createFailed"), { description: api.asError(e) });
    } finally {
      setCreating(false);
    }
  }

  /** 面板「保存到本软件」：写台账 → 刷新列表让眼睛出现。失败时保留面板（关掉等于丢明文）。 */
  async function savePending() {
    if (!pending) return;
    setSaving(true);
    try {
      await api.saveQoderApiKeyPlaintext(pending.id, pending.value);
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
      await api.revokeQoderApiKey(revokeTarget.id);
      toast.success(t("qoder.gateway.key.revoked"), { description: revokeTarget.name });
      setRevokeTarget(null);
      await load();
      onChanged?.();
    } catch (e) {
      toast.error(t("qoder.gateway.key.revokeFailed"), { description: api.asError(e) });
    } finally {
      setBusy(false);
    }
  }

  async function confirmRestore() {
    if (!restoreTarget) return;
    setBusy(true);
    try {
      await api.restoreQoderApiKey(restoreTarget.id);
      toast.success(t("qoder.gateway.key.restored"), { description: restoreTarget.name });
      setRestoreTarget(null);
      await load();
      onChanged?.();
    } catch (e) {
      toast.error(t("qoder.gateway.key.restoreFailed"), { description: api.asError(e) });
    } finally {
      setBusy(false);
    }
  }

  async function confirmDelete() {
    if (!deleteTarget) return;
    setBusy(true);
    try {
      await api.deleteQoderApiKey(deleteTarget.id);
      toast.success(t("qoder.gateway.key.deleted"), { description: deleteTarget.name });
      setDeleteTarget(null);
      await load();
      onChanged?.();
    } catch (e) {
      toast.error(t("qoder.gateway.key.deleteFailed"), { description: api.asError(e) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className={cn(className)}>
      {/* 工具条与另两条产品线 / 统一网关**共用同一个构件**：计数 + 说明 + 创建出口。 */}
      <KeysBar total={keys.length} live={keys.filter((key) => !key.revoked).length}>
        <DemoAction>
          <Button size="sm" onClick={openCreate}>
            <KeyRound />
            {t("qoder.gateway.key.create")}
          </Button>
        </DemoAction>
      </KeysBar>

      {loading && keys.length === 0 ? (
        /* 区块**首次加载** ⇒ `<Skeleton>` 占位（§3.6）；文字型「加载中」+ `<Loader2>`
           是**按钮操作中**态的语言，两者不可混用。 */
        <div className="space-y-2 py-4" aria-busy="true">
          <Skeleton className="h-4 w-40" />
          <Skeleton className="h-4 w-64" />
          <Skeleton className="h-4 w-52" />
        </div>
      ) : keys.length === 0 ? (
        <EmptyState icon={KeyRound} title={t("qoder.gateway.key.empty")} className="py-4" />
      ) : (
        <div className="min-w-0 overflow-x-auto">
          <table className="w-full min-w-[640px] text-left text-sm">
            <thead>
              <tr className="text-xs text-muted-foreground">
                <th className="pb-2 pr-4 font-medium">{t("qoder.gateway.key.name")}</th>
                <th className="pb-2 pr-4 font-medium">{t("qoder.gateway.key.col.region")}</th>
                <th className="pb-2 pr-4 font-medium">{t("qoder.gateway.key.col.prefix")}</th>
                <th className="pb-2 pr-4 font-medium">{t("qoder.gateway.key.col.createdAt")}</th>
                <th className="pb-2 pr-4 font-medium">{t("qoder.gateway.key.col.lastUsed")}</th>
                {/* ★ issue #76：用量 / 权限两列（与 WB / Trae 同形，构件共用）。 */}
                <th className="pb-2 pr-4 font-medium">{t("wbStats.keyPerm.quota")}</th>
                <th className="pb-2 pr-4 font-medium">{t("wbStats.keyPerm.scope")}</th>
                <th className="pb-2 pr-4 font-medium">{t("qoder.gateway.key.col.status")}</th>
                <th className="pb-2 font-medium">{t("qoder.gateway.key.col.actions")}</th>
              </tr>
            </thead>
            <tbody>
              {keys.map((key) => {
                const revoked = key.revoked;
                const policy = policyOf(key);
                const expanded = expandedId === key.id;
                const scopeNames = policy.scopeAccounts.map(
                  (id) => qoderAccounts.find((a) => a.id === id)?.name ?? id,
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
                      <Badge variant="secondary" className="rounded-md">
                        {regionDescriptor(keyRegion(key)).versionLabel}
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
                    <td className="py-2 pr-4 text-xs text-muted-foreground">
                      {formatDate(key.createdAt)}
                    </td>
                    <td className="py-2 pr-4 text-xs text-muted-foreground">
                      {key.lastUsedAt ? formatDate(key.lastUsedAt) : t("qoder.gateway.key.neverUsed")}
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
                          {t("qoder.gateway.key.statusRevoked")}
                        </Badge>
                      ) : (
                        <Badge variant="success" className="rounded-md">
                          {t("qoder.gateway.key.statusActive")}
                        </Badge>
                      )}
                    </td>
                    {/* 操作列挡冒泡：点吊销/恢复不顺手展开明细。 */}
                    <td className="py-2" onClick={(event) => event.stopPropagation()}>
                      {/* ★ 已吊销 ⇒ **两个出口都在**：恢复（可逆）+ 删除（不可逆）。
                          与另两条产品线同形同序（恢复在前、删除在后）。 */}
                      {revoked ? (
                        <div className="flex items-center gap-1">
                          <Button variant="ghost" size="sm" onClick={() => setRestoreTarget(key)}>
                            <RotateCcw />
                            {t("qoder.gateway.key.restore")}
                          </Button>
                          <Button
                            variant="ghost"
                            size="sm"
                            className="text-destructive hover:text-destructive"
                            onClick={() => setDeleteTarget(key)}
                          >
                            <Trash2 />
                            {t("qoder.gateway.key.delete")}
                          </Button>
                        </div>
                      ) : (
                        <Button variant="ghost" size="sm" onClick={() => setRevokeTarget(key)}>
                          {t("qoder.gateway.key.revoke")}
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
        accounts={qoderAccounts}
        saving={permSaving}
        onSave={(next) => void onSavePolicy(next)}
      />

      {/* 说明段（原型 `<p class="tiny muted" style="margin:10px 2px 0">`）：掩码口径 /
          累计口径 / 吊销可逆。三条产品线共用一条键，差异只在表里的「归属」列。 */}
      {keys.length > 0 && (
        <p className="mt-2.5 text-[11.5px] leading-5 text-muted-foreground">
          {t("shared.gateway.keys.note")}
        </p>
      )}

      {/* 创建对话框 */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("qoder.gateway.key.create")}</DialogTitle>
            <DialogDescription>{t("qoder.gateway.key.createDesc")}</DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="qoder-key-name">{t("qoder.gateway.key.name")}</Label>
              <Input
                id="qoder-key-name"
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder={t("qoder.gateway.key.namePlaceholder")}
                spellCheck={false}
                autoComplete="off"
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="qoder-key-region">{t("qoder.gateway.key.region")}</Label>
              <Select value={region} onValueChange={(value) => setRegion(value as Region)}>
                <SelectTrigger
                  id="qoder-key-region"
                  className="w-full"
                  aria-label={t("qoder.gateway.key.region")}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {REGIONS.map((item) => (
                    <SelectItem key={item} value={item}>
                      {regionDescriptor(item).versionLabel}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">{t("qoder.gateway.key.regionHint")}</p>
            </div>
            {/* ★ 第三行：可选自定义密钥（空 ⇒ 自动生成；非空 ⇒ 原样使用）。 */}
            <div className="space-y-2">
              <Label htmlFor="qoder-key-custom">{t("shared.gateway.keys.customLabel")}</Label>
              <Input
                id="qoder-key-custom"
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
              {t("qoder.gateway.key.cancel")}
            </Button>
            <Button onClick={() => void onCreate()} disabled={creating}>
              {creating && <Loader2 className="animate-spin" />}
              {t("qoder.gateway.key.createSubmit")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 创建后的二选一面板（2026-11-04）——与另两条产品线同一形态：
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
            <DialogTitle>{t("qoder.gateway.key.revokeTitle")}</DialogTitle>
            <DialogDescription>
              {t("qoder.gateway.key.revokeDesc", { name: revokeTarget?.name ?? "" })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRevokeTarget(null)} disabled={busy}>
              {t("qoder.gateway.key.cancel")}
            </Button>
            <Button variant="destructive" onClick={() => void confirmRevoke()} disabled={busy}>
              {t("qoder.gateway.key.revoke")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 恢复确认（可逆动作 ⇒ 主按钮非 destructive） */}
      <Dialog open={restoreTarget !== null} onOpenChange={(open) => !open && setRestoreTarget(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("qoder.gateway.key.restoreTitle")}</DialogTitle>
            <DialogDescription>
              {t("qoder.gateway.key.restoreDesc", { name: restoreTarget?.name ?? "" })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRestoreTarget(null)} disabled={busy}>
              {t("qoder.gateway.key.cancel")}
            </Button>
            <Button onClick={() => void confirmRestore()} disabled={busy}>
              {t("qoder.gateway.key.restore")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 删除确认 */}
      <Dialog open={deleteTarget !== null} onOpenChange={(open) => !open && setDeleteTarget(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("qoder.gateway.key.deleteTitle")}</DialogTitle>
            <DialogDescription>
              {t("qoder.gateway.key.deleteDesc", { name: deleteTarget?.name ?? "" })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteTarget(null)} disabled={busy}>
              {t("qoder.gateway.key.cancel")}
            </Button>
            <Button variant="destructive" onClick={() => void confirmDelete()} disabled={busy}>
              {t("qoder.gateway.key.delete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

/**
 * Key 记录里的区域串 → `Region`。
 *
 * 后端只可能给 `"cn"` / `"global"`（`QoderRegion` 的两个取值）；出现别的值说明磁盘被
 * 人工改过 —— 此时**回落 `cn` 并在界面上如实显示**（而不是抛错炸掉整张 Key 表：
 * 一张表里一个坏字段不该让整页不可用）。
 */
function keyRegion(record: QoderApiKeyRecord): Region {
  return record.region === "global" ? "global" : "cn";
}
