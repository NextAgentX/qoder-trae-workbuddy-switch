import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Copy, KeyRound, Loader2, RotateCcw, Trash2 } from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { DemoAction } from "@/components/demo-action";
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
import { cn } from "@/lib/utils";

/**
 * 归属取值集合 = **4 个变体**（区域 × 程序；合并版 `trae` 与 `trae_code` 共用变体，
 * 已在 `allPrograms()` 里按 `variant` 去重，见该函数的说明）。
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
 * 区域标识 → 程序位：`cn` / `global` 落到该区域**在役**的程序位
 * （2026-10-15 收敛后 = 合并版客户端 `trae_merged` / `global_trae_merged`）。
 *
 * 与后端「区域标识落到该区域主程序」**同向** —— 收敛前主程序是 TraeWork、
 * 收敛后在役的只剩合并版，区域标识就该落合并版：新建 Key 默认绑定
 * 在役程序位，而不是退役的 TraeWork。
 *
 * 用途有两处，都不能省：
 * 1. `Select` 的 value 必须是选项集合里的值，否则下拉显示空白（页面只有区域维度）
 *    —— 选项集合（`PROGRAMS`）已收敛为合并版两条，区域标识若归一到
 *    `trae_work` 就不在集合里，正是这个症状；
 * 2. 归属列展示 —— 否则「两把行为完全相同的 Key」一个显示「国内版」、一个显示「Trae CN」。
 *
 * 遗留程序位标识（`trae_work` / `trae_cn` / `global_trae_code`）**原样返回**：
 * 旧 Key 的归属是历史事实，展示层靠 `programLabelOf` 的回落渲染它们（不归一、不改写）。
 */
function normalizeProgramId(variant: TraeVariantId): TraeVariantId {
  if (variant === "cn") return "trae_merged";
  if (variant === "global") return "global_trae_merged";
  return variant;
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
 * 明文只在创建时一次性返回（`create_trae_api_key`），列表接口永远只给脱敏前缀。
 */
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
  const [creating, setCreating] = useState(false);
  const [plaintext, setPlaintext] = useState<{ value: string; name: string } | null>(null);
  /**
   * 「刚复制成功」态（驱动复制按钮 `Copy` → `Check`）。
   *
   * 与 WorkBuddy 侧 `api-key-table.tsx` **同一规则**：只影响图标与文案，**不影响可点性** ——
   * 明文只显示这一次，用户可能要反复复制，按钮永远保持可点。
   * 定时器存 ref：连点两次时旧定时器不会把新一次的「已复制」提前复位。
   */
  const [copied, setCopied] = useState(false);
  const copyTimer = useRef<number | null>(null);
  const [revokeTarget, setRevokeTarget] = useState<TraeApiKeyRecord | null>(null);
  /**
   * ★ 恢复目标：与 `revokeTarget` **互逆**，与 WorkBuddy 侧 `api-key-table.tsx` **同一规则** ——
   * 已吊销的行必须同时给出「恢复」与「删除」两条出口（吊销只置 `revoked_at`，明文未变）。
   */
  const [restoreTarget, setRestoreTarget] = useState<TraeApiKeyRecord | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<TraeApiKeyRecord | null>(null);
  const [busy, setBusy] = useState(false);

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
      const result = await api.createTraeApiKey(trimmed, variant);
      const value = result.key;
      if (!value) {
        // 创建成功但没回明文 = 契约异常，必须显式提示而不是静默（明文不可复原）。
        toast.error(t("trae.gateway.key.noPlaintext"));
        return;
      }
      setCreateOpen(false);
      setCopied(false);
      setPlaintext({ value, name: trimmed });
      await load();
      onChanged?.();
    } catch (e) {
      toast.error(t("trae.gateway.key.createFailed"), { description: api.asError(e) });
    } finally {
      setCreating(false);
    }
  }

  /** 复制明文。**可反复调用**：成功后把按钮切成「已复制」，1.6s 后复位。 */
  async function onCopyPlaintext() {
    if (!plaintext) return;
    await copyText(plaintext.value, t("trae.gateway.key.copied"));
    setCopied(true);
    if (copyTimer.current !== null) window.clearTimeout(copyTimer.current);
    copyTimer.current = window.setTimeout(() => setCopied(false), 1600);
  }

  /** 关闭明文对话框。**唯一的关闭路径**（遮罩 / ESC 已被拦截，见下方 DialogContent）。 */
  function closePlaintext() {
    if (copyTimer.current !== null) window.clearTimeout(copyTimer.current);
    copyTimer.current = null;
    setCopied(false);
    setPlaintext(null);
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
    <Card className={cn("gap-0 py-0", className)}>
      <div className="flex items-center justify-between gap-3 border-b border-border/60 px-5 py-3">
        <span className="text-sm font-semibold">API Key</span>
        <DemoAction>
          <Button size="sm" onClick={openCreate}>
            <KeyRound />
            {t("trae.gateway.key.create")}
          </Button>
        </DemoAction>
      </div>

      <div className="px-5 py-3">
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
                  <th className="pb-2 pr-4 font-medium">{t("trae.gateway.key.col.status")}</th>
                  <th className="pb-2 font-medium">{t("trae.gateway.key.col.actions")}</th>
                </tr>
              </thead>
              <tbody>
                {keys.map((key) => {
                  const revoked = key.revoked;
                  return (
                    <tr key={key.id} className="border-t border-border/60">
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
                      {/* ★ E3：掩码补上尾部 4 位（`prefix••••••••suffix`）——
                          用户靠它**确认是哪一把**（同时建两把时前缀可能只差一两位）。
                          旧台账 / 短 legacy Key 无 `suffix` ⇒ 只显示前缀 + 省略号，
                          与 WorkBuddy 侧 `api-key-table.tsx` **同一规则**。 */}
                      <td className="py-2 pr-4 font-mono text-xs text-muted-foreground">
                        {key.suffix
                          ? `${key.prefix}••••••••${key.suffix}`
                          : `${key.prefix}…`}
                      </td>
                      <td className="py-2 pr-4 text-xs text-muted-foreground">{formatDate(key.createdAt)}</td>
                      <td className="py-2 pr-4 text-xs text-muted-foreground">
                        {key.lastUsedAt ? formatDate(key.lastUsedAt) : t("trae.gateway.key.neverUsed")}
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
                      <td className="py-2">
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
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </div>

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

      {/* 一次性明文展示。
          ★ 2026-10-13：与 WorkBuddy 侧 `api-key-table.tsx` **同规则** —— 遮罩点击与 ESC
            一律拦截（明文不可复原，误关即永久丢失），复制按钮不禁用（允许反复复制）。 */}
      <Dialog open={plaintext !== null} onOpenChange={(open) => !open && closePlaintext()}>
        <DialogContent
          className="sm:max-w-md"
          onInteractOutside={(event) => event.preventDefault()}
          onEscapeKeyDown={(event) => event.preventDefault()}
        >
          <DialogHeader>
            <DialogTitle>{t("trae.gateway.key.createdTitle")}</DialogTitle>
            <DialogDescription>{t("trae.gateway.key.createdDesc")}</DialogDescription>
          </DialogHeader>
          {plaintext && (
            <div className="flex items-center gap-2 rounded-lg border border-border bg-muted/40 px-3 py-2.5">
              {/* `select-all`：点一下即全选，手动 Ctrl+C 也能用（剪贴板 API 被拒时的兜底）。 */}
              <code className="min-w-0 flex-1 select-all break-all font-mono text-xs">{plaintext.value}</code>
              <Button
                variant="outline"
                size="sm"
                data-act="gw-key-copy"
                onClick={() => void onCopyPlaintext()}
              >
                {copied ? <Check /> : <Copy />}
                {copied ? t("shared.clipboard.copied") : t("trae.gateway.key.copy")}
              </Button>
            </div>
          )}
          <DialogFooter>
            <Button onClick={closePlaintext}>{t("trae.gateway.key.saved")}</Button>
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
    </Card>
  );
}
