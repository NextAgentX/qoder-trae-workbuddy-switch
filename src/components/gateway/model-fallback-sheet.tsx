/**
 * 降级链编辑抽屉 —— 与原型 `docs/prototype/demo-app.js::fbDrawerHtml` 同构。
 *
 * ## 为什么是 Sheet（右侧抽屉）而不是 Dialog
 *
 * 原型里「改一份带多组的配置」一律走右侧抽屉（Key 权限、CLI 接入同款），弹窗只留给
 * 「确认一个动作」。这里要选模型、排顺序、勾条件、看预演，属前者。
 *
 * ## 模拟路径为什么是纵向时序
 *
 * 抽屉宽度下横向箭头链必然折行，折完就看不出顺序了（原型第一版实测）。
 * 纵向一行一跳，`1 2 3` 的序号 + 右对齐的结论，扫一眼就知道会用谁。
 *
 * ## 保存是「整作用域覆盖」
 *
 * 后端 `save_model_fallback` 用 `set_scope` 整段替换 —— 所以提交时必须把**其它链**
 * 一起带上，否则保存一条会删掉别的。这是最容易写错的一处。
 */

import { useEffect, useMemo, useState } from "react";
import { TriangleAlert } from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import {
  getGatewayModels,
  getQoderGatewayModels,
  getTraeGatewayModels,
  previewModelFallback,
} from "@/lib/api";
import { useT } from "@/lib/i18n";
import { normalizeQoderGatewayCatalog } from "@/lib/qoder-gateway";
import { SCOPES, type Scope } from "@/lib/scope";
import { normalizeTraeGatewayModels } from "@/lib/trae-gateway";
import { cn } from "@/lib/utils";
import { useGatewayStore } from "@/stores/gateway";
import type {
  FallbackPreview,
  FallbackTargetInput,
  FallbackTrigger,
  ModelFallbackView,
  Region,
  ScopeFallbackConfig,
} from "@/lib/types";

/** 可勾选的触发条件（顺序即展示顺序：先「默认开」后「需谨慎」）。 */
/**
 * 「与入口同作用域」在下拉里的哨兵值。
 *
 * Radix 的 `Select` 把**空字符串**当作「无值」（`SelectValue` 会渲染 placeholder），
 * 所以不能用 `""` 表示「本区域」—— 用户会看到一个空白下拉，以为没配。
 */
const SELF_SCOPE = "__self__";

const TRIGGERS: { key: FallbackTrigger; labelKey: string; descKey: string }[] = [
  { key: "gate", labelKey: "shared.gateway.fallback.trigger.gate", descKey: "shared.gateway.fallback.trigger.gateDesc" },
  { key: "model_rate", labelKey: "shared.gateway.fallback.trigger.model_rate", descKey: "shared.gateway.fallback.trigger.model_rateDesc" },
  { key: "model_blocked", labelKey: "shared.gateway.fallback.trigger.model_blocked", descKey: "shared.gateway.fallback.trigger.model_blockedDesc" },
  { key: "server", labelKey: "shared.gateway.fallback.trigger.server", descKey: "shared.gateway.fallback.trigger.serverDesc" },
  { key: "timeout", labelKey: "shared.gateway.fallback.trigger.timeout", descKey: "shared.gateway.fallback.trigger.timeoutDesc" },
  // 404：上游说「这个模型不存在」—— 对**账号**的模型清单而言是真事（换号有用），
  // 对**模型**而言可能是上游整体下线了（换模型有用）。所以它是一档可配条件。
  { key: "not_found", labelKey: "shared.gateway.fallback.trigger.not_found", descKey: "shared.gateway.fallback.trigger.not_foundDesc" },
];

interface Draft {
  from: string;
  targets: FallbackTargetInput[];
  triggers: FallbackTrigger[];
}

/**
 * 下拉里的一个模型。
 *
 * ★ `id` 是**值**：`/v1/models` 交给客户端、客户端请求里带的就是它
 * （`deepseek-v4.1-flash`）。`name` 只用于展示（`Deepseek-V4.1-Flash`）。
 * 用展示名当值会让「跨区域同名」与「同区域同名不同 id」（`-sg`）彻底无法表达，
 * 也会让降级链永远命中不了入站请求 —— 两者是两种写法，不是两个模型。
 */
export interface ModelOption {
  id: string;
  name: string;
}

/** 与后端 `model_fallback::norm_model` 同口径：忽略大小写与 `-_.:/`、空白。 */
function normModel(value: string): string {
  return value.replace(/[-_.:/ ]/g, "").toLowerCase();
}

interface Props {
  scope: Scope;
  region: Region;
  /** 页面注入的模型清单（Trae / Qoder 的清单不在 gateway store 里）。 */
  modelOptions?: ModelOption[];
  view: ModelFallbackView;
  /** `undefined` = 关闭；`null` = 新建；`string` = 编辑该主模型。 */
  editing: string | null | undefined;
  onClose: () => void;
}

export function ModelFallbackSheet({
  scope,
  region,
  modelOptions,
  view,
  editing,
  onClose,
}: Props) {
  const t = useT();
  const saveFallback = useGatewayStore((s) => s.saveFallback);
  const catalog = useGatewayStore((s) => s.models[region]);
  const disabledSet = useGatewayStore((s) => s.disabled[scope]);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [preview, setPreview] = useState<FallbackPreview | null>(null);
  const [saving, setSaving] = useState(false);

  const open = editing !== undefined;

  /* 可选模型：已禁用的不出现（禁用集合存的是**模型 id**，与 `/v1/models` 同源），
     并按 id 去重。同一区域里「同展示名、不同 id」（如 `deepseek-v4.1-flash-sg`）必须
     都能选到 —— 那正是它过去在列表里消失的原因。 */
  /**
   * 跨作用域目标的模型清单（按 scope 缓存）。
   *
   * 能拉到就**给下拉**（比让用户手打 id 靠谱），拉不到才回落手填 ——
   * 手填本身没错，错的是不给列表也不说为什么。
   */
  const [scopeModels, setScopeModels] = useState<Record<string, ModelOption[]>>({});
  /**
   * 目标平台给出的原因（Qoder 缺账号 / 拉取失败时才非空）。
   *
   * 空清单**必须解释**：不解释就是一个没有任何选项的弹层，用户只能猜「是不是坏了」。
   */
  const [scopeNotes, setScopeNotes] = useState<Record<string, string | null>>({});

  useEffect(() => {
    if (!draft) return;
    const wanted = new Set(
      draft.targets
        .map((target) => target.scope)
        .filter((candidate): candidate is string => Boolean(candidate) && candidate !== scope),
    );
    for (const candidate of wanted) {
      if (scopeModels[candidate]) continue;
      const platform = candidate.split("-")[0];
      const region = candidate.endsWith("global") ? "global" : "cn";
      const load: Promise<{ models: ModelOption[]; note: string | null }> =
        platform === "wb"
          ? getGatewayModels(region).then((snapshot) => ({
              models: snapshot.models.map((model) => ({ id: model.id, name: model.name })),
              note: null,
            }))
          : platform === "qoder"
            ? getQoderGatewayModels(candidate as Scope).then((raw) => {
                const catalog = normalizeQoderGatewayCatalog(raw);
                return {
                  models: catalog.rows.map((row) => ({ id: row.id, name: row.name })),
                  note: catalog.note,
                };
              })
            : platform === "trae"
              ? getTraeGatewayModels(region).then((raw) => ({
                  models: normalizeTraeGatewayModels(raw),
                  note: null,
                }))
              : Promise.resolve({ models: [], note: null });
      void load
        .then(({ models, note }) => {
          setScopeModels((prev) => ({ ...prev, [candidate]: models }));
          setScopeNotes((prev) => ({ ...prev, [candidate]: note }));
        })
        .catch(() => {
          /* 拉不到就让用户手填，不阻塞配置 */
          setScopeModels((prev) => ({ ...prev, [candidate]: [] }));
        });
    }
  }, [draft, scope, scopeModels]);

  /** 列表本身（已按 id 去重、已过禁用过滤）。 */
  const options = useMemo(() => {
    // 页面注入了清单（Trae / Qoder）就用它：那两条平台的模型不在 gateway store 里。
    const source: ModelOption[] =
      modelOptions ?? (catalog?.models ?? []).map((model) => ({ id: model.id, name: model.name }));
    const seen = new Set<string>();
    const out: ModelOption[] = [];
    for (const option of source) {
      const id = option.id?.trim();
      if (!id || seen.has(id)) continue;
      // ★ 禁用集合存的是 id（与 `/v1/models` 的出口同一份），拿 name 去查永远不命中。
      if (disabledSet?.has(id)) continue;
      seen.add(id);
      out.push({ id, name: option.name?.trim() || id });
    }
    return out;
  }, [modelOptions, catalog, disabledSet]);

  /**
   * 展示文案：**同名不同 id 时必须把 id 带出来**，否则用户看到两个一模一样的选项。
   * （`deepseek-v4.1-flash` 与 `deepseek-v4.1-flash-sg` 的展示名完全相同。）
   */
  function labelOf(option: ModelOption, list: ModelOption[] = options): string {
    const duplicated = list.filter((other) => other.name === option.name).length > 1;
    return duplicated ? `${option.name}（${option.id}）` : option.name;
  }

  // 打开时初始化草稿（每次切换目标都重建，抽屉是「一次性表单」而非长驻编辑器）。
  useEffect(() => {
    if (!open) {
      setDraft(null);
      setPreview(null);
      return;
    }
    const existing = editing ? view.chains.find((chain) => chain.from === editing) : null;
    /* ★ 草稿必须**补齐到 `max_targets` 行**：UI 恒画满 `max_targets` 行（空格也要能选），
       而 `draft.targets` 只装已有目标 ⇒ 不补齐时点空行的下拉会读到 `targets[i].scope`，
       `undefined.scope` 直接抛 TypeError（整个抽屉白屏）。 */
    const rows: FallbackTargetInput[] = existing
      ? existing.targets.map((target) => ({
          // 同作用域存 `null`（配置里最简形态）；跨作用域才带 scope。
          scope: target.scope === scope ? null : target.scope,
          model: target.model,
        }))
      : [];
    setDraft({
      from: existing?.from ?? "",
      targets: Array.from({ length: view.max_targets }, (_, index) =>
        rows[index] ? { ...rows[index] } : { scope: null, model: "" },
      ),
      triggers: existing ? [...existing.triggers] : [...view.baseline],
    });
  }, [open, editing, view]);

  // 预演：后端纯计算，零副作用（不写、不领探针票、不发上游）。
  const targetsKey =
    draft?.targets
      .filter((target) => target.model.trim())
      .map((target) => `${target.scope ?? ""}:${target.model}`)
      .join(",") ?? "";
  useEffect(() => {
    if (!draft?.from) {
      setPreview(null);
      return;
    }
    let alive = true;
    void previewModelFallback(
      scope,
      draft.from,
      draft.targets.filter((target) => target.model.trim()).map((target) => target.model),
    )
      .then((result) => {
        if (alive) setPreview(result);
      })
      .catch(() => {
        if (alive) setPreview(null);
      });
    return () => {
      alive = false;
    };
    // `targetsKey` 是数组的稳定指纹（数组本身每次渲染都是新引用）。
  }, [scope, draft?.from, targetsKey]);

  async function onSave() {
    if (!draft?.from) return;
    const targets = draft.targets
      .filter((target) => target.model.trim())
      .map((target) => ({
        // 同作用域传 `null`（后端等价于裸字符串），跨作用域才带 scope。
        scope: target.scope && target.scope !== scope ? target.scope : null,
        model: target.model.trim(),
      }));
    if (!targets.length) {
      toast.error(t("shared.gateway.fallback.needTarget"), {
        description: t("shared.gateway.fallback.needTargetDesc"),
      });
      return;
    }
    /* ★ 整作用域覆盖：必须把其它链原样带上，否则保存一条会删掉别的。
       正在编辑的那条由下面按**归一后的键**重写，这里跳过 —— 旧配置的链键是展示名
       （`DeepSeek-V4.1-Flash`），入站请求带的是 id，保存一次就该迁成 id，而不是留下两份。 */
    const fromId =
      options.find((option) => normModel(option.id) === normModel(draft.from))?.id ?? draft.from;
    const chains: ScopeFallbackConfig["chains"] = {};
    for (const chain of view.chains) {
      if (chain.from === editing) continue;
      chains[chain.from] = {
        targets: chain.targets.map((target) => ({
          scope: target.scope === scope ? null : target.scope,
          model: target.model,
        })),
        triggers: chain.triggers,
        enabled: chain.enabled,
      };
    }
    chains[fromId] = { targets, triggers: draft.triggers, enabled: true };
    setSaving(true);
    try {
      await saveFallback(scope, { enabled: view.enabled, chains });
      toast.success(t("shared.gateway.fallback.saveOk"), {
        description: t("shared.gateway.fallback.saveOkDesc"),
      });
      onClose();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setSaving(false);
    }
  }

  const isNew = editing === null;

  /** 取第 `index` 行；草稿已补齐到 `max_targets`，这里兜住「长度被外部改动」的意外。 */
  const rowAt = (rows: FallbackTargetInput[], index: number): FallbackTargetInput =>
    rows[index] ?? { scope: null, model: "" };

  return (
    <Sheet open={open} onOpenChange={(next) => (!next ? onClose() : undefined)}>
      <SheetContent side="right" showCloseButton={false} className="flex w-full flex-col sm:max-w-md">
        <SheetHeader>
          <SheetTitle>
            {isNew
              ? t("shared.gateway.fallback.drawer.new")
              : t("shared.gateway.fallback.drawer.edit", { model: editing ?? "" })}
          </SheetTitle>
          <SheetDescription>{t("shared.gateway.fallback.notice")}</SheetDescription>
        </SheetHeader>

        {draft ? (
          <div className="min-h-0 flex-1 space-y-5 overflow-y-auto px-5 pb-2">
            {/* ── 主模型 ── */}
            <section className="space-y-2">
              <h3 className="text-[13px] font-semibold">
                {t("shared.gateway.fallback.field.from")}
              </h3>
              {isNew ? (
                <Select
                  value={draft.from}
                  onValueChange={(value) =>
                    setDraft((prev) => (prev ? { ...prev, from: value } : prev))
                  }
                >
                  <SelectTrigger className="w-full font-mono text-[12px]">
                    <SelectValue placeholder="—" />
                  </SelectTrigger>
                  <SelectContent>
                    {options.map((option) => (
                      <SelectItem
                        key={option.id}
                        value={option.id}
                        className="font-mono text-[12px]"
                      >
                        {labelOf(option)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              ) : (
                <div className="rounded-md border border-border bg-card-2 px-2.5 py-1.5 font-mono text-[12px]">
                  {draft.from}
                </div>
              )}
              <p className="text-[11.5px] leading-relaxed text-muted-foreground">
                {t("shared.gateway.fallback.field.fromLocked")}
              </p>
            </section>

            {/* ── 备选序列 ── */}
            <section className="space-y-2">
              <h3 className="text-[13px] font-semibold">
                {t("shared.gateway.fallback.field.targets")}
              </h3>
              <div className="space-y-1.5">
                {Array.from({ length: view.max_targets }, (_, index) => {
                  const target = draft.targets[index] ?? { scope: null, model: "" };
                  const crossScope = Boolean(target.scope && target.scope !== scope);
                  const list = crossScope ? (scopeModels[target.scope as string] ?? []) : options;
                  // 目标平台清单拉不到（Trae 的清单不在这里解析 / Qoder 没账号）⇒ 回落手填。
                  const manual = crossScope && list.length === 0;
                  return (
                  <div key={index} className="flex items-center gap-1.5">
                    <span className="w-3.5 shrink-0 text-right font-mono text-[10px] text-muted-foreground">
                      {index + 1}
                    </span>
                    {/* 作用域：默认「本区域」；同平台的另一区域可切（WB 已支持执行），
                        跨平台列出但**禁用**并说明原因 —— 不列出来会让用户以为不支持，
                        列出来能选则是假开关。 */}
                    <Select
                      value={target.scope ?? SELF_SCOPE}
                      onValueChange={(value) =>
                        setDraft((prev) => {
                          if (!prev) return prev;
                          const targets = [...prev.targets];
                          const current = rowAt(targets, index);
                          const next = value === SELF_SCOPE ? null : value;
                          const changed = next !== (current.scope ?? null);
                          targets[index] = {
                            ...current,
                            scope: next,
                            // 换作用域后原模型名可能不在目标清单里，清空避免误存。
                            model: changed ? "" : current.model,
                          };
                          return { ...prev, targets };
                        })
                      }
                    >
                      <SelectTrigger className="w-[92px] shrink-0 text-[11.5px]">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value={SELF_SCOPE}>
                          {t("shared.gateway.fallback.field.thisScope")}
                        </SelectItem>
                        {SCOPES.filter((candidate) => candidate !== scope).map((candidate) => {
                          // 跨平台现在三个入口都支持（由 `cross_depth` 限深，最多一层）。
                          return (
                            <SelectItem key={candidate} value={candidate}>
                              {candidate}
                            </SelectItem>
                          );
                        })}
                      </SelectContent>
                    </Select>
                    {/* 跨平台目标：能拉到目标平台的清单就给下拉（Qoder 的目录带凭据现拉），
                        拉不到才回落手填 —— 那时 placeholder 会告诉用户要填目标平台的模型 id。 */}
                    {manual ? (
                      <div className="min-w-0 flex-1 space-y-1">
                        <Input
                          value={target.model}
                          onChange={(event) =>
                            setDraft((prev) => {
                              if (!prev) return prev;
                              const targets = [...prev.targets];
                              targets[index] = {
                                ...rowAt(targets, index),
                                model: event.target.value,
                              };
                              return { ...prev, targets };
                            })
                          }
                          placeholder={t("shared.gateway.fallback.field.crossModelPlaceholder")}
                          className="w-full font-mono text-[12px]"
                        />
                        {/* 目标平台自己给出的原因（Qoder 缺账号 / 拉取失败）：
                            空清单不给解释，用户只能当成「坏了」。 */}
                        {scopeNotes[target.scope as string] ? (
                          <p className="text-[11px] leading-snug text-muted-foreground">
                            {scopeNotes[target.scope as string]}
                          </p>
                        ) : null}
                      </div>
                    ) : (
                    <Select
                      value={target.model}
                      onValueChange={(value) =>
                        setDraft((prev) => {
                          if (!prev) return prev;
                          const targets = [...prev.targets];
                          targets[index] = { ...rowAt(targets, index), model: value };
                          return { ...prev, targets };
                        })
                      }
                    >
                      <SelectTrigger className="min-w-0 flex-1 font-mono text-[12px]">
                        <SelectValue placeholder="—" />
                      </SelectTrigger>
                      <SelectContent>
                        {list
                          /* 自环只在**同作用域**成立（口径与后端 `normalize` / `plan_for` 一致）：
                             跨作用域的同名模型是另一个上游，不能从下拉里藏掉 ——
                             否则「主模型国际版 + 备选国内版同名」永远配不出来。 */
                          .filter((option) => crossScope || normModel(option.id) !== normModel(draft.from))
                          .map((option) => (
                            <SelectItem
                              key={option.id}
                              value={option.id}
                              className="font-mono text-[12px]"
                            >
                              {labelOf(option, list)}
                            </SelectItem>
                          ))}
                        {/* 已存的目标可能不在当前清单里（目录变了 / 手工改过配置）——
                            补一条，否则下拉显示空白、用户以为没配。 */}
                        {target.model && !list.some((option) => option.id === target.model) ? (
                          <SelectItem value={target.model} className="font-mono text-[12px]">
                            {target.model}
                          </SelectItem>
                        ) : null}
                      </SelectContent>
                    </Select>
                    )}
                    <Button
                      variant="outline"
                      size="sm"
                      className="px-2"
                      disabled={index === 0}
                      aria-label="↑"
                      onClick={() =>
                        setDraft((prev) => {
                          if (!prev || index === 0) return prev;
                          const targets = [...prev.targets];
                          [targets[index - 1], targets[index]] = [
                            targets[index],
                            targets[index - 1],
                          ];
                          return { ...prev, targets };
                        })
                      }
                    >
                      ↑
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      className="px-2"
                      disabled={index === view.max_targets - 1}
                      aria-label="↓"
                      onClick={() =>
                        setDraft((prev) => {
                          if (!prev || index >= view.max_targets - 1) return prev;
                          const targets = [...prev.targets];
                          [targets[index + 1], targets[index]] = [
                            targets[index],
                            targets[index + 1],
                          ];
                          return { ...prev, targets };
                        })
                      }
                    >
                      ↓
                    </Button>
                  </div>
                  );
                })}
              </div>
              <p className="text-[11.5px] leading-relaxed text-muted-foreground">
                {t("shared.gateway.fallback.field.targetsHint", {
                  max: view.max_targets,
                  hops: view.max_targets + 1,
                })}
              </p>
              {/* 能力校验未做（P1）—— 必须**明示**，否则用户以为网关会替他挑「能接住的」备选。 */}
              <p className="text-[11px] leading-relaxed text-muted-foreground/80">
                {t("shared.gateway.fallback.capsNote")}
              </p>
            </section>

            {/* ── 触发条件 ── */}
            <section className="space-y-2">
              <h3 className="text-[13px] font-semibold">
                {t("shared.gateway.fallback.field.triggers")}
              </h3>
              <div className="space-y-1.5">
                {TRIGGERS.map((trigger) => {
                  const checked = draft.triggers.includes(trigger.key);
                  return (
                    <label
                      key={trigger.key}
                      className={cn(
                        "flex cursor-pointer items-start gap-2.5 rounded-md border px-2.5 py-2 transition-colors",
                        checked
                          ? "border-primary/50 bg-primary/10"
                          : "border-border bg-card-2 hover:border-border-strong",
                      )}
                    >
                      <Checkbox
                        checked={checked}
                        onCheckedChange={(next) =>
                          setDraft((prev) => {
                            if (!prev) return prev;
                            const triggers = next
                              ? [...prev.triggers, trigger.key]
                              : prev.triggers.filter((item) => item !== trigger.key);
                            return { ...prev, triggers };
                          })
                        }
                        className="mt-0.5"
                      />
                      <span className="min-w-0">
                        <b className="block text-[12.5px] font-medium">
                          {t(trigger.labelKey as never)}
                        </b>
                        <span className="block text-[11px] leading-relaxed text-muted-foreground">
                          {t(trigger.descKey as never)}
                        </span>
                      </span>
                    </label>
                  );
                })}

                {/* 固定不降级：**让用户看见「什么不会降级」比让他猜更重要**。 */}
                <div className="rounded-md border border-border bg-card-2 px-2.5 py-2 opacity-70">
                  <b className="block text-[12.5px] font-medium">
                    {t("shared.gateway.fallback.never.title")}
                  </b>
                  <span className="block text-[11px] leading-relaxed text-muted-foreground">
                    {t("shared.gateway.fallback.never.desc")}
                  </span>
                </div>
              </div>
            </section>

            {/* ── 模拟路径（纵向时序） ── */}
            <section className="space-y-2">
              <h3 className="text-[13px] font-semibold">
                {t("shared.gateway.fallback.sim.title")}
              </h3>
              <div className="rounded-lg border border-border bg-card-2 px-3 py-2">
                {!preview ? (
                  <p className="py-1 text-[11.5px] text-muted-foreground">
                    {draft.from ? "…" : t("shared.gateway.fallback.sim.try")}
                  </p>
                ) : (
                  <>
                    {preview.steps.map((step, index) => (
                      <div
                        key={`${step.model}-${index}`}
                        className={cn(
                          "grid grid-cols-[14px_minmax(0,1fr)_48px_auto] items-center gap-2 py-1 text-[11.5px]",
                          index > 0 && "border-t border-border",
                        )}
                      >
                        <span className="text-center font-mono text-[10px] text-muted-foreground">
                          {index + 1}
                        </span>
                        <span className="truncate font-mono">{step.model}</span>
                        <span className="text-[10.5px] text-muted-foreground">
                          {step.role === "primary"
                            ? t("shared.gateway.fallback.sim.primary")
                            : t("shared.gateway.fallback.sim.target", { n: index })}
                        </span>
                        <span
                          className={cn(
                            "whitespace-nowrap text-right text-[10.5px]",
                            step.action === "try" && index > 0 && "text-success",
                            step.action === "try" && index === 0 && "text-warning",
                            step.action !== "try" && "text-warning",
                          )}
                        >
                          {step.action === "skip_gated"
                            ? t("shared.gateway.fallback.sim.skipGated")
                            : step.action === "skip_disabled"
                              ? t("shared.gateway.fallback.sim.skipDisabled")
                              : index === 0
                                ? t("shared.gateway.fallback.sim.try")
                                : t("shared.gateway.fallback.sim.willUse")}
                        </span>
                      </div>
                    ))}
                    <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">
                      {t("shared.gateway.fallback.sim.allFail")}
                    </p>
                  </>
                )}
              </div>
            </section>

            {!draft.from ? (
              <p className="flex items-start gap-1.5 text-[11.5px] text-warning">
                <TriangleAlert className="mt-0.5 size-3 shrink-0" />
                {t("shared.gateway.fallback.needTarget")}
              </p>
            ) : null}
          </div>
        ) : null}

        <div className="flex gap-2 border-t border-border px-5 py-3">
          <Button variant="outline" className="flex-1" onClick={onClose}>
            {t("shared.gateway.fallback.cancel")}
          </Button>
          <Button
            className="flex-1"
            disabled={!draft?.from || saving}
            onClick={() => void onSave()}
          >
            {t("shared.gateway.fallback.save")}
          </Button>
        </div>
      </SheetContent>
    </Sheet>
  );
}
