import { useLocation, useNavigate } from "react-router-dom";
import { GitBranch } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { useT } from "@/lib/i18n";
import { platformDescriptor, platformOfPath } from "@/lib/platforms/registry";
import { scopeOf } from "@/lib/scope";
import { useScope } from "@/lib/use-scope";
import type { TranslationKey } from "@/locales/zh";

/**
 * Trae 侧的「会话」目的地 —— **结构化空态**（`P8-ARCH.md` §1.3）。
 *
 * ## 为什么是这个页面（而不是复用 WorkBuddy 的 `SessionsPage`）
 *
 * 会话复制**只有 WorkBuddy 桌面**具备（`switch.rs:93`）⇒ 侧栏**不渲染**「会话」入口
 * （由 `DESTINATION_CAPABILITY` 的能力门控过滤，**不是**平台分支）。
 * 但 URL 直达（`/s/trae-cn/sessions`）仍可达（§3.2 规则 2：只是不渲染入口，不封路由）
 * ⇒ 本页给**结构化空态**：说明「该平台暂不支持会话管理」+ **原因**，并给**出口**。
 *
 * ## ★ 红线：**绝不**列出 WorkBuddy 的会话组
 *
 * 列出 WB 会话会让用户以为「会话已经复制过来了」（§3.15 能力边界）——
 * 所以本页**不**引用副本视图 / 会话命令，**不**渲染客户端 Tab 与状态筛选。
 */
export default function TraeSessionsPage() {
  const t = useT();
  const navigate = useNavigate();
  const location = useLocation();
  const product = platformOfPath(location.pathname);
  const [region] = useScope(product);
  /* 产品名走注册表（唯一来源），**不**写死 `TraeWork` —— 与 `PRODUCT_LABEL_KEY` 同一纪律。 */
  const productLabel = t(platformDescriptor(product).labelKey as TranslationKey);

  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-5 p-5">
      <header className="flex items-center gap-3">
        <div className="min-w-0">
          <h1 className="text-lg font-semibold tracking-[-0.01em]">{t("sessions.title")}</h1>
          <p className="mt-0.5 text-xs text-muted-foreground">{t("sessions.subtitle")}</p>
        </div>
      </header>

      <Card className="gap-0 py-0">
        {/* 四要素（§3.6）：图标 + 标题 + **说明原因** + 出口。
            说明必须讲清「为什么没有」——依赖平台自身的会话存储与云端归属（§3.5 不假承诺）。 */}
        <EmptyState
          icon={GitBranch}
          title={t("sessions.unsupported.title", { product: productLabel })}
          description={t("sessions.unsupported.description")}
          action={
            <Button
              variant="outline"
              size="sm"
              onClick={() => navigate(`/s/${scopeOf(product, region)}/accounts`)}
            >
              {t("sessions.unsupported.action")}
            </Button>
          }
        />
      </Card>
    </div>
  );
}
