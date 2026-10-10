import { useCallback, useEffect, useRef, useState } from "react";
import {
  ArrowLeftRight,
  Blocks,
  FolderOpen,
  Import,
  Loader2,
  RefreshCw,
} from "lucide-react";

import { DemoAction } from "@/components/demo-action";
import { SettingsFieldRow } from "@/components/settings-primitives";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import * as api from "@/lib/api";
import { useT } from "@/lib/i18n";
import { otherRegion, regionLabel, REGIONS } from "@/lib/region";
import type { Region, SkillCopyReport, SkillEntry, SkillListResponse } from "@/lib/types";
import { cn } from "@/lib/utils";

/**
 * **Skill 管理（cc-switch 式）** —— 侧栏底部应用级入口。
 *
 * ## 与「通用设置」同区，但不并入 `app-settings.tsx`
 *
 * 侧栏底部（`App.tsx` 的那个 `<section>`）是两个产品分区**唯一共用**的渲染区域，
 * 因此应用级入口都落在这里。但 skill 管理**不是应用级设置**（不改本应用的外观 / 自启 /
 * 更新），它是一套独立的「region 级资产」管理（清单 / 启停 / 跨区搬迁 / 本地导入），
 * 自带一整域文案与取数。塞进 `app-settings.tsx` 会让「应用级设置的唯一来源」这条
 * 边界失真，故独立成文件、只**借用同一渲染区**（PRD §8.1）。
 *
 * ## 形态与 `AppSettingsDialog` 同构
 *
 * **受控 Dialog**（`open` / `onOpenChange`，由顶栏 ⚙ 菜单驱动）：**懒挂载**，
 * 不开就不取数（清单要读盘 + 解析 frontmatter，常驻挂载会让每次启动都白付一次 IPC）。
 *
 * ## 门禁（PRD §10.4）
 *
 * 本工具读写的是**运行本应用那台机器**上的 `{data_dir}/skills`。webui 直连远程服务端时，
 * 那是**服务端**的目录 —— 此时不隐藏入口（命令在 webui 通道同样可用），而是给出
 * 「机器来源」提示（`skills.webuiNote`），避免用户误以为在改本机。
 * 演示模式下写 / 动作由 `DemoAction` 遮罩，清单走 fixture（只读）。
 */

/** 受控入口（由顶栏 ⚙ 菜单驱动，固定在「通用设置」同级）。 */
export function SkillManagerDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const t = useT();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("skillManager.title")}</DialogTitle>
          <DialogDescription>{t("skillManager.description")}</DialogDescription>
        </DialogHeader>
        {/* Dialog 内容默认卸载 ⇒ 面板内的取数只在打开时发生（懒挂载）。 */}
        <SkillManagerPanel />
      </DialogContent>
    </Dialog>
  );
}

/** 面板内的一次性提示（切换 / 搬迁 / 导入的结果与错误）。 */
type Notice = { type: "ok" | "err"; text: string };

function SkillManagerPanel() {
  const t = useT();
  const [region, setRegion] = useState<Region>("cn");
  const [data, setData] = useState<SkillListResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  /** 正在切换的 skill 目录名（同一时刻只允许一个开关在飞）。 */
  const [busyDir, setBusyDir] = useState<string | null>(null);
  const [copying, setCopying] = useState(false);
  const [report, setReport] = useState<SkillCopyReport | null>(null);
  const [importPath, setImportPath] = useState("");
  const [importing, setImporting] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);

  // 竞态防护：切 region / 刷新时旧请求可能后到，用序号丢弃过期响应。
  const requestSeq = useRef(0);

  const load = useCallback(async () => {
    const seq = ++requestSeq.current;
    setLoading(true);
    setError(null);
    try {
      const next = await api.listSkills(region);
      if (seq !== requestSeq.current) return;
      setData(next);
    } catch (cause) {
      if (seq !== requestSeq.current) return;
      setError(api.asError(cause));
    } finally {
      if (seq === requestSeq.current) setLoading(false);
    }
  }, [region]);

  useEffect(() => {
    void load();
  }, [load]);

  const skills = data?.skills ?? [];

  async function onToggle(entry: SkillEntry, enabled: boolean) {
    if (busyDir) return;
    setBusyDir(entry.dirName);
    setNotice(null);
    try {
      const result = await api.setSkillEnabled(region, entry.dirName, enabled);
      // 后端返回的是**写盘后回读的权威值**：以它更新该行，并把 inconsistent 清掉
      // （启停会一并写齐两个字段，不再「只写一半」）。
      setData((prev) =>
        prev
          ? {
              ...prev,
              skills: prev.skills.map((skill) =>
                skill.dirName === entry.dirName
                  ? { ...skill, enabled: result.enabled, inconsistent: false }
                  : skill,
              ),
            }
          : prev,
      );
    } catch (cause) {
      setNotice({ type: "err", text: t("skillManager.toggle.failed", { message: api.asError(cause) }) });
    } finally {
      setBusyDir(null);
    }
  }

  async function onCopyCross() {
    if (copying) return;
    setCopying(true);
    setNotice(null);
    setReport(null);
    const source = otherRegion(region);
    try {
      // 「合并补齐」：目标侧缺的补，同名冲突保留目标侧（绝不静默覆盖）。
      const result = await api.copySkillsCross(source, region);
      setReport(result);
      await load();
    } catch (cause) {
      setNotice({
        type: "err",
        text: t("skillManager.copyCross.failed", { message: api.asError(cause) }),
      });
    } finally {
      setCopying(false);
    }
  }

  async function onImport() {
    const path = importPath.trim();
    if (!path || importing) return;
    setImporting(true);
    setNotice(null);
    try {
      const result = await api.importSkill(region, path);
      if (result.conflict) {
        setNotice({ type: "err", text: t("skillManager.import.conflict", { name: result.dirName }) });
      } else if (result.identical) {
        setNotice({ type: "ok", text: t("skillManager.import.identical", { name: result.dirName }) });
      } else {
        setNotice({ type: "ok", text: t("skillManager.import.done", { name: result.dirName }) });
        setImportPath("");
      }
      await load();
    } catch (cause) {
      setNotice({ type: "err", text: t("skillManager.import.failed", { message: api.asError(cause) }) });
    } finally {
      setImporting(false);
    }
  }

  async function onOpenDir() {
    setNotice(null);
    try {
      await api.openSkillsDir(region);
    } catch (cause) {
      setNotice({ type: "err", text: api.asError(cause) });
    }
  }

  return (
    <div className="max-h-[70vh] space-y-4 overflow-y-auto pr-1">
      {/* 顶部：区域 Tabs + 计数 + 动作 */}
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <Tabs value={region} onValueChange={(value) => setRegion(value as Region)}>
            <TabsList aria-label={t("skillManager.title")}>
              {REGIONS.map((value) => (
                <TabsTrigger key={value} value={value}>
                  {regionLabel(value)}
                </TabsTrigger>
              ))}
            </TabsList>
          </Tabs>
          {!loading && !error ? (
            <span className="text-xs text-muted-foreground tabular-nums">
              {t("skillManager.count", { n: skills.length })}
            </span>
          ) : null}
        </div>
        <div className="flex items-center gap-2">
          <DemoAction>
            <Button size="sm" variant="outline" onClick={() => void onOpenDir()}>
              <FolderOpen />
              {t("skillManager.openDir")}
            </Button>
          </DemoAction>
          <Button size="sm" variant="outline" onClick={() => void load()} disabled={loading}>
            {loading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
            {t("skillManager.reload")}
          </Button>
        </div>
      </div>

      {/* webui 直连远程服务端：明确标注「操作的是服务端机器上的目录」。 */}
      {api.isWebui() && !api.isDemoMode() ? (
        <Alert variant="warning">
          <AlertDescription>{t("skillManager.webuiNote")}</AlertDescription>
        </Alert>
      ) : null}

      {/* 清单 */}
      {loading && !data ? (
        <div className="space-y-2">
          {[0, 1, 2].map((index) => (
            <Skeleton key={index} className="h-14 w-full rounded-lg" />
          ))}
        </div>
      ) : error ? (
        <p className="text-sm text-destructive">{t("skillManager.error", { message: error })}</p>
      ) : skills.length === 0 ? (
        <EmptyState
          icon={Blocks}
          title={t("skillManager.empty.title")}
          description={t("skillManager.empty.description")}
        />
      ) : (
        <Card className="gap-0 overflow-hidden rounded-xl py-0 shadow-none">
          {skills.map((entry, index) => (
            <SkillRow
              key={entry.dirName}
              entry={entry}
              busy={busyDir === entry.dirName}
              locked={busyDir !== null && busyDir !== entry.dirName}
              isLast={index === skills.length - 1}
              onToggle={onToggle}
            />
          ))}
        </Card>
      )}

      {/* 底部：跨区搬迁 + 本地导入 */}
      <div className="space-y-3 border-t border-border pt-4">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <DemoAction>
            <Button size="sm" variant="outline" onClick={() => void onCopyCross()} disabled={copying}>
              {copying ? <Loader2 className="animate-spin" /> : <ArrowLeftRight />}
              {t("skillManager.copyCross")}
            </Button>
          </DemoAction>
          <span className="text-xs text-muted-foreground">
            {t("skillManager.copyCross.source", { source: regionLabel(otherRegion(region)) })}
          </span>
        </div>

        {report ? <CopyReportSummary report={report} /> : null}

        <div className="flex flex-wrap items-center gap-2">
          <Input
            value={importPath}
            onChange={(event) => setImportPath(event.target.value)}
            placeholder={t("skillManager.import.placeholder")}
            className="min-w-0 flex-1"
            spellCheck={false}
            autoComplete="off"
          />
          <DemoAction>
            <Button
              size="sm"
              variant="outline"
              onClick={() => void onImport()}
              disabled={importing || importPath.trim() === ""}
            >
              {importing ? <Loader2 className="animate-spin" /> : <Import />}
              {t("skillManager.import")}
            </Button>
          </DemoAction>
        </div>
      </div>

      {notice ? (
        <Alert variant={notice.type === "err" ? "destructive" : "default"}>
          <AlertDescription>{notice.text}</AlertDescription>
        </Alert>
      ) : null}
    </div>
  );
}

interface SkillRowProps {
  entry: SkillEntry;
  /** 本行正在切换（开关禁用 + 转圈）。 */
  busy: boolean;
  /** 别的行正在切换 ⇒ 本行也暂时禁用（避免并发写同一个目录）。 */
  locked: boolean;
  isLast: boolean;
  onToggle: (entry: SkillEntry, enabled: boolean) => void;
}

/** 一行 skill：名称 / 版本 / 来源徽章 + 右侧启停开关。 */
function SkillRow({ entry, busy, locked, isLast, onToggle }: SkillRowProps) {
  const t = useT();
  // 无合法 frontmatter ⇒ 不可启停（后端会拒绝，不擅自造 frontmatter）。
  const toggleable = entry.hasFrontmatter;
  const baseDescription = entry.description || (toggleable ? undefined : t("skillManager.noFrontmatter"));
  // `inconsistent`（两个启停字段只写了一半）额外提示「点击开关会一并写齐」（PRD §8.2）。
  const description = entry.inconsistent
    ? [baseDescription, t("skillManager.inconsistent.hint")].filter(Boolean).join(" ")
    : baseDescription;

  return (
    <SettingsFieldRow
      className={cn(isLast && "border-b-0")}
      operational
      label={
        <span className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
          <span className="truncate">{entry.name}</span>
          {entry.version ? (
            <span className="font-mono text-[11px] font-normal text-muted-foreground">
              {t("skillManager.version", { version: entry.version })}
            </span>
          ) : null}
          <Badge variant="secondary" className="font-normal">
            {entry.agentCreated ? t("skillManager.badge.agentCreated") : t("skillManager.badge.manual")}
          </Badge>
          {entry.inconsistent ? (
            <Badge variant="warning" className="font-normal">
              {t("skillManager.inconsistent")}
            </Badge>
          ) : null}
        </span>
      }
      description={description}
    >
      <Switch
        checked={entry.enabled}
        disabled={!toggleable || busy || locked}
        onCheckedChange={(value) => onToggle(entry, value)}
        aria-label={t("skillManager.toggle.aria", { name: entry.name })}
      />
    </SettingsFieldRow>
  );
}

/** 跨区搬迁结果摘要：新增 / 冲突跳过 / 已存在 + 冲突说明。 */
function CopyReportSummary({ report }: { report: SkillCopyReport }) {
  const t = useT();
  const total = report.copied.length + report.skippedConflict.length + report.skippedIdentical.length;

  if (total === 0) {
    return <p className="text-xs text-muted-foreground">{t("skillManager.copyCross.none")}</p>;
  }

  return (
    <div className="space-y-1 text-xs text-muted-foreground">
      <p className="font-medium text-foreground">{t("skillManager.copyCross.done")}</p>
      <p className="flex flex-wrap gap-x-3 gap-y-1">
        <span>{t("skillManager.copyCross.copied", { n: report.copied.length })}</span>
        <span>{t("skillManager.copyCross.conflict", { n: report.skippedConflict.length })}</span>
        <span>{t("skillManager.copyCross.identical", { n: report.skippedIdentical.length })}</span>
      </p>
      {report.skippedConflict.length > 0 ? (
        <p>{t("skillManager.copyCross.conflictNote")}</p>
      ) : null}
    </div>
  );
}