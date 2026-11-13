import { useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import type { RiskControlSettings } from "@/lib/types";

const FIELDS: { key: keyof RiskControlSettings; label: string; min: number; max: number; unit: string }[] = [
  { key: "requests_per_minute", label: "请求速率上限", min: 0, max: 600, unit: "次/分钟，0 关闭" },
  { key: "max_in_flight_per_account", label: "单账号并发", min: 1, max: 256, unit: "个请求" },
  { key: "max_in_flight_global", label: "平台总并发", min: 1, max: 256, unit: "个请求" },
  { key: "min_account_gap_ms", label: "账号最小请求间隔", min: 0, max: 3600000, unit: "毫秒" },
  { key: "max_transient_attempts", label: "临时故障最大重试次数", min: 0, max: 10, unit: "次，仅安全重试" },
  { key: "fallback_cooldown_secs", label: "限流默认冷却", min: 1, max: 2592000, unit: "秒" },
];

export function RiskControlsForm({
  region,
  value,
  saving,
  onSave,
}: {
  region: "cn" | "global";
  value: RiskControlSettings;
  saving: boolean;
  onSave: (value: RiskControlSettings) => Promise<void>;
}) {
  const [draft, setDraft] = useState(value);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => setDraft(value), [value]);

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    const invalid = FIELDS.find(({ key, min, max }) => !Number.isInteger(draft[key]) || draft[key] < min || draft[key] > max);
    if (invalid) {
      setError(`${invalid.label}范围为 ${invalid.min}–${invalid.max} ${invalid.unit}`);
      return;
    }
    setError(null);
    await onSave(draft);
  }

  return (
    <Card className="mt-4 p-4">
      <form onSubmit={(event) => void submit(event)}>
        <div className="mb-3">
          <h3 className="text-sm font-semibold">请求风控 · {region === "cn" ? "中国区" : "国际区"}</h3>
          <p className="mt-1 text-xs text-muted-foreground">仅约束本平台、本区域的请求。认证拒绝、封禁和限流不会触发自动换号。</p>
        </div>
        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {FIELDS.map(({ key, label, min, max, unit }) => (
            <label key={key} className="space-y-1 text-xs">
              <span className="font-medium">{label}</span>
              <Input
                aria-label={label}
                type="number"
                min={min}
                max={max}
                step={1}
                value={draft[key]}
                onChange={(event) => setDraft((current) => ({ ...current, [key]: Number(event.target.value) }))}
              />
              <span className="text-muted-foreground">{unit} · 范围 {min}–{max}</span>
            </label>
          ))}
        </div>
        {error && <p role="alert" className="mt-3 text-sm text-destructive">{error}</p>}
        <div className="mt-3 flex justify-end">
          <Button type="submit" size="sm" disabled={saving}>{saving ? "保存中…" : "保存风控设置"}</Button>
        </div>
      </form>
    </Card>
  );
}
