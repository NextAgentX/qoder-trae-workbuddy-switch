import { useEffect, useState } from "react";
import { toast } from "sonner";

import { RiskControlsForm } from "@/components/gateway/risk-controls-form";
import * as api from "@/lib/api";
import { DEFAULT_RISK_CONTROL_SETTINGS } from "@/lib/risk-controls";
import type { RiskControlSettings } from "@/lib/types";

/**
 * 请求风控（跨三个平台共用的速率 / 并发 / 间隔参数）。
 *
 * ## 为什么放在网关工作台的「概览」里
 *
 * 这组参数只对**网关转发**生效（`docs/specs/RISK-CONTROLS-DESIGN.md` §A：三平台各进程内执行
 * 同一份参数），而网关的开关 / 端口 / 日志都在网关工作台。此前它挂在设置页的
 * 「通用」分区 —— 那里本该只放应用框架设置，而且同一个网关在各处出现了三份入口。
 * 现在跟随网关配置，用户在一处就能看到「网关怎么跑」。
 */
export function RiskControlsCard({ className }: { className?: string }) {
  const [settings, setSettings] = useState<RiskControlSettings>(DEFAULT_RISK_CONTROL_SETTINGS);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let active = true;
    void api
      .getRiskControlSettings()
      .then((value) => {
        if (active) setSettings(value);
      })
      .catch((error) => toast.error("Failed to load risk controls", { description: api.asError(error) }))
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, []);

  async function persist(value: RiskControlSettings) {
    setSaving(true);
    try {
      setSettings(await api.saveRiskControlSettings(value));
      toast.success("Risk controls saved");
    } catch (error) {
      toast.error("Failed to save risk controls", { description: api.asError(error) });
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className={className}>
      <RiskControlsForm value={settings} saving={loading || saving} onSave={persist} />
    </div>
  );
}
